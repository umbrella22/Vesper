package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

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

internal data class DashTemplate(
    val media: String,
    val initialization: String?,
    val startNumber: Long,
    val timescale: Long,
    val duration: Long,
)

internal data class DashSegmentBase(
    val initialization: VesperRelayDashByteRange,
    val indexRange: VesperRelayDashByteRange,
) {
    fun toBridgeModel(): VesperRelayDashByteRangeSegmentBase =
        VesperRelayDashByteRangeSegmentBase(
            initialization = initialization,
            indexRange = indexRange,
        )
}

internal data class DashTrackContext(
    val kind: String,
    val mediaId: String,
    val representationId: String,
    val mimeType: String?,
    val codecs: String?,
    val mediaBaseUri: String,
)

internal class DashReferenceResolver(
    private val origin: VesperRelayDashSourceOrigin,
    private val baseDetails: Map<String, String>,
) {
    fun baseFor(parentBaseUri: String, element: org.w3c.dom.Element): String =
        firstBaseUrl(element)
            ?.let { reference(parentBaseUri, it) }
            ?: parentBaseUri

    fun reference(baseUri: String, reference: String): String =
        resolveDashReference(baseUri, reference, origin, baseDetails)
}
