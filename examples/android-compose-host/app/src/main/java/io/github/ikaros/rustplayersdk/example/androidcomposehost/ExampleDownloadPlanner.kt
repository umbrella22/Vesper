package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Context
import io.github.ikaros.vesper.player.android.VesperDownloadAssetIndex
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadProfile
import io.github.ikaros.vesper.player.android.VesperDownloadResourceRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSegmentRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import java.io.File
import java.io.StringReader
import java.net.URI
import javax.xml.parsers.DocumentBuilderFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.w3c.dom.Document
import org.w3c.dom.Element
import org.xml.sax.InputSource

internal data class ExamplePreparedDownloadTask(
    val source: VesperDownloadSource,
    val profile: VesperDownloadProfile,
    val assetIndex: VesperDownloadAssetIndex,
)

internal fun exampleDraftDownloadLabel(source: VesperPlayerSource): String =
    source.label.takeIf { it.isNotBlank() } ?: exampleDraftDownloadLabel(source.uri)

internal fun exampleDraftDownloadLabel(uri: String): String {
    val parsedUri = runCatching { URI(uri) }.getOrNull()
    val pathSegments =
        parsedUri?.path
            ?.split('/')
            ?.filter { it.isNotBlank() }
            .orEmpty()
    val fileName = pathSegments.lastOrNull()
    val parentDirectory = pathSegments.dropLast(1).lastOrNull()
    val rawCandidate =
        when {
            fileName.isNullOrBlank() -> parsedUri?.host
            fileName.lowercase() in GENERIC_MANIFEST_FILE_NAMES && !parentDirectory.isNullOrBlank() ->
                parentDirectory
            '.' in fileName -> fileName.substringBeforeLast('.')
            else -> fileName
        } ?: parsedUri?.host
    return rawCandidate
        ?.replace('_', ' ')
        ?.replace('-', ' ')
        ?.trim()
        ?.takeIf { it.isNotBlank() }
        ?: uri
}

internal suspend fun prepareExampleDownloadTask(
    context: Context,
    assetId: String,
    source: VesperPlayerSource,
): ExamplePreparedDownloadTask = withContext(Dispatchers.IO) {
    when (source.protocol) {
        VesperPlayerSourceProtocol.Hls -> prepareHlsDownloadTask(context, assetId, source)
        VesperPlayerSourceProtocol.Dash -> prepareDashDownloadTask(context, assetId, source)
        else ->
            ExamplePreparedDownloadTask(
                source = VesperDownloadSource(source = source),
                profile = VesperDownloadProfile(),
                assetIndex = VesperDownloadAssetIndex(),
            )
    }
}

private fun prepareHlsDownloadTask(
    context: Context,
    assetId: String,
    source: VesperPlayerSource,
): ExamplePreparedDownloadTask {
    val manifestUri = source.uri
    val manifestText = fetchRemoteText(manifestUri)
    val targetDirectory = exampleDownloadTargetDirectory(context, assetId)
    val resourceRecords = linkedMapOf<String, VesperDownloadResourceRecord>()
    val segmentRecords = linkedMapOf<String, VesperDownloadSegmentRecord>()

    fun addResource(uri: String) {
        val relativePath = relativePathForRemoteUri(uri)
        resourceRecords.putIfAbsent(
            relativePath,
            VesperDownloadResourceRecord(
                resourceId = relativePath,
                uri = uri,
                relativePath = relativePath,
            ),
        )
    }

    fun addSegment(uri: String, sequence: Long?) {
        val relativePath = relativePathForRemoteUri(uri)
        segmentRecords.putIfAbsent(
            relativePath,
            VesperDownloadSegmentRecord(
                segmentId = relativePath,
                uri = uri,
                relativePath = relativePath,
                sequence = sequence,
            ),
        )
    }

    fun addPlaylistEntry(entry: HlsPlaylistEntry) {
        when (entry.kind) {
            HlsPlaylistEntryKind.Resource -> addResource(entry.uri)
            HlsPlaylistEntryKind.Segment -> addSegment(entry.uri, entry.sequence)
        }
    }

    addResource(manifestUri)

    val parsedMaster = parseHlsMasterManifest(manifestText, manifestUri)
    var primaryPlaylistText: String? = null
    if (parsedMaster != null) {
        addResource(parsedMaster.variantPlaylistUri)
        parsedMaster.audioPlaylistUri?.let(::addResource)

        val videoPlaylistText = fetchRemoteText(parsedMaster.variantPlaylistUri)
        primaryPlaylistText = videoPlaylistText
        parseHlsMediaPlaylist(videoPlaylistText, parsedMaster.variantPlaylistUri)
            .forEach(::addPlaylistEntry)

        parsedMaster.audioPlaylistUri?.let { audioPlaylistUri ->
            val audioPlaylistText = fetchRemoteText(audioPlaylistUri)
            parseHlsMediaPlaylist(audioPlaylistText, audioPlaylistUri)
                .forEach(::addPlaylistEntry)
        }
    } else {
        primaryPlaylistText = manifestText
        parseHlsMediaPlaylist(manifestText, manifestUri).forEach(::addPlaylistEntry)
    }

    val preparedSourceLabel =
        resolvePreparedHlsLabel(
            originalSource = source,
            manifestUri = manifestUri,
            manifestText = manifestText,
            primaryPlaylistText = primaryPlaylistText,
        )

    return ExamplePreparedDownloadTask(
        source =
            VesperDownloadSource(
                source = source.copy(label = preparedSourceLabel),
                contentFormat = VesperDownloadContentFormat.HlsSegments,
                manifestUri = manifestUri,
            ),
        profile = VesperDownloadProfile(targetDirectory = targetDirectory.absolutePath),
        assetIndex =
            VesperDownloadAssetIndex(
                contentFormat = VesperDownloadContentFormat.HlsSegments,
                resources = resourceRecords.values.toList(),
                segments = segmentRecords.values.toList(),
            ),
    )
}

