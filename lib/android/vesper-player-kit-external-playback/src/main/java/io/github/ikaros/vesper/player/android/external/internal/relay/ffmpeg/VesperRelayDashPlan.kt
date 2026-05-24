package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic

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
