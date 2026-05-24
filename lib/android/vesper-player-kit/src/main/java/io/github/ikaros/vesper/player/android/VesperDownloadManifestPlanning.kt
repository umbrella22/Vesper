package io.github.ikaros.vesper.player.android

import java.io.StringReader
import java.net.URI
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Element
import org.xml.sax.InputSource

internal data class HlsMasterPlaylist(
    val variants: List<HlsVariant>,
    val audio: List<HlsRendition>,
)

internal data class HlsVariant(
    val uri: String,
    val attributes: Map<String, String>,
)

internal data class HlsRendition(
    val uri: String,
    val attributes: Map<String, String>,
)

internal data class HlsMediaPlaylist(
    val targetDuration: String?,
    val version: String?,
    val maps: List<HlsMap>,
    val segments: List<HlsSegment>,
)

internal data class HlsMap(
    val uri: String,
    val byteRange: VesperDownloadByteRange?,
)

internal data class HlsSegment(
    val uri: String,
    val duration: String?,
    val byteRange: VesperDownloadByteRange?,
    val sequence: Long,
)

internal data class DashPlannedRepresentation(
    val id: String,
    val mediaId: String,
    val mimeType: String?,
    val codecs: String?,
    val bandwidth: String?,
    val baseUri: String,
    val baseUrl: String?,
    val template: DashTemplate?,
)

internal data class DashTemplate(
    val media: String,
    val initialization: String?,
    val startNumber: Long,
    val timescale: Long,
    val duration: Long,
)

internal fun parseHlsMasterPlaylist(
    manifestUri: String,
    manifestText: String,
): HlsMasterPlaylist {
    val variants = mutableListOf<HlsVariant>()
    val audio = mutableListOf<HlsRendition>()
    var pendingVariant: Map<String, String>? = null

    manifestText.lineSequence().map(String::trim).filter(String::isNotEmpty).forEach { line ->
        when {
            line.startsWith("#EXT-X-STREAM-INF:", ignoreCase = true) -> {
                pendingVariant = parseHlsAttributes(line.substringAfter(':', ""))
            }
            line.startsWith("#EXT-X-MEDIA:", ignoreCase = true) -> {
                val attributes = parseHlsAttributes(line.substringAfter(':', ""))
                val uri = attributes["URI"]
                if (uri != null && attributes["TYPE"]?.equals("AUDIO", ignoreCase = true) == true) {
                    audio += HlsRendition(resolveRemoteReference(manifestUri, uri), attributes)
                }
            }
            line.startsWith("#") -> Unit
            pendingVariant != null -> {
                variants += HlsVariant(resolveRemoteReference(manifestUri, line), pendingVariant.orEmpty())
                pendingVariant = null
            }
        }
    }

    return HlsMasterPlaylist(variants = variants, audio = audio)
}

internal fun parseHlsMediaPlaylist(
    playlistUri: String,
    playlistText: String,
): HlsMediaPlaylist {
    var targetDuration: String? = null
    var version: String? = null
    var endList = false
    var playlistTypeVod = false
    var pendingDuration: String? = null
    var pendingByteRange: VesperDownloadByteRange? = null
    var previousRangeEnd = 0L
    var sequence = 0L
    val maps = mutableListOf<HlsMap>()
    val segments = mutableListOf<HlsSegment>()

    playlistText.lineSequence().map(String::trim).filter(String::isNotEmpty).forEach { line ->
        when {
            line.startsWith("#EXT-X-TARGETDURATION:", ignoreCase = true) -> {
                targetDuration = line.substringAfter(':').trim()
            }
            line.startsWith("#EXT-X-VERSION:", ignoreCase = true) -> {
                version = line.substringAfter(':').trim()
            }
            line.equals("#EXT-X-ENDLIST", ignoreCase = true) -> {
                endList = true
            }
            line.startsWith("#EXT-X-PLAYLIST-TYPE:", ignoreCase = true) -> {
                playlistTypeVod = line.substringAfter(':').trim().equals("VOD", ignoreCase = true)
            }
            line.startsWith("#EXT-X-MAP:", ignoreCase = true) -> {
                val attributes = parseHlsAttributes(line.substringAfter(':', ""))
                val uri = attributes["URI"] ?: error("HLS EXT-X-MAP was missing URI")
                val byteRange = attributes["BYTERANGE"]?.let { parseHlsByteRange(it, previousRangeEnd) }
                if (byteRange != null) {
                    previousRangeEnd = byteRange.offset + byteRange.length
                }
                maps += HlsMap(resolveRemoteReference(playlistUri, uri), byteRange)
            }
            line.startsWith("#EXT-X-BYTERANGE:", ignoreCase = true) -> {
                pendingByteRange = parseHlsByteRange(line.substringAfter(':').trim(), previousRangeEnd)
                pendingByteRange?.let { previousRangeEnd = it.offset + it.length }
            }
            line.startsWith("#EXTINF:", ignoreCase = true) -> {
                pendingDuration = line.substringAfter(':').substringBefore(',').trim()
            }
            line.startsWith("#") -> Unit
            else -> {
                sequence += 1
                segments +=
                    HlsSegment(
                        uri = resolveRemoteReference(playlistUri, line),
                        duration = pendingDuration,
                        byteRange = pendingByteRange,
                        sequence = sequence,
                    )
                pendingDuration = null
                pendingByteRange = null
            }
        }
    }

    if (!endList && !playlistTypeVod) {
        error("HLS download preparation requires a VOD playlist or EXT-X-ENDLIST")
    }
    if (segments.isEmpty()) {
        error("HLS media playlist did not contain any segments")
    }
    return HlsMediaPlaylist(
        targetDuration = targetDuration,
        version = version,
        maps = maps,
        segments = segments,
    )
}

