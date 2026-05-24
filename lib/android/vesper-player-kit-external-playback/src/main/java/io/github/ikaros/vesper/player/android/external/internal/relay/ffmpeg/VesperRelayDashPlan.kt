package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import java.io.File
import java.io.IOException
import java.io.StringReader
import java.net.URI
import java.util.Locale
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Document
import org.w3c.dom.Element
import org.xml.sax.InputSource

internal data class VesperRelayDashPlan(
    val tracks: List<VesperRelayDashTrackPlan>,
)

internal data class VesperRelayDashTrackPlan(
    val kind: String,
    val mediaId: String,
    val mimeType: String?,
    val codecs: String?,
    val initializationUri: String?,
    val initializationRange: VesperRelayDashByteRange? = null,
    val segments: List<VesperRelayDashSegment>,
    val pipePath: String = "",
)

internal data class VesperRelayDashSegment(
    val index: Long,
    val uri: String,
    val byteRange: VesperRelayDashByteRange? = null,
)

private data class DashTemplate(
    val media: String,
    val initialization: String?,
    val startNumber: Long,
    val timescale: Long,
    val duration: Long,
)

private data class DashSegmentBase(
    val initialization: VesperRelayDashByteRange,
    val indexRange: VesperRelayDashByteRange,
) {
    fun toBridgeModel(): VesperRelayDashByteRangeSegmentBase =
        VesperRelayDashByteRangeSegmentBase(
            initialization = initialization,
            indexRange = indexRange,
        )
}

private data class DashTrackContext(
    val kind: String,
    val mediaId: String,
    val representationId: String,
    val mimeType: String?,
    val codecs: String?,
    val mediaBaseUri: String,
)

private class DashReferenceResolver(
    private val origin: VesperRelayDashSourceOrigin,
    private val baseDetails: Map<String, String>,
) {
    fun baseFor(parentBaseUri: String, element: Element): String =
        firstBaseUrl(element)
            ?.let { reference(parentBaseUri, it) }
            ?: parentBaseUri

    fun reference(baseUri: String, reference: String): String =
        resolveDashReference(baseUri, reference, origin, baseDetails)
}

