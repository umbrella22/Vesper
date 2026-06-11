package io.github.ikaros.vesper.player.android

enum class VesperPlaybackCapabilityProbeStatus {
    Supported,
    FallbackRequired,
    Unsupported,
    Unknown,
}

enum class VesperPlaybackCodecFamily {
    H264,
    Hevc,
    Av1,
    Vvc,
    Unknown,
}

enum class VesperPlaybackCapabilityOutputFormat {
    Nv12,
    P010,
    SurfaceOpaque,
    Unknown,
}

enum class VesperPlaybackCapabilityHdrKind {
    None,
    Hdr10,
    Hlg,
    DolbyVision,
    Unknown,
}

enum class VesperPlaybackCapabilityDolbyVisionMode {
    None,
    FullChainCandidate,
    CompatibleBaseLayer,
    Unsupported,
}

enum class VesperPlaybackCapabilityConfidence {
    CodecOnly,
    SourceMetadata,
    SessionProbe,
}

enum class VesperRecommendedPlaybackPath {
    NativeFramePipeline,
    SystemPlayer,
}

data class VesperAndroidSessionProbeResult(
    val supportedHdrKinds: Set<VesperPlaybackCapabilityHdrKind> = emptySet(),
    val diagnostics: Map<String, String> = emptyMap(),
)

fun interface VesperAndroidSessionProbeProvider {
    fun probe(request: VesperPlaybackCapabilityProbeRequest): VesperAndroidSessionProbeResult?
}

data class VesperPlaybackCapabilityProbeRequest(
    val source: VesperPlayerSource? = null,
    val codec: String? = null,
    val width: Int? = null,
    val height: Int? = null,
    val frameRate: Float? = null,
    val requiresNativeFrame: Boolean = false,
    val sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
        VesperSourceNormalizerConfiguration(),
    val frameProcessorConfiguration: VesperFrameProcessorConfiguration =
        VesperFrameProcessorConfiguration(),
    val nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(),
)

data class VesperHdrChromaticityPoint(
    val x: Double,
    val y: Double,
)

data class VesperPlaybackCapabilityHdrMetadata(
    val hdrKind: VesperPlaybackCapabilityHdrKind? = null,
    val dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode? = null,
    val probe: String? = null,
    val codec: String? = null,
    val sampleMimeType: String? = null,
    val colorPrimaries: String? = null,
    val colorSpace: String? = null,
    val colorRange: String? = null,
    val transferFunction: String? = null,
    val yCbCrMatrix: String? = null,
    val alternativeTransferCharacteristics: String? = null,
    val lumaBitDepth: Int? = null,
    val chromaBitDepth: Int? = null,
    val hdrStaticInfoPresent: Boolean? = null,
    val hdrStaticInfoByteLength: Int? = null,
    val hdrStaticInfoParseError: String? = null,
    val maxContentLightLevelNits: Int? = null,
    val maxFrameAverageLightLevelNits: Int? = null,
    val masteringDisplayColorVolumePresent: Boolean? = null,
    val masteringDisplayColorVolumeByteLength: Int? = null,
    val masteringDisplayColorVolumeParseError: String? = null,
    val masteringDisplayPrimary0: VesperHdrChromaticityPoint? = null,
    val masteringDisplayPrimary1: VesperHdrChromaticityPoint? = null,
    val masteringDisplayPrimary2: VesperHdrChromaticityPoint? = null,
    val masteringDisplayWhitePoint: VesperHdrChromaticityPoint? = null,
    val masteringDisplayMaxLuminanceNits: Double? = null,
    val masteringDisplayMinLuminanceNits: Double? = null,
    val dolbyVisionCodec: String? = null,
    val dolbyVisionProfile: Int? = null,
    val dolbyVisionLevel: Int? = null,
    val dolbyVisionCompatibility: String? = null,
    val dolbyVisionProfileFamily: String? = null,
    val dolbyVisionBaseLayer: String? = null,
    val dolbyVisionFallbackTarget: String? = null,
    val dolbyVisionBaseLayerEvidence: String? = null,
    val dolbyVisionBaseLayerTransferFunction: String? = null,
) {
    internal val isEmpty: Boolean
        get() =
            hdrKind == null &&
                dolbyVisionMode == null &&
                probe == null &&
                codec == null &&
                sampleMimeType == null &&
                colorPrimaries == null &&
                colorSpace == null &&
                colorRange == null &&
                transferFunction == null &&
                yCbCrMatrix == null &&
                alternativeTransferCharacteristics == null &&
                lumaBitDepth == null &&
                chromaBitDepth == null &&
                hdrStaticInfoPresent == null &&
                hdrStaticInfoByteLength == null &&
                hdrStaticInfoParseError == null &&
                maxContentLightLevelNits == null &&
                maxFrameAverageLightLevelNits == null &&
                masteringDisplayColorVolumePresent == null &&
                masteringDisplayColorVolumeByteLength == null &&
                masteringDisplayColorVolumeParseError == null &&
                masteringDisplayPrimary0 == null &&
                masteringDisplayPrimary1 == null &&
                masteringDisplayPrimary2 == null &&
                masteringDisplayWhitePoint == null &&
                masteringDisplayMaxLuminanceNits == null &&
                masteringDisplayMinLuminanceNits == null &&
                dolbyVisionCodec == null &&
                dolbyVisionProfile == null &&
                dolbyVisionLevel == null &&
                dolbyVisionCompatibility == null &&
                dolbyVisionProfileFamily == null &&
                dolbyVisionBaseLayer == null &&
                dolbyVisionFallbackTarget == null &&
                dolbyVisionBaseLayerEvidence == null &&
                dolbyVisionBaseLayerTransferFunction == null
}