private fun parseHlsAttributes(input: String): Map<String, String> {
    val result = linkedMapOf<String, String>()
    var start = 0
    var inQuotes = false
    input.forEachIndexed { index, character ->
        if (character == '"') {
            inQuotes = !inQuotes
        }
        if (character == ',' && !inQuotes) {
            parseAttributePair(input.substring(start, index))?.let { (key, value) -> result[key] = value }
            start = index + 1
        }
    }
    parseAttributePair(input.substring(start))?.let { (key, value) -> result[key] = value }
    return result
}

private fun parseAttributePair(input: String): Pair<String, String>? {
    val key = input.substringBefore('=', "").trim().takeIf { it.isNotEmpty() } ?: return null
    val value = input.substringAfter('=', "").trim().trim('"')
    return key to value
}

private fun parseHlsByteRange(
    value: String,
    previousRangeEnd: Long,
): VesperDownloadByteRange? {
    val lengthText = value.substringBefore('@').trim()
    val offsetText = value.substringAfter('@', "").trim()
    val length = lengthText.toLongOrNull() ?: return null
    val offset = offsetText.toLongOrNull() ?: previousRangeEnd
    return VesperDownloadByteRange(offset = offset, length = length)
}

internal fun rewriteHlsMaster(
    variantAttributes: Map<String, String>,
    mediaResourceNames: List<String>,
): String {
    val audioPlaylist = mediaResourceNames.firstOrNull { it.startsWith("audio") }
    val videoPlaylist = mediaResourceNames.firstOrNull { it.startsWith("video") }
        ?: mediaResourceNames.firstOrNull()
        ?: "video.m3u8"
    val bandwidth = variantAttributes["BANDWIDTH"] ?: "1"
    return buildString {
        append("#EXTM3U\n#EXT-X-VERSION:3\n")
        if (audioPlaylist != null) {
            append("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"audio\",DEFAULT=YES,AUTOSELECT=YES,URI=\"")
            append(audioPlaylist)
            append("\"\n")
            append("#EXT-X-STREAM-INF:BANDWIDTH=$bandwidth,AUDIO=\"audio\"\n")
        } else {
            append("#EXT-X-STREAM-INF:BANDWIDTH=$bandwidth\n")
        }
        append(videoPlaylist)
        append('\n')
    }
}

internal fun rewriteHlsMedia(
    mediaId: String,
    playlist: HlsMediaPlaylist,
    localMaps: Map<String, String>,
): String =
    buildString {
        append("#EXTM3U\n")
        append("#EXT-X-VERSION:${playlist.version ?: "3"}\n")
        append("#EXT-X-PLAYLIST-TYPE:VOD\n")
        playlist.targetDuration?.let { append("#EXT-X-TARGETDURATION:$it\n") }
        playlist.maps.lastOrNull()?.let { map ->
            localMaps["${map.uri}:${map.byteRange}"]?.let { path ->
                append("#EXT-X-MAP:URI=\"$path\"\n")
            }
        }
        playlist.segments.forEach { segment ->
            append("#EXTINF:${segment.duration ?: "0"},\n")
            append("segments/$mediaId-${segment.sequence.toString().padStart(5, '0')}.${extensionFromUri(segment.uri, "ts")}\n")
        }
        append("#EXT-X-ENDLIST\n")
    }