internal fun planHostPreparedDash(
    manifestText: String,
    manifestUri: String,
    sourceOrigin: VesperRelayDashSourceOrigin = remoteDashOrigin(manifestUri),
    baseDetails: Map<String, String> = emptyMap(),
    resolver: VesperRelayDashResourceResolver = VesperRelayDashResourceResolver(
        origin = sourceOrigin,
        manifestLogicalUri = manifestUri,
    ),
): VesperRelayDashPlan {
    val document =
        try {
            parseXmlDocument(manifestText)
        } catch (error: Exception) {
            throw VesperRelayHostInputException(
                status = 415,
                diagnostic = VesperRelayDiagnostic(
                    code = "unsupported_dash_layout",
                    message = "DASH MPD could not be parsed for host-prepared relay remux.",
                    details = baseDetails + mapOf("hostError" to (error.message ?: error.javaClass.simpleName)),
                ),
            )
        }

    val manifestType = document.documentElement.getAttribute("type")
    val isDynamic = manifestType.isNotBlank() && !manifestType.equals("static", ignoreCase = true)
    val hasSegmentTemplate = document.hasDashElement("SegmentTemplate")
    val hasSegmentBase = document.hasDashElement("SegmentBase")
    if (isDynamic && !hasSegmentTemplate) {
        if (hasSegmentBase) {
            throw unsupportedDashLayout(
                baseDetails = baseDetails,
                message = "Dynamic DASH SegmentBase is not supported by host-prepared relay remux v1.",
                details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            )
        }
        throw unsupportedDynamicDash(baseDetails)
    }

    if (document.getElementsByTagNameNS("*", "ContentProtection").length > 0) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_encrypted_dash",
                message = "Encrypted DASH content cannot be remuxed for DLNA fallback.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }
    if (document.getElementsByTagNameNS("*", "SegmentTimeline").length > 0) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_dash_layout",
                message = "DASH SegmentTimeline is not supported by host-prepared relay remux v1.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }

    val durationSeconds = parseIso8601DurationSeconds(
        document.documentElement.getAttribute("mediaPresentationDuration"),
    )

    val periods = childElementsByTagName(document.documentElement, "Period")
    if (periods.size > 1) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_dash_layout",
                message = "Multiple DASH periods are not supported by host-prepared relay remux v1.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }

    val referenceResolver = DashReferenceResolver(sourceOrigin, baseDetails)
    val mpdBase = referenceResolver.baseFor(manifestUri, document.documentElement)
    val period = periods.firstOrNull()
    val periodBase = period?.let { referenceResolver.baseFor(mpdBase, it) } ?: mpdBase
    val adaptationSets =
        if (period != null) {
            childElementsByTagName(period, "AdaptationSet")
        } else {
            childElementsByTagName(document.documentElement, "AdaptationSet")
        }

    val planned = mutableListOf<VesperRelayDashTrackPlan>()
    val selectedKinds = mutableSetOf<String>()
    adaptationSets.forEachIndexed { index, adaptation ->
        val representations = childElementsByTagName(adaptation, "Representation")
        val selectedRepresentation = selectRepresentation(representations, isDynamic) ?: return@forEachIndexed
        val kind = dashMediaKind(adaptation, selectedRepresentation) ?: return@forEachIndexed
        if (kind in selectedKinds) {
            return@forEachIndexed
        }
        val representationId =
            selectedRepresentation.getAttribute("id").takeIf(String::isNotBlank) ?: "$kind$index"
        val mediaId = "$kind$index"
        val adaptationBase = referenceResolver.baseFor(periodBase, adaptation)
        val representationBase = referenceResolver.baseFor(adaptationBase, selectedRepresentation)
        val trackContext = DashTrackContext(
            kind = kind,
            mediaId = mediaId,
            representationId = representationId,
            mimeType = selectedRepresentation.getAttribute("mimeType").takeIf(String::isNotBlank)
                ?: adaptation.getAttribute("mimeType").takeIf(String::isNotBlank),
            codecs = selectedRepresentation.getAttribute("codecs").takeIf(String::isNotBlank)
                ?: adaptation.getAttribute("codecs").takeIf(String::isNotBlank),
            mediaBaseUri = representationBase,
        )
        val template = dashTemplateFromElement(selectedRepresentation)
            ?: dashTemplateFromElement(adaptation)
        when {
            template != null ->
                planned += planSegmentTemplateTrack(
                    context = trackContext,
                    template = template,
                    durationSeconds = durationSeconds,
                    baseDetails = baseDetails,
                    referenceResolver = referenceResolver,
                )
            isDynamic -> {
                return@forEachIndexed
            }
            else -> {
                val segmentBase = dashSegmentBaseFromElement(selectedRepresentation, baseDetails, kind, mediaId)
                    ?: dashSegmentBaseFromElement(adaptation, baseDetails, kind, mediaId)
                    ?: throw unsupportedDashLayout(
                        baseDetails = baseDetails,
                        message = "Host-prepared relay remux v1 requires SegmentTemplate or SegmentBase tracks.",
                        details = mapOf("trackKind" to kind, "mediaId" to representationId),
                    )
                planned += planSegmentBaseTrack(
                    context = trackContext,
                    segmentBase = segmentBase,
                    baseDetails = baseDetails,
                    resolver = resolver,
                )
            }
        }
        selectedKinds += kind
    }

    if (planned.none { it.kind == "video" }) {
        if (isDynamic && hasSegmentBase) {
            throw unsupportedDashLayout(
                baseDetails = baseDetails,
                message = "Dynamic DASH SegmentBase is not supported by host-prepared relay remux v1.",
                details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            )
        }
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH MPD did not contain a supported video SegmentBase or SegmentTemplate representation.",
            details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
        )
    }
    return VesperRelayDashPlan(planned.sortedBy { if (it.kind == "video") 0 else 1 })
}