private fun prepareDashDownloadTask(
    context: Context,
    assetId: String,
    source: VesperPlayerSource,
): ExamplePreparedDownloadTask {
    val manifestUri = source.uri
    val manifestText = fetchRemoteText(manifestUri)
    val manifestDocument = parseXmlDocument(manifestText)
    val targetDirectory = exampleDownloadTargetDirectory(context, assetId)
    val resourceRecords = linkedMapOf<String, VesperDownloadResourceRecord>()
    val segmentRecords = linkedMapOf<String, VesperDownloadSegmentRecord>()

    fun addResource(uri: String) {
        val relativePath = relativePathForRemoteUri(uri)
        resourceRecords.putIfAbsent(
            relativePath,
            VesperDownloadResourceRecord(
                resourceId = relativePath,
                uri = uri,
                relativePath = relativePath,
            ),
        )
    }

    fun addSegment(uri: String, sequence: Long) {
        val relativePath = relativePathForRemoteUri(uri)
        segmentRecords.putIfAbsent(
            relativePath,
            VesperDownloadSegmentRecord(
                segmentId = relativePath,
                uri = uri,
                relativePath = relativePath,
                sequence = sequence,
            ),
        )
    }

    addResource(manifestUri)

    val presentationDurationSeconds =
        parseIso8601DurationSeconds(
            manifestDocument.documentElement.getAttribute("mediaPresentationDuration"),
        )
    val adaptationSets = manifestDocument.getElementsByTagNameNS("*", "AdaptationSet")
    var nextSequence = 0L
    for (index in 0 until adaptationSets.length) {
        val adaptation = adaptationSets.item(index) as? Element ?: continue
        val mimeType = adaptation.getAttribute("mimeType")
        if (!mimeType.startsWith("video/") && !mimeType.startsWith("audio/")) {
            continue
        }

        val selectedRepresentation =
            childElementsByTagName(adaptation, "Representation").firstOrNull() ?: continue
        val template =
            childElementsByTagName(selectedRepresentation, "SegmentTemplate").firstOrNull()
                ?: childElementsByTagName(adaptation, "SegmentTemplate").firstOrNull()
                ?: continue

        val representationId = selectedRepresentation.getAttribute("id")
        if (representationId.isBlank()) {
            continue
        }
        val initializationTemplate = template.getAttribute("initialization")
        if (initializationTemplate.isBlank()) {
            continue
        }
        val mediaTemplate = template.getAttribute("media")
        if (mediaTemplate.isBlank()) {
            continue
        }
        val startNumber = template.getAttribute("startNumber").toLongOrNull() ?: 1L
        val timescale = template.getAttribute("timescale").toLongOrNull() ?: 1L
        val duration = template.getAttribute("duration").toLongOrNull() ?: continue
        val segmentCount =
            presentationDurationSeconds
                ?.let { seconds ->
                    kotlin.math.ceil(seconds * timescale.toDouble() / duration.toDouble()).toLong()
                }
                ?.coerceAtLeast(1L)
                ?: 1L

        val initializationUri =
            resolveRemoteReference(
                manifestUri,
                initializationTemplate.replace("\$RepresentationID\$", representationId),
            )
        addResource(initializationUri)

        repeat(segmentCount.toInt()) { offset ->
            val segmentNumber = startNumber + offset
            val segmentUri =
                resolveRemoteReference(
                    manifestUri,
                    mediaTemplate
                        .replace("\$RepresentationID\$", representationId)
                        .replace("\$Number\$", segmentNumber.toString()),
                )
            addSegment(segmentUri, nextSequence)
            nextSequence += 1
        }
    }

    val preparedSourceLabel =
        resolvePreparedDashLabel(
            originalSource = source,
            manifestUri = manifestUri,
            manifestDocument = manifestDocument,
        )

    return ExamplePreparedDownloadTask(
        source =
            VesperDownloadSource(
                source = source.copy(label = preparedSourceLabel),
                contentFormat = VesperDownloadContentFormat.DashSegments,
                manifestUri = manifestUri,
            ),
        profile = VesperDownloadProfile(targetDirectory = targetDirectory.absolutePath),
        assetIndex =
            VesperDownloadAssetIndex(
                contentFormat = VesperDownloadContentFormat.DashSegments,
                resources = resourceRecords.values.toList(),
                segments = segmentRecords.values.toList(),
            ),
    )
}