internal fun parseXmlDocument(xmlText: String) =
    DocumentBuilderFactory
        .newInstance()
        .apply { isNamespaceAware = true }
        .newDocumentBuilder()
        .parse(InputSource(StringReader(xmlText)))

internal fun selectDashRepresentations(
    document: org.w3c.dom.Document,
    manifestUri: String,
    profile: VesperDownloadProfile,
): List<DashPlannedRepresentation> {
    val mpdBase = childElementsByTagName(document.documentElement, "BaseURL")
        .firstOrNull()
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }
        ?.let { resolveRemoteReference(manifestUri, it) }
        ?: manifestUri
    val result = mutableListOf<DashPlannedRepresentation>()
    val adaptationSets = document.getElementsByTagNameNS("*", "AdaptationSet")
    for (index in 0 until adaptationSets.length) {
        val adaptation = adaptationSets.item(index) as? Element ?: continue
        val adaptationMimeType = adaptation.getAttribute("mimeType").takeIf(String::isNotBlank)
        if (adaptationMimeType != null &&
            !adaptationMimeType.startsWith("video/") &&
            !adaptationMimeType.startsWith("audio/")
        ) {
            continue
        }
        val adaptationBase = childElementsByTagName(adaptation, "BaseURL")
            .firstOrNull()
            ?.textContent
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
            ?.let { resolveRemoteReference(mpdBase, it) }
            ?: mpdBase
        val adaptationTemplate = dashTemplateFromElement(adaptation)
        val representations = childElementsByTagName(adaptation, "Representation")
        val selectedRepresentation =
            profile.variantId
                ?.let { variantId -> representations.firstOrNull { it.getAttribute("id") == variantId } }
                ?: representations.firstOrNull()
                ?: continue
        val id = selectedRepresentation.getAttribute("id").takeIf(String::isNotBlank) ?: index.toString()
        val representationBase = childElementsByTagName(selectedRepresentation, "BaseURL")
            .firstOrNull()
            ?.textContent
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
        val template = dashTemplateFromElement(selectedRepresentation) ?: adaptationTemplate
        val mimeType = selectedRepresentation.getAttribute("mimeType").takeIf(String::isNotBlank) ?: adaptationMimeType
        val mediaKind = when {
            mimeType?.startsWith("audio/") == true -> "audio"
            mimeType?.startsWith("video/") == true -> "video"
            else -> "media"
        }
        result +=
            DashPlannedRepresentation(
                id = id,
                mediaId = "$mediaKind$index",
                mimeType = mimeType,
                codecs = selectedRepresentation.getAttribute("codecs").takeIf(String::isNotBlank),
                bandwidth = selectedRepresentation.getAttribute("bandwidth").takeIf(String::isNotBlank),
                baseUri = representationBase?.let { resolveRemoteReference(adaptationBase, it) } ?: adaptationBase,
                baseUrl = if (template == null) representationBase else null,
                template = template,
            )
    }
    return result
}

private fun dashTemplateFromElement(element: Element): DashTemplate? {
    val template = childElementsByTagName(element, "SegmentTemplate").firstOrNull() ?: return null
    val media = template.getAttribute("media").takeIf(String::isNotBlank) ?: return null
    return DashTemplate(
        media = media,
        initialization = template.getAttribute("initialization").takeIf(String::isNotBlank),
        startNumber = template.getAttribute("startNumber").toLongOrNull() ?: 1L,
        timescale = template.getAttribute("timescale").toLongOrNull() ?: 1L,
        duration = template.getAttribute("duration").toLongOrNull() ?: 0L,
    )
}

private fun childElementsByTagName(
    parent: Element,
    tagName: String,
): List<Element> =
    buildList {
        val children = parent.childNodes
        for (index in 0 until children.length) {
            val child = children.item(index) as? Element ?: continue
            if (child.localName == tagName || child.tagName == tagName) {
                add(child)
            }
        }
    }

internal fun parseIso8601DurationSeconds(value: String?): Double? {
    if (value.isNullOrBlank() || !value.startsWith("PT")) {
        return null
    }
    var number = ""
    var total = 0.0
    value.drop(2).forEach { character ->
        if (character.isDigit() || character == '.') {
            number += character
            return@forEach
        }
        val parsed = number.toDoubleOrNull() ?: return null
        number = ""
        when (character) {
            'H' -> total += parsed * 3600.0
            'M' -> total += parsed * 60.0
            'S' -> total += parsed
            else -> return null
        }
    }
    return total.takeIf { it > 0.0 }
}