private fun selectRepresentation(
    representations: List<Element>,
    isDynamic: Boolean,
): Element? {
    if (!isDynamic) {
        return representations.firstOrNull()
    }
    return representations.firstOrNull { dashTemplateFromElement(it) != null }
        ?: representations.firstOrNull()
}

private fun planSegmentTemplateTrack(
    context: DashTrackContext,
    template: DashTemplate,
    durationSeconds: Double?,
    baseDetails: Map<String, String>,
    referenceResolver: DashReferenceResolver,
): VesperRelayDashTrackPlan {
    val finiteDurationSeconds = durationSeconds
        ?: throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_dash_layout",
                message = "Host-prepared relay remux requires a finite DASH mediaPresentationDuration.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    validateTemplate(template, baseDetails, context.kind, context.representationId)
    val segmentSeconds = template.duration.toDouble() / template.timescale.coerceAtLeast(1L).toDouble()
    val segmentCount = kotlin.math.ceil(finiteDurationSeconds / segmentSeconds)
        .toLong()
        .coerceAtLeast(1L)
    if (segmentCount > Int.MAX_VALUE) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate expands to too many segments for relay remux v1.",
            details = mapOf("trackKind" to context.kind, "mediaId" to context.representationId),
        )
    }
    val initializationUri = template.initialization?.let { initialization ->
        referenceResolver.reference(
            context.mediaBaseUri,
            expandDashTemplate(initialization, context.representationId, template.startNumber),
        )
    }
    val segments = (0 until segmentCount).map { offset ->
        val number = template.startNumber + offset
        VesperRelayDashSegment(
            index = number,
            uri = referenceResolver.reference(
                context.mediaBaseUri,
                expandDashTemplate(template.media, context.representationId, number),
            ),
        )
    }
    return VesperRelayDashTrackPlan(
        kind = context.kind,
        mediaId = context.mediaId,
        mimeType = context.mimeType,
        codecs = context.codecs,
        initializationUri = initializationUri,
        segments = segments,
    )
}

private fun planSegmentBaseTrack(
    context: DashTrackContext,
    segmentBase: DashSegmentBase,
    baseDetails: Map<String, String>,
    resolver: VesperRelayDashResourceResolver,
): VesperRelayDashTrackPlan {
    val mediaSegments =
        try {
            val sidxBytes = resolver.readRange(context.mediaBaseUri, segmentBase.indexRange)
            val sidx = VesperRelayDashBridgeApiProvider.parseSidx(sidxBytes)
            VesperRelayDashBridgeApiProvider.mediaSegments(segmentBase.toBridgeModel(), sidx)
        } catch (error: IOException) {
            throw VesperRelayHostInputException(
                status = error.dashResourceHttpStatus(),
                diagnostic = VesperRelayDiagnostic(
                    code = error.dashResourceErrorCode(),
                    message = "Failed to fetch DASH sidx for host-prepared relay remux.",
                    details = baseDetails
                        .withSegmentHash(context.mediaBaseUri)
                        .withHostError(error.message ?: error.javaClass.simpleName),
                ),
            )
        } catch (error: Exception) {
            throw unsupportedDashLayout(
                baseDetails = baseDetails,
                message = "DASH SegmentBase sidx could not be parsed for host-prepared relay remux.",
                details = mapOf(
                    "trackKind" to context.kind,
                    "mediaId" to context.mediaId,
                    "segmentUriHash" to hashForDiagnostic(context.mediaBaseUri),
                    "hostError" to (error.message ?: error.javaClass.simpleName),
                ),
            )
        }

    return VesperRelayDashTrackPlan(
        kind = context.kind,
        mediaId = context.mediaId,
        mimeType = context.mimeType,
        codecs = context.codecs,
        initializationUri = context.mediaBaseUri,
        initializationRange = segmentBase.initialization,
        segments = mediaSegments.mapIndexed { index, segment ->
            VesperRelayDashSegment(
                index = index.toLong(),
                uri = context.mediaBaseUri,
                byteRange = segment.range,
            )
        },
    )
}

