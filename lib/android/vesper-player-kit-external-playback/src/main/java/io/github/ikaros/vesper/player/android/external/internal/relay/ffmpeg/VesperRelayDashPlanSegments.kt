package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import java.io.IOException

internal fun selectRepresentation(
    representations: List<org.w3c.dom.Element>,
    isDynamic: Boolean,
): org.w3c.dom.Element? {
    if (!isDynamic) {
        return representations.firstOrNull()
    }
    return representations.firstOrNull { dashTemplateFromElement(it) != null }
        ?: representations.firstOrNull()
}

internal fun planSegmentTemplateTrack(
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

internal fun planSegmentBaseTrack(
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