private fun exampleDownloadTargetDirectory(
    context: Context,
    assetId: String,
): File =
    File(context.filesDir, "vesper-downloads/$assetId").apply {
        mkdirs()
    }

private fun resolvePreparedHlsLabel(
    originalSource: VesperPlayerSource,
    manifestUri: String,
    manifestText: String,
    primaryPlaylistText: String?,
): String {
    val draftLabel = exampleDraftDownloadLabel(manifestUri)
    if (originalSource.label.isNotBlank() && originalSource.label != draftLabel) {
        return originalSource.label
    }
    return extractHlsManifestTitle(manifestText)
        ?: primaryPlaylistText?.let(::extractHlsManifestTitle)
        ?: draftLabel
}

private fun resolvePreparedDashLabel(
    originalSource: VesperPlayerSource,
    manifestUri: String,
    manifestDocument: Document,
): String {
    val draftLabel = exampleDraftDownloadLabel(manifestUri)
    if (originalSource.label.isNotBlank() && originalSource.label != draftLabel) {
        return originalSource.label
    }
    return extractDashManifestTitle(manifestDocument) ?: draftLabel
}

private fun extractHlsManifestTitle(manifestText: String): String? =
    manifestText.lineSequence()
        .map(String::trim)
        .firstNotNullOfOrNull { line ->
            if (!line.startsWith("#EXT-X-SESSION-DATA", ignoreCase = true)) {
                return@firstNotNullOfOrNull null
            }
            val attributes = parseAttributeList(line.substringAfter(':', ""))
            val dataId = attributes["DATA-ID"]?.lowercase()
            val title =
                attributes["VALUE"]
                    ?.trim()
                    ?.takeIf { it.isNotBlank() }
            if (dataId?.contains("title") == true) title else null
        }

private fun extractDashManifestTitle(manifestDocument: Document): String? {
    val titles = manifestDocument.getElementsByTagNameNS("*", "Title")
    for (index in 0 until titles.length) {
        val title = titles.item(index)?.textContent?.trim()
        if (!title.isNullOrBlank()) {
            return title
        }
    }
    return null
}

private data class HlsMasterSelection(
    val variantPlaylistUri: String,
    val audioPlaylistUri: String?,
)

private enum class HlsPlaylistEntryKind {
    Resource,
    Segment,
}

private data class HlsPlaylistEntry(
    val kind: HlsPlaylistEntryKind,
    val uri: String,
    val sequence: Long? = null,
)