private fun validateTemplate(
    template: DashTemplate,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
) {
    if (template.duration <= 0L || template.timescale <= 0L) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate duration and timescale must be greater than zero.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (template.initialization.isNullOrBlank()) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "Host-prepared fMP4 DASH remux requires an initialization segment.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (!DASH_NUMBER_TOKEN.containsMatchIn(template.media) || template.media.contains("${'$'}Time")) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "Host-prepared relay remux v1 requires SegmentTemplate media with Number tokens.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
}

private fun unsupportedDashLayout(
    baseDetails: Map<String, String>,
    message: String,
    details: Map<String, String>,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_dash_layout",
            message = message,
            details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE) + details,
        ),
    )

private fun unsupportedDynamicDash(baseDetails: Map<String, String>): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_dynamic_dash",
            message = "Dynamic DASH MPD is not supported by host-prepared relay remux v1.",
            details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
        ),
    )

private fun unsupportedMixedDashOrigin(
    baseDetails: Map<String, String>,
    origin: VesperRelayDashSourceOrigin,
    resolvedUri: String,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_mixed_dash_origin",
            message = "DASH references must stay within the source origin for relay remux.",
            details = baseDetails + mapOf(
                "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
                "sourceOrigin" to origin.kind,
                "resourceUriHash" to hashForDiagnostic(resolvedUri),
            ),
        ),
    )

private fun parseXmlDocument(xmlText: String): Document {
    val factory = DocumentBuilderFactory.newInstance().apply {
        isNamespaceAware = true
        runCatching { setFeature("http://apache.org/xml/features/disallow-doctype-decl", true) }
        runCatching { setFeature("http://xml.org/sax/features/external-general-entities", false) }
        runCatching { setFeature("http://xml.org/sax/features/external-parameter-entities", false) }
    }
    return factory
        .newDocumentBuilder()
        .parse(InputSource(StringReader(xmlText)))
}

private fun dashTemplateFromElement(element: Element): DashTemplate? {
    val template = childElementsByTagName(element, "SegmentTemplate").firstOrNull() ?: return null
    return DashTemplate(
        media = template.getAttribute("media").takeIf(String::isNotBlank) ?: return null,
        initialization = template.getAttribute("initialization").takeIf(String::isNotBlank),
        startNumber = template.getAttribute("startNumber").toLongOrNull() ?: 1L,
        timescale = template.getAttribute("timescale").toLongOrNull() ?: 1L,
        duration = template.getAttribute("duration").toLongOrNull() ?: 0L,
    )
}

private fun dashSegmentBaseFromElement(
    element: Element,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
): DashSegmentBase? {
    val segmentBase = childElementsByTagName(element, "SegmentBase").firstOrNull() ?: return null
    val indexRangeValue = segmentBase.getAttribute("indexRange").takeIf(String::isNotBlank)
        ?: throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentBase requires an indexRange.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    val initializationRangeValue = childElementsByTagName(segmentBase, "Initialization")
        .firstOrNull()
        ?.getAttribute("range")
        ?.takeIf(String::isNotBlank)
        ?: throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentBase requires an Initialization range.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    val indexRange = parseDashByteRange(indexRangeValue, "SegmentBase indexRange", baseDetails, kind, mediaId)
    val initializationRange = parseDashByteRange(
        initializationRangeValue,
        "SegmentBase Initialization range",
        baseDetails,
        kind,
        mediaId,
    )
    return DashSegmentBase(
        initialization = initializationRange,
        indexRange = indexRange,
    )
}

private fun parseDashByteRange(
    value: String,
    field: String,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
): VesperRelayDashByteRange {
    val separator = value.indexOf('-')
    if (separator <= 0 || separator == value.lastIndex) {
        throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    }
    val startText = value.substring(0, separator)
    val endText = value.substring(separator + 1)
    val start = startText.trim().toLongOrNull()
        ?: throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    val end = endText.trim().toLongOrNull()
        ?: throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    if (start < 0L || end < start) {
        throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    }
    return VesperRelayDashByteRange(start = start, end = end)
}