data class VesperPlaybackCapabilityProbeResult(
    val status: VesperPlaybackCapabilityProbeStatus,
    val codecFamily: VesperPlaybackCodecFamily,
    val systemPlaybackSupported: Boolean,
    val hardwareDecodeSupported: Boolean,
    val sdkManagedNativeFrameSupported: Boolean,
    val recommendedPlaybackPath: VesperRecommendedPlaybackPath,
    val outputFormat: VesperPlaybackCapabilityOutputFormat,
    val hdrKind: VesperPlaybackCapabilityHdrKind,
    val dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode,
    val confidence: VesperPlaybackCapabilityConfidence,
    val missingCapabilities: List<String> = emptyList(),
    val diagnostics: Map<String, String> = emptyMap(),
    val hdrMetadata: VesperPlaybackCapabilityHdrMetadata? = null,
)

internal data class AndroidDolbyVisionCodecInfo(
    val codec: String,
    val profile: Int?,
    val level: Int?,
) {
    val dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode
        get() = matrix.dolbyVisionMode

    val diagnostics: Map<String, String>
        get() =
            buildMap {
                put("dolbyVisionCodec", codec)
                profile?.let { put("dolbyVisionProfile", it.toString()) }
                level?.let { put("dolbyVisionLevel", it.toString()) }
                put("dolbyVisionCompatibility", matrix.compatibility)
                put("dolbyVisionProfileFamily", matrix.profileFamily)
                put("dolbyVisionBaseLayer", matrix.baseLayer)
                put("dolbyVisionFallbackTarget", matrix.fallbackTarget)
            }

    val matrix: AndroidDolbyVisionProfileMatrix
        get() = AndroidDolbyVisionProfileMatrix.fromProfile(profile)
}

internal data class AndroidDolbyVisionProfileMatrix(
    val dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode,
    val compatibility: String,
    val profileFamily: String,
    val baseLayer: String,
    val fallbackTarget: String,
) {
    companion object {
        fun fromProfile(profile: Int?): AndroidDolbyVisionProfileMatrix =
            when (profile) {
                5 ->
                    AndroidDolbyVisionProfileMatrix(
                        dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.Unsupported,
                        compatibility = "noCompatibleBaseLayer",
                        profileFamily = "profile5SingleLayer",
                        baseLayer = "none",
                        fallbackTarget = "dolbyVisionSystemPlayer",
                    )
                7 ->
                    AndroidDolbyVisionProfileMatrix(
                        dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
                        compatibility = "dualLayerBaseLayerCandidate",
                        profileFamily = "profile7DualLayer",
                        baseLayer = "hdr10BaseLayerCandidate",
                        fallbackTarget = "hdr10BaseLayerSystemPlayer",
                    )
                8 ->
                    AndroidDolbyVisionProfileMatrix(
                        dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
                        compatibility = "compatibleBaseLayerCandidate",
                        profileFamily = "profile8SingleLayerCompatible",
                        baseLayer = "compatibleBaseLayerUnknown",
                        fallbackTarget = "compatibleBaseLayerSystemPlayer",
                    )
                9 ->
                    AndroidDolbyVisionProfileMatrix(
                        dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.Unsupported,
                        compatibility = "unknownProfile",
                        profileFamily = "profile9ConservativeUnknown",
                        baseLayer = "unknown",
                        fallbackTarget = "unknownSystemPlayer",
                    )
                null ->
                    AndroidDolbyVisionProfileMatrix(
                        dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.Unsupported,
                        compatibility = "profileUnknown",
                        profileFamily = "profileUnknown",
                        baseLayer = "unknown",
                        fallbackTarget = "unknownSystemPlayer",
                    )
                else ->
                    AndroidDolbyVisionProfileMatrix(
                        dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.Unsupported,
                        compatibility = "unknownProfile",
                        profileFamily = "unknownProfile",
                        baseLayer = "unknown",
                        fallbackTarget = "unknownSystemPlayer",
                    )
            }
    }
}

fun interface VesperAndroidCodecProbeProvider {
    fun hasHardwareDecoder(mimeType: String?): Boolean
}