private fun parseHlsMasterManifest(
    manifestText: String,
    manifestUri: String,
): HlsMasterSelection? {
    val audioPlaylists = linkedMapOf<String, MutableList<String>>()
    val variants = mutableListOf<Pair<Long, Pair<String, String?>>>()
    var pendingVariantBandwidth: Long? = null
    var pendingAudioGroupId: String? = null

    manifestText.lineSequence().forEach { line ->
        val trimmed = line.trim()
        when {
            trimmed.startsWith("#EXT-X-MEDIA", ignoreCase = true) -> {
                val attributes = parseAttributeList(trimmed.substringAfter(':', ""))
                if (attributes["TYPE"] == "AUDIO") {
                    val groupId = attributes["GROUP-ID"] ?: return@forEach
                    val uri = attributes["URI"] ?: return@forEach
                    audioPlaylists.getOrPut(groupId) { mutableListOf() }
                        .add(resolveRemoteReference(manifestUri, uri))
                }
            }
            trimmed.startsWith("#EXT-X-STREAM-INF", ignoreCase = true) -> {
                val attributes = parseAttributeList(trimmed.substringAfter(':', ""))
                pendingVariantBandwidth = attributes["BANDWIDTH"]?.toLongOrNull()
                pendingAudioGroupId = attributes["AUDIO"]
            }
            pendingVariantBandwidth != null && trimmed.isNotEmpty() && !trimmed.startsWith("#") -> {
                variants +=
                    pendingVariantBandwidth to (
                        resolveRemoteReference(manifestUri, trimmed) to pendingAudioGroupId
                    )
                pendingVariantBandwidth = null
                pendingAudioGroupId = null
            }
        }
    }

    val selectedVariant = variants.firstOrNull()?.second ?: return null
    val audioPlaylistUri =
        selectedVariant.second
            ?.let { groupId -> audioPlaylists[groupId]?.firstOrNull() }
    return HlsMasterSelection(
        variantPlaylistUri = selectedVariant.first,
        audioPlaylistUri = audioPlaylistUri,
    )
}

private fun parseHlsMediaPlaylist(
    playlistText: String,
    playlistUri: String,
): List<HlsPlaylistEntry> {
    val entries = mutableListOf<HlsPlaylistEntry>()
    var nextSequence = 0L
    playlistText.lineSequence().forEach { line ->
        val trimmed = line.trim()
        when {
            trimmed.startsWith("#EXT-X-MEDIA-SEQUENCE", ignoreCase = true) -> {
                nextSequence = trimmed.substringAfter(':').toLongOrNull() ?: nextSequence
            }
            trimmed.startsWith("#EXT-X-KEY", ignoreCase = true) ||
                trimmed.startsWith("#EXT-X-MAP", ignoreCase = true) -> {
                val attributes = parseAttributeList(trimmed.substringAfter(':', ""))
                val uri = attributes["URI"] ?: return@forEach
                entries +=
                    HlsPlaylistEntry(
                        kind = HlsPlaylistEntryKind.Resource,
                        uri = resolveRemoteReference(playlistUri, uri),
                    )
            }
            trimmed.isNotEmpty() && !trimmed.startsWith("#") -> {
                entries +=
                    HlsPlaylistEntry(
                        kind = HlsPlaylistEntryKind.Segment,
                        uri = resolveRemoteReference(playlistUri, trimmed),
                        sequence = nextSequence,
                    )
                nextSequence += 1
            }
        }
    }
    return entries
}

private fun parseAttributeList(line: String): Map<String, String> {
    val result = linkedMapOf<String, String>()
    ATTRIBUTE_PATTERN.findAll(line).forEach { match ->
        result[match.groupValues[1]] =
            match.groupValues[3].ifBlank { match.groupValues[2] }
                .trim()
                .trim('"')
    }
    return result
}

private fun parseXmlDocument(xmlText: String): Document =
    DocumentBuilderFactory
        .newInstance()
        .apply { isNamespaceAware = true }
        .newDocumentBuilder()
        .parse(InputSource(StringReader(xmlText)))

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

private fun parseIso8601DurationSeconds(value: String?): Double? {
    if (value.isNullOrBlank()) {
        return null
    }
    val match = ISO8601_DURATION_PATTERN.matchEntire(value) ?: return null
    val hours = match.groupValues[1].toDoubleOrNull() ?: 0.0
    val minutes = match.groupValues[2].toDoubleOrNull() ?: 0.0
    val seconds = match.groupValues[3].toDoubleOrNull() ?: 0.0
    return hours * 3600.0 + minutes * 60.0 + seconds
}

private fun relativePathForRemoteUri(uri: String): String {
    val path =
        runCatching { URI(uri).path }
            .getOrNull()
            ?.trim()
            .orEmpty()
            .trimStart('/')
    return path.ifBlank {
        uri.substringAfterLast('/').ifBlank { "download.bin" }
    }
}

private val ATTRIBUTE_PATTERN = Regex("""([A-Z0-9-]+)=("([^"]*)"|[^,]*)""")
private val ISO8601_DURATION_PATTERN = Regex("""PT(?:([0-9.]+)H)?(?:([0-9.]+)M)?(?:([0-9.]+)S)?""")
private val GENERIC_MANIFEST_FILE_NAMES =
    setOf(
        "master.m3u8",
        "playlist.m3u8",
        "index.m3u8",
        "prog_index.m3u8",
        "manifest.mpd",
        "stream.mpd",
    )