private fun invalidDashByteRange(
    value: String,
    field: String,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
): VesperRelayHostInputException =
    unsupportedDashLayout(
        baseDetails = baseDetails,
        message = "$field is invalid for host-prepared relay remux.",
        details = mapOf("trackKind" to kind, "mediaId" to mediaId, "byteRange" to value),
    )

private fun dashMediaKind(
    adaptation: Element,
    representation: Element,
): String? {
    val mimeType = sequenceOf(
        representation.getAttribute("mimeType"),
        adaptation.getAttribute("mimeType"),
        adaptation.getAttribute("contentType"),
    ).firstOrNull { it.isNotBlank() }
    when {
        mimeType?.startsWith("video/", ignoreCase = true) == true -> return "video"
        mimeType?.startsWith("audio/", ignoreCase = true) == true -> return "audio"
        mimeType.equals("video", ignoreCase = true) -> return "video"
        mimeType.equals("audio", ignoreCase = true) -> return "audio"
    }
    val codecs = representation.getAttribute("codecs").lowercase(Locale.US)
    return when {
        codecs.startsWith("mp4a") || codecs.startsWith("ac-3") || codecs.startsWith("ec-3") -> "audio"
        codecs.startsWith("avc") ||
            codecs.startsWith("hvc") ||
            codecs.startsWith("hev") ||
            codecs.startsWith("av01") -> "video"
        else -> null
    }
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

private fun Document.hasDashElement(tagName: String): Boolean =
    getElementsByTagNameNS("*", tagName).length > 0 || getElementsByTagName(tagName).length > 0

private fun firstBaseUrl(element: Element): String? =
    childElementsByTagName(element, "BaseURL")
        .firstOrNull()
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }

private fun parseIso8601DurationSeconds(value: String?): Double? {
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

private val DASH_NUMBER_TOKEN = Regex("""\${'$'}Number(?:%0(\d+)d)?\${'$'}""")

private fun expandDashTemplate(
    template: String,
    representationId: String,
    number: Long,
): String =
    DASH_NUMBER_TOKEN
        .replace(template.replace("${'$'}RepresentationID${'$'}", representationId)) { match ->
            val width = match.groupValues.getOrNull(1)?.toIntOrNull()
            if (width == null) {
                number.toString()
            } else {
                number.toString().padStart(width, '0')
            }
        }

private fun resolveDashReference(
    baseUri: String,
    reference: String,
    origin: VesperRelayDashSourceOrigin,
    baseDetails: Map<String, String>,
): String {
    val resolved =
        runCatching {
            val ref = URI(reference)
            if (ref.isAbsolute || baseUri.isBlank()) {
                ref.toString()
            } else {
                URI(baseUri).resolve(ref).normalize().toString()
            }
        }.getOrElse { reference }
    val scheme = resolved.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US)
    return when (origin.kind) {
        "remote" -> {
            if (scheme == "http" || scheme == "https") {
                resolved
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        "file" -> {
            if (resolved.isRemoteDashUri() && origin.allowRemoteMediaReferences) {
                resolved
            } else if (scheme == "file") {
                val root = File(URI(origin.rootUri)).canonicalFile.toPath()
                val candidate = File(URI(resolved)).canonicalFile.toPath()
                if (candidate.startsWith(root)) {
                    resolved
                } else {
                    throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
                }
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        "content" -> {
            if (resolved.isRemoteDashUri() && origin.allowRemoteMediaReferences) {
                resolved
            } else if (scheme == "content" && contentUriWithinRoot(resolved, origin.rootUri)) {
                resolved
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        else -> throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
    }
}

private fun remoteDashOrigin(uri: String): VesperRelayDashSourceOrigin =
    VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = uri,
        rootUri = uri,
    )
