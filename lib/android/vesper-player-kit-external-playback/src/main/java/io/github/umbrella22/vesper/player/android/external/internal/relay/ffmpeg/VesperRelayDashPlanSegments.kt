package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import java.io.IOException

internal fun selectRepresentation(
    representations: List<org.w3c.dom.Element>,
    isDynamic: Boolean,
): org.w3c.dom.Element? {
    if (!isDynamic) {
        return representations.firstOrNull()
    }
    return representations.firstOrNull { childElementsByTagName(it, "SegmentTemplate").isNotEmpty() }
        ?: representations.firstOrNull()
}

internal fun planSegmentTemplateTrack(
    context: DashTrackContext,
    template: DashTemplate,
    manifestType: String,
    durationMs: Long?,
    baseDetails: Map<String, String>,
    referenceResolver: DashReferenceResolver,
): VesperRelayDashTrackPlan {
    validateTemplate(template, baseDetails, context.kind, context.representationId)
    if (template.timeline.isEmpty() && durationMs == null) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "Host-prepared relay remux requires a finite DASH mediaPresentationDuration for fixed-duration templates.",
            details = mapOf("trackKind" to context.kind, "mediaId" to context.mediaId),
        )
    }
    val templateSegments = try {
        VesperRelayDashBridgeApiProvider.templateSegments(
            // Preserve the existing finite snapshot route for dynamic fixed-duration MPDs.
            manifestType = if (template.timeline.isEmpty()) "static" else manifestType,
            durationMs = durationMs,
            template = template,
        )
    } catch (error: VesperRelayDashSegmentLimitException) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate expands to too many segments for relay remux v1.",
            details = mapOf(
                "trackKind" to context.kind,
                "mediaId" to context.representationId,
                "segmentCount" to error.segmentCount.toString(),
                "maxSegmentsPerTrack" to MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK.toString(),
            ),
        )
    } catch (error: Exception) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate could not be expanded for host-prepared relay remux.",
            details = mapOf(
                "trackKind" to context.kind,
                "mediaId" to context.representationId,
                "hostError" to (error.message ?: error.javaClass.simpleName),
            ),
        )
    }
    if (templateSegments.size > MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate expands to too many segments for relay remux v1.",
            details = mapOf(
                "trackKind" to context.kind,
                "mediaId" to context.representationId,
                "segmentCount" to templateSegments.size.toString(),
                "maxSegmentsPerTrack" to MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK.toString(),
            ),
        )
    }
    val initializationUri = template.initialization?.let { initialization ->
        referenceResolver.reference(
            context.mediaBaseUri,
            expandDashTemplate(
                template = initialization,
                context = context,
                number = templateSegments.firstOrNull()?.number ?: template.startNumber,
                time = templateSegments.firstOrNull()?.time,
                baseDetails = baseDetails,
            ),
        )
    }
    val segments = templateSegments.map { segment ->
        VesperRelayDashSegment(
            index = segment.number,
            uri = referenceResolver.reference(
                context.mediaBaseUri,
                expandDashTemplate(
                    template = template.media,
                    context = context,
                    number = segment.number,
                    time = segment.time,
                    baseDetails = baseDetails,
                ),
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
                .also { segments ->
                    if (segments.size > MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK) {
                        throw VesperRelayDashSegmentLimitException(segments.size)
                    }
                }
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
        } catch (error: VesperRelayDashSegmentLimitException) {
            throw unsupportedDashLayout(
                baseDetails = baseDetails,
                message = "DASH SegmentBase expands to too many segments for relay remux v1.",
                details = mapOf(
                    "trackKind" to context.kind,
                    "mediaId" to context.mediaId,
                    "segmentCount" to error.segmentCount.toString(),
                    "maxSegmentsPerTrack" to MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK.toString(),
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
    if (template.timescale <= 0L || (template.duration != null && template.duration <= 0L)) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate duration and timescale must be greater than zero.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (template.duration == null && template.timeline.isEmpty()) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate requires duration or SegmentTimeline.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (template.startNumber < 0L || template.presentationTimeOffset < 0L) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate numbering values must be non-negative.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (template.timeline.any { entry ->
            entry.startTime != null && entry.startTime < 0L ||
                entry.duration <= 0L || entry.repeatCount < -1
        }
    ) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTimeline contains an invalid entry.",
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
}

private fun expandDashTemplate(
    template: String,
    context: DashTrackContext,
    number: Long?,
    time: Long?,
    baseDetails: Map<String, String>,
): String =
    try {
        VesperRelayDashBridgeApiProvider.expandTemplate(
            template = template,
            representationId = context.representationId,
            bandwidth = context.bandwidth,
            number = number,
            time = time,
        )
    } catch (error: Exception) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate URI could not be expanded for host-prepared relay remux.",
            details = mapOf(
                "trackKind" to context.kind,
                "mediaId" to context.representationId,
                "templateHash" to hashForDiagnostic(template),
                "hostError" to (error.message ?: error.javaClass.simpleName),
            ),
        )
    }