internal fun expandDashTemplate(
    template: String,
    representationId: String,
    number: Long,
): String =
    replaceDashNumberToken(template.replace("\$RepresentationID\$", representationId), number)

private fun replaceDashNumberToken(
    value: String,
    number: Long,
): String {
    var output = value.replace("\$Number\$", number.toString())
    while (true) {
        val start = output.indexOf("\$Number%")
        if (start < 0) {
            return output
        }
        val end = output.indexOf('$', start + "\$Number%".length)
        if (end < 0) {
            return output
        }
        val spec = output.substring(start + "\$Number%".length, end)
        val width = spec.removeSuffix("d").removePrefix("0").toIntOrNull() ?: 0
        output = output.replaceRange(start, end + 1, number.toString().padStart(width, '0'))
    }
}

internal fun rewriteDashMpd(
    duration: String?,
    adaptationSets: List<String>,
): String =
    buildString {
        append("<MPD type=\"static\"")
        duration?.takeIf { it.isNotBlank() }?.let { append(" mediaPresentationDuration=\"").append(escapeXml(it)).append('"') }
        append(" xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period>")
        adaptationSets.forEach(::append)
        append("</Period></MPD>\n")
    }

internal fun rewriteDashTemplateAdaptationSet(
    representation: DashPlannedRepresentation,
    template: DashTemplate,
    mediaId: String,
    segmentCount: Long,
): String {
    val mime = representation.mimeType?.let { " mimeType=\"${escapeXml(it)}\"" }.orEmpty()
    val codecs = representation.codecs?.let { " codecs=\"${escapeXml(it)}\"" }.orEmpty()
    val bandwidth = representation.bandwidth ?: "1"
    val initialization = template.initialization?.let { " initialization=\"segments/$mediaId-init.mp4\"" }.orEmpty()
    return "<AdaptationSet$mime><Representation id=\"${escapeXml(representation.id)}\" bandwidth=\"$bandwidth\"$codecs><SegmentTemplate timescale=\"${template.timescale}\" duration=\"${template.duration}\" startNumber=\"${template.startNumber}\"$initialization media=\"segments/$mediaId-\$Number\$.m4s\" /></Representation></AdaptationSet><!-- plannedSegments=$segmentCount -->"
}

internal fun rewriteDashSegmentBaseAdaptationSet(
    representation: DashPlannedRepresentation,
    localName: String,
): String {
    val mime = representation.mimeType?.let { " mimeType=\"${escapeXml(it)}\"" }.orEmpty()
    val codecs = representation.codecs?.let { " codecs=\"${escapeXml(it)}\"" }.orEmpty()
    val bandwidth = representation.bandwidth ?: "1"
    return "<AdaptationSet$mime><Representation id=\"${escapeXml(representation.id)}\" bandwidth=\"$bandwidth\"$codecs><BaseURL>${escapeXml(localName)}</BaseURL><SegmentBase /></Representation></AdaptationSet>"
}

internal fun parseFlvClipManifest(
    baseUri: String,
    manifestText: String,
): List<String> =
    manifestText
        .lineSequence()
        .map(String::trim)
        .filter { it.isNotEmpty() && !it.startsWith("#") && !it.equals("ffconcat version 1.0", ignoreCase = true) }
        .map { line ->
            line.removePrefix("file")
                .trim()
                .trim('\'', '"')
        }
        .filter(String::isNotEmpty)
        .map { resolveRemoteReference(baseUri, it) }
        .toList()

internal fun resolveRemoteReference(
    baseUri: String,
    reference: String,
): String =
    runCatching {
        val ref = URI(reference)
        if (ref.isAbsolute || baseUri.isBlank()) {
            ref.toString()
        } else {
            URI(baseUri).resolve(ref).toString()
        }
    }.getOrElse { reference }

internal fun extensionFromUri(
    uri: String,
    fallback: String,
): String {
    val name = lastPathSegmentFromUri(uri) ?: return fallback
    return name.substringAfterLast('.', "").takeIf { it.isNotBlank() && it != name } ?: fallback
}

private fun escapeXml(value: String): String =
    value
        .replace("&", "&amp;")
        .replace("\"", "&quot;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")

internal fun escapeFfconcatPath(path: String): String = path.replace("'", "'\\''")

