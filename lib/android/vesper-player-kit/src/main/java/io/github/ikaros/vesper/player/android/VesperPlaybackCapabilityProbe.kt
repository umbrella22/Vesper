package io.github.ikaros.vesper.player.android

import android.content.Context
import android.hardware.display.DisplayManager
import android.os.Build
import android.util.Log
import android.view.Display
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes

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

object VesperPlaybackCapabilityProbe {
    fun probe(
        request: VesperPlaybackCapabilityProbeRequest,
        codecProbeProvider: VesperAndroidCodecProbeProvider =
            VesperAndroidCodecProbeProvider { mimeType ->
                VesperHardwareMediaCodecSelector.hasHardwareDecoder(mimeType)
            },
        sessionProbeProvider: VesperAndroidSessionProbeProvider? = null,
    ): VesperPlaybackCapabilityProbeResult {
        val codecFamily = request.codec.toPlaybackCodecFamily()
        val effectiveRequiresNativeFrame =
            request.requiresNativeFrame ||
                request.nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.PreferNativeFrame ||
                request.nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
        val sourceIsRemote = request.source?.kind == VesperPlayerSourceKind.Remote
        val sourceIsLocal = request.source?.kind == VesperPlayerSourceKind.Local
        val mimeType = codecFamily.androidMimeType
        val hardwareDecodeSupported = codecProbeProvider.hasHardwareDecoder(mimeType)
        val systemPlaybackSupported = sourceIsRemote || sourceIsLocal || codecFamily != VesperPlaybackCodecFamily.Unknown
        val hdrKind = request.codec.detectHdrKind()
        val dolbyVisionCodecInfo = request.codec.detectDolbyVisionCodecInfo()
        val isHdrOrDolbyVision = hdrKind != VesperPlaybackCapabilityHdrKind.None &&
            hdrKind != VesperPlaybackCapabilityHdrKind.Unknown
        val sessionProbeResult =
            if (isHdrOrDolbyVision) {
                sessionProbeProvider?.probe(request)
            } else {
                null
            }
        val missing = mutableListOf<String>()
        val diagnostics =
            linkedMapOf(
                "probeVersion" to "1",
                "sourceKind" to (request.source?.kind?.wireName ?: "unknown"),
                "sourceProtocol" to (request.source?.protocol?.wireName ?: "unknown"),
            )
        sessionProbeResult?.diagnostics?.let(diagnostics::putAll)
        dolbyVisionCodecInfo?.diagnostics?.let(diagnostics::putAll)
        diagnostics.applyDolbyVisionProfile8Refinement()

        if (request.codec.isNullOrBlank()) {
            missing += "codecMetadata"
        }
        if (codecFamily == VesperPlaybackCodecFamily.Unknown) {
            missing += "codecFamily"
        }
        if (effectiveRequiresNativeFrame && sourceIsRemote) {
            missing += "hostManagedNetworkProbeNotImplemented"
        }
        if (effectiveRequiresNativeFrame && request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty()) {
            missing += "nativeFrameDecoderPlugin"
        }
        if (effectiveRequiresNativeFrame && !hardwareDecodeSupported) {
            missing += "deviceHardwareDecode"
        }
        if (isHdrOrDolbyVision) {
            missing += "hdrProgrammableProcessingNotSupported"
            diagnostics["playbackPathPolicy"] = "hdrSystemPlaybackOnly"
            diagnostics["recommendedPlaybackPathReason"] = "hdrNativeFrameUnsupported"
            val displayHdrProbeAvailable =
                sessionProbeResult != null &&
                    (
                        sessionProbeResult.diagnostics[DISPLAY_HDR_PROBE_AVAILABLE_KEY] == "true" ||
                            sessionProbeResult.supportedHdrKinds.isNotEmpty()
                        )
            if (displayHdrProbeAvailable && hdrKind !in sessionProbeResult.supportedHdrKinds) {
                missing += "displayHdrCapability"
                diagnostics["displayHdrSupported"] = "false"
            }
            if (sessionProbeResult?.diagnostics?.get(DISPLAY_FRAME_RATE_SUPPORTED_KEY) == "false") {
                missing += "displayFrameRate"
            }
            if (sessionProbeResult?.diagnostics?.get(CODEC_FORMAT_PROBE_AVAILABLE_KEY) == "true" &&
                sessionProbeResult.diagnostics[CODEC_FORMAT_SUPPORTED_KEY] == "false"
            ) {
                missing += sessionProbeResult.diagnostics[CODEC_FORMAT_MISSING_CAPABILITY_KEY]
                    ?: "codecFormatCapability"
            }
        }

        val nativeFrameSupported =
            effectiveRequiresNativeFrame &&
                !sourceIsRemote &&
                hardwareDecodeSupported &&
                request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isNotEmpty()
        val recommendedPlaybackPath =
            if (isHdrOrDolbyVision) {
                VesperRecommendedPlaybackPath.SystemPlayer
            } else if (nativeFrameSupported) {
                VesperRecommendedPlaybackPath.NativeFramePipeline
            } else {
                VesperRecommendedPlaybackPath.SystemPlayer
            }
        val status =
            when {
                request.codec.isNullOrBlank() -> VesperPlaybackCapabilityProbeStatus.Unknown
                codecFamily == VesperPlaybackCodecFamily.Unknown -> VesperPlaybackCapabilityProbeStatus.Unsupported
                missing.isEmpty() -> VesperPlaybackCapabilityProbeStatus.Supported
                systemPlaybackSupported -> VesperPlaybackCapabilityProbeStatus.FallbackRequired
                else -> VesperPlaybackCapabilityProbeStatus.Unsupported
            }

        val dolbyVisionMode =
            dolbyVisionCodecInfo?.dolbyVisionMode
                ?: if (hdrKind == VesperPlaybackCapabilityHdrKind.DolbyVision) {
                    VesperPlaybackCapabilityDolbyVisionMode.Unsupported
                } else {
                    VesperPlaybackCapabilityDolbyVisionMode.None
                }
        val confidence =
            if (sessionProbeResult != null) {
                VesperPlaybackCapabilityConfidence.SessionProbe
            } else if (sourceIsLocal) {
                VesperPlaybackCapabilityConfidence.SourceMetadata
            } else {
                VesperPlaybackCapabilityConfidence.CodecOnly
            }
        val hdrMetadata =
            buildHdrMetadata(
                hdrKind = hdrKind,
                dolbyVisionMode = dolbyVisionMode,
                diagnostics = diagnostics,
            )

        return VesperPlaybackCapabilityProbeResult(
            status = status,
            codecFamily = codecFamily,
            systemPlaybackSupported = systemPlaybackSupported,
            hardwareDecodeSupported = hardwareDecodeSupported,
            sdkManagedNativeFrameSupported = nativeFrameSupported,
            recommendedPlaybackPath = recommendedPlaybackPath,
            outputFormat =
                if (recommendedPlaybackPath == VesperRecommendedPlaybackPath.SystemPlayer && isHdrOrDolbyVision) {
                    VesperPlaybackCapabilityOutputFormat.SurfaceOpaque
                } else if (effectiveRequiresNativeFrame) {
                    VesperPlaybackCapabilityOutputFormat.SurfaceOpaque
                } else {
                    VesperPlaybackCapabilityOutputFormat.Unknown
                },
            hdrKind = hdrKind,
            dolbyVisionMode = dolbyVisionMode,
            confidence = confidence,
            missingCapabilities = missing.distinct(),
            diagnostics = diagnostics,
            hdrMetadata = hdrMetadata,
        )
    }

    fun buildHdrMetadata(
        hdrKind: VesperPlaybackCapabilityHdrKind,
        dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode,
        diagnostics: Map<String, String>,
    ): VesperPlaybackCapabilityHdrMetadata? {
        val refinedDiagnostics = diagnostics.withDolbyVisionProfile8Refinement()
        val metadata =
            VesperPlaybackCapabilityHdrMetadata(
                hdrKind = hdrKind.takeIf { it != VesperPlaybackCapabilityHdrKind.None && it != VesperPlaybackCapabilityHdrKind.Unknown },
                dolbyVisionMode = dolbyVisionMode.takeIf { it != VesperPlaybackCapabilityDolbyVisionMode.None },
                probe = refinedDiagnostics.firstString("runtimeFormatHdrMetadataProbe", "assetVideoHdrMetadataProbe", "assetProbe"),
                codec = refinedDiagnostics.firstString("assetVideoCodec", "runtimeFormatCodecs"),
                sampleMimeType = refinedDiagnostics.stringValue("runtimeFormatSampleMimeType"),
                colorPrimaries = refinedDiagnostics.stringValue("assetVideoColorPrimaries"),
                colorSpace = refinedDiagnostics.stringValue("runtimeFormatColorSpace"),
                colorRange = refinedDiagnostics.stringValue("runtimeFormatColorRange"),
                transferFunction = refinedDiagnostics.firstString("assetVideoTransferFunction", "runtimeFormatColorTransfer"),
                yCbCrMatrix = refinedDiagnostics.stringValue("assetVideoYCbCrMatrix"),
                alternativeTransferCharacteristics =
                    refinedDiagnostics.stringValue("assetVideoAlternativeTransferCharacteristics"),
                lumaBitDepth = refinedDiagnostics.intValue("runtimeFormatLumaBitDepth"),
                chromaBitDepth = refinedDiagnostics.intValue("runtimeFormatChromaBitDepth"),
                hdrStaticInfoPresent = refinedDiagnostics.boolValue("runtimeFormatHdrStaticInfoPresent"),
                hdrStaticInfoByteLength = refinedDiagnostics.intValue("runtimeFormatHdrStaticInfoByteLength"),
                hdrStaticInfoParseError = refinedDiagnostics.stringValue("runtimeFormatHdrStaticInfoParseError"),
                maxContentLightLevelNits =
                    refinedDiagnostics.firstInt("assetVideoMaxContentLightLevelNits", "runtimeFormatMaxContentLightLevelNits"),
                maxFrameAverageLightLevelNits =
                    refinedDiagnostics.firstInt(
                        "assetVideoMaxFrameAverageLightLevelNits",
                        "runtimeFormatMaxFrameAverageLightLevelNits",
                    ),
                masteringDisplayColorVolumePresent =
                    refinedDiagnostics.boolValue("assetVideoMasteringDisplayColorVolumePresent"),
                masteringDisplayColorVolumeByteLength =
                    refinedDiagnostics.intValue("assetVideoMasteringDisplayColorVolumeByteLength"),
                masteringDisplayColorVolumeParseError =
                    refinedDiagnostics.stringValue("assetVideoMasteringDisplayColorVolumeParseError"),
                masteringDisplayPrimary0 = refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary0"),
                masteringDisplayPrimary1 = refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary1"),
                masteringDisplayPrimary2 = refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary2"),
                masteringDisplayWhitePoint = refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayWhitePoint"),
                masteringDisplayMaxLuminanceNits =
                    refinedDiagnostics.doubleValue("assetVideoMasteringDisplayMaxLuminanceNits"),
                masteringDisplayMinLuminanceNits =
                    refinedDiagnostics.doubleValue("assetVideoMasteringDisplayMinLuminanceNits"),
                dolbyVisionCodec = refinedDiagnostics.stringValue("dolbyVisionCodec"),
                dolbyVisionProfile = refinedDiagnostics.intValue("dolbyVisionProfile"),
                dolbyVisionLevel = refinedDiagnostics.intValue("dolbyVisionLevel"),
                dolbyVisionCompatibility = refinedDiagnostics.stringValue("dolbyVisionCompatibility"),
                dolbyVisionProfileFamily = refinedDiagnostics.stringValue("dolbyVisionProfileFamily"),
                dolbyVisionBaseLayer = refinedDiagnostics.stringValue("dolbyVisionBaseLayer"),
                dolbyVisionFallbackTarget = refinedDiagnostics.stringValue("dolbyVisionFallbackTarget"),
                dolbyVisionBaseLayerEvidence = refinedDiagnostics.stringValue("dolbyVisionBaseLayerEvidence"),
                dolbyVisionBaseLayerTransferFunction = refinedDiagnostics.stringValue("dolbyVisionBaseLayerTransferFunction"),
            )
        return metadata.takeIf { !it.isEmpty }
    }
}

object VesperAndroidDisplaySessionProbeProvider {
    fun fromContext(context: Context): VesperAndroidSessionProbeProvider =
        VesperAndroidSessionProbeProvider { request ->
            val display = context.primaryDisplayOrNull()
            val codecProbe = context.probeCodecFormatSupport(request)
            if (display == null && codecProbe == null) {
                return@VesperAndroidSessionProbeProvider null
            }
            val diagnostics =
                linkedMapOf(
                    "sessionProbe" to "androidDisplayAndCodecCapabilities",
                )
            val supportedHdrKinds = display?.supportedHdrKinds() ?: emptySet()
            if (display != null) {
                diagnostics[DISPLAY_HDR_PROBE_AVAILABLE_KEY] = "true"
                diagnostics["displayName"] = display.name ?: "unknown"
                display.refreshRateDiagnostics(request)?.let(diagnostics::putAll)
            }
            codecProbe?.appendDiagnosticsTo(diagnostics)
            VesperAndroidSessionProbeResult(
                supportedHdrKinds = supportedHdrKinds,
                diagnostics = diagnostics,
            )
        }
}

internal fun androidCodecFormatForSessionProbe(
    codec: String?,
    codecFamily: VesperPlaybackCodecFamily = codec.toPlaybackCodecFamily(),
): Format? {
    if (codec.isNullOrBlank()) {
        return null
    }
    val sampleMimeType = MimeTypes.getVideoMediaMimeType(codec) ?: codecFamily.androidMimeType ?: return null
    if (!MimeTypes.isVideo(sampleMimeType)) {
        return null
    }
    val builder = Format.Builder()
        .setCodecs(codec)
        .setSampleMimeType(sampleMimeType)
    return builder.build()
}

internal fun androidCodecFormatForSessionProbe(
    request: VesperPlaybackCapabilityProbeRequest,
): Format? {
    val format = androidCodecFormatForSessionProbe(request.codec) ?: return null
    val builder = format.buildUpon()
    request.width?.takeIf { it > 0 }?.let(builder::setWidth)
    request.height?.takeIf { it > 0 }?.let(builder::setHeight)
    request.frameRate?.takeIf { it > 0f }?.let(builder::setFrameRate)
    return builder.build()
}

private data class AndroidCodecFormatProbeResult(
    val sampleMimeType: String,
    val codecs: String?,
    val width: Int?,
    val height: Int?,
    val frameRate: Float?,
    val supported: Boolean,
    val decoderCount: Int,
    val decoderName: String?,
    val unsupportedReason: String?,
) {
    fun appendDiagnosticsTo(diagnostics: MutableMap<String, String>) {
        diagnostics[CODEC_FORMAT_PROBE_AVAILABLE_KEY] = "true"
        diagnostics[CODEC_FORMAT_SAMPLE_MIME_TYPE_KEY] = sampleMimeType
        codecs?.let { diagnostics["codecFormatCodecs"] = it }
        width?.let { diagnostics["codecFormatWidth"] = it.toString() }
        height?.let { diagnostics["codecFormatHeight"] = it.toString() }
        frameRate?.let { diagnostics["codecFormatFrameRate"] = it.toString() }
        diagnostics[CODEC_FORMAT_SUPPORTED_KEY] = supported.toString()
        diagnostics["codecFormatDecoderCount"] = decoderCount.toString()
        decoderName?.let { diagnostics["codecFormatDecoder"] = it }
        unsupportedReason?.let { reason ->
            diagnostics["codecFormatUnsupportedReason"] = reason
            diagnostics[CODEC_FORMAT_MISSING_CAPABILITY_KEY] =
                when (reason) {
                    "noHardwareDecoder" -> "deviceHardwareDecode"
                    "formatRejected" -> "codecProfileLevel"
                    else -> "codecFormatCapability"
                }
        }
    }
}

private fun Context.probeCodecFormatSupport(
    request: VesperPlaybackCapabilityProbeRequest,
): AndroidCodecFormatProbeResult? {
    val format = androidCodecFormatForSessionProbe(request) ?: return null
    val sampleMimeType = format.sampleMimeType ?: return null
    val decoders =
        runCatching {
            VesperHardwareMediaCodecSelector.getDecoderInfos(
                sampleMimeType,
                requiresSecureDecoder = false,
                requiresTunnelingDecoder = false,
            )
        }.onFailure { error ->
            Log.w(SESSION_PROBE_TAG, "failed to query hardware decoder format support for $sampleMimeType", error)
        }.getOrElse {
            return AndroidCodecFormatProbeResult(
                sampleMimeType = sampleMimeType,
                codecs = format.codecs?.takeIf { it.isNotBlank() },
                width = format.codecFormatWidth,
                height = format.codecFormatHeight,
                frameRate = format.codecFormatFrameRate,
                supported = false,
                decoderCount = 0,
                decoderName = null,
                unsupportedReason = "decoderQueryFailed",
            )
        }
    if (decoders.isEmpty()) {
        return AndroidCodecFormatProbeResult(
            sampleMimeType = sampleMimeType,
            codecs = format.codecs?.takeIf { it.isNotBlank() },
            width = format.codecFormatWidth,
            height = format.codecFormatHeight,
            frameRate = format.codecFormatFrameRate,
            supported = false,
            decoderCount = 0,
            decoderName = null,
            unsupportedReason = "noHardwareDecoder",
        )
    }
    val supportedDecoder =
        decoders.firstOrNull { decoder ->
            runCatching {
                decoder.isFormatSupported(this, format)
            }.onFailure { error ->
                Log.w(SESSION_PROBE_TAG, "failed to probe decoder ${decoder.name} for $sampleMimeType", error)
            }.getOrDefault(false)
        }
    return AndroidCodecFormatProbeResult(
        sampleMimeType = sampleMimeType,
        codecs = format.codecs?.takeIf { it.isNotBlank() },
        width = format.codecFormatWidth,
        height = format.codecFormatHeight,
        frameRate = format.codecFormatFrameRate,
        supported = supportedDecoder != null,
        decoderCount = decoders.size,
        decoderName = supportedDecoder?.name,
        unsupportedReason = if (supportedDecoder == null) "formatRejected" else null,
    )
}

private val Format.codecFormatWidth: Int?
    get() = width.takeIf { it != Format.NO_VALUE && it > 0 }

private val Format.codecFormatHeight: Int?
    get() = height.takeIf { it != Format.NO_VALUE && it > 0 }

private val Format.codecFormatFrameRate: Float?
    get() = frameRate.takeIf { it.isFinite() && it > 0f }

private val VesperPlaybackCodecFamily.androidMimeType: String?
    get() =
        when (this) {
            VesperPlaybackCodecFamily.H264 -> "video/avc"
            VesperPlaybackCodecFamily.Hevc -> "video/hevc"
            VesperPlaybackCodecFamily.Av1 -> "video/av01"
            VesperPlaybackCodecFamily.Vvc -> "video/vvc"
            VesperPlaybackCodecFamily.Unknown -> null
        }

private fun String?.toPlaybackCodecFamily(): VesperPlaybackCodecFamily =
    when (vesperAndroidVideoCodecFamily(this)) {
        VesperAndroidVideoCodecFamily.Avc -> VesperPlaybackCodecFamily.H264
        VesperAndroidVideoCodecFamily.Hevc -> VesperPlaybackCodecFamily.Hevc
        VesperAndroidVideoCodecFamily.Av1 -> VesperPlaybackCodecFamily.Av1
        VesperAndroidVideoCodecFamily.Vvc -> VesperPlaybackCodecFamily.Vvc
        VesperAndroidVideoCodecFamily.Unknown -> VesperPlaybackCodecFamily.Unknown
    }

private fun String?.detectHdrKind(): VesperPlaybackCapabilityHdrKind {
    if (isNullOrBlank()) {
        return VesperPlaybackCapabilityHdrKind.None
    }
    val normalizedCodecs =
        split(',')
        .map { it.trim().lowercase() }
        .filter(String::isNotBlank)
        .map { rawCodec ->
            val normalized =
                if (rawCodec.startsWith("video/")) {
                    rawCodec.removePrefix("video/")
                } else {
                    rawCodec
                }
            normalized
        }
    if (normalizedCodecs.any { it.startsWith("dvh1") || it.startsWith("dvhe") || it == "dolbyvision" }) {
        return VesperPlaybackCapabilityHdrKind.DolbyVision
    }
    if (normalizedCodecs.any { it == "hdr10" || it == "hdr10+" || it == "hdr10plus" }) {
        return VesperPlaybackCapabilityHdrKind.Hdr10
    }
    if (normalizedCodecs.any { it == "hlg" }) {
        return VesperPlaybackCapabilityHdrKind.Hlg
    }
    return VesperPlaybackCapabilityHdrKind.None
}

private fun Map<String, String>.firstString(vararg keys: String): String? =
    keys.firstNotNullOfOrNull(::stringValue)

private fun Map<String, String>.firstInt(vararg keys: String): Int? =
    keys.firstNotNullOfOrNull(::intValue)

internal fun Map<String, String>.withDolbyVisionProfile8Refinement(): Map<String, String> =
    toMutableMap().also { it.applyDolbyVisionProfile8Refinement() }

internal fun MutableMap<String, String>.applyDolbyVisionProfile8Refinement() {
    if (intValue("dolbyVisionProfile") != 8) {
        return
    }
    val evidence = dolbyVisionProfile8BaseLayerEvidence() ?: return
    put("dolbyVisionCompatibility", evidence.compatibility)
    put("dolbyVisionProfileFamily", "profile8SingleLayerCompatible")
    put("dolbyVisionBaseLayer", evidence.baseLayer)
    put("dolbyVisionFallbackTarget", evidence.fallbackTarget)
    put("dolbyVisionBaseLayerEvidence", evidence.key)
    put("dolbyVisionBaseLayerTransferFunction", evidence.transferFunction)
}

private fun Map<String, String>.dolbyVisionProfile8BaseLayerEvidence(): DolbyVisionProfile8BaseLayerEvidence? =
    listOf(
        "assetVideoTransferFunction",
        "assetVideoAlternativeTransferCharacteristics",
        "runtimeFormatColorTransfer",
    )
        .firstNotNullOfOrNull { key ->
            stringValue(key)?.let { transferFunction ->
                DolbyVisionProfile8BaseLayerEvidence.fromTransferFunction(
                    key = key,
                    transferFunction = transferFunction,
                )
            }
        }

private data class DolbyVisionProfile8BaseLayerEvidence(
    val key: String,
    val transferFunction: String,
    val compatibility: String,
    val baseLayer: String,
    val fallbackTarget: String,
) {
    companion object {
        fun fromTransferFunction(
            key: String,
            transferFunction: String,
        ): DolbyVisionProfile8BaseLayerEvidence? {
            val normalized = transferFunction.lowercase()
            return when {
                normalized.contains("hlg") ||
                    normalized.contains("arib") ||
                    normalized.contains("std-b67") ||
                    normalized.contains("std_b67") ->
                    DolbyVisionProfile8BaseLayerEvidence(
                        key = key,
                        transferFunction = transferFunction,
                        compatibility = "profile8HlgBaseLayer",
                        baseLayer = "hlgBaseLayer",
                        fallbackTarget = "hlgBaseLayerSystemPlayer",
                    )
                normalized.contains("pq") ||
                    normalized.contains("2084") ||
                    normalized.contains("st2084") ||
                    normalized.contains("st_2084") ->
                    DolbyVisionProfile8BaseLayerEvidence(
                        key = key,
                        transferFunction = transferFunction,
                        compatibility = "profile8Hdr10BaseLayer",
                        baseLayer = "hdr10BaseLayer",
                        fallbackTarget = "hdr10BaseLayerSystemPlayer",
                    )
                normalized == "sdr" ||
                    normalized == "srgb" ||
                    normalized.contains("bt709") ||
                    normalized.contains("bt.709") ||
                    normalized.contains("itu_r_709") ||
                    normalized.contains("gamma") ->
                    DolbyVisionProfile8BaseLayerEvidence(
                        key = key,
                        transferFunction = transferFunction,
                        compatibility = "profile8SdrBaseLayer",
                        baseLayer = "sdrBaseLayer",
                        fallbackTarget = "sdrBaseLayerSystemPlayer",
                    )
                else -> null
            }
        }
    }
}

private fun Map<String, String>.stringValue(key: String): String? =
    this[key]?.takeIf(String::isNotEmpty)

private fun Map<String, String>.boolValue(key: String): Boolean? =
    when (this[key]) {
        "true" -> true
        "false" -> false
        else -> null
    }

private fun Map<String, String>.intValue(key: String): Int? =
    stringValue(key)?.toIntOrNull()

private fun Map<String, String>.doubleValue(key: String): Double? =
    stringValue(key)?.toDoubleOrNull()?.takeIf(Double::isFinite)

private fun Map<String, String>.chromaticityPoint(key: String): VesperHdrChromaticityPoint? {
    val value = stringValue(key) ?: return null
    val parts = value.split(',')
    if (parts.size != 2) {
        return null
    }
    val x = parts[0].trim().toDoubleOrNull() ?: return null
    val y = parts[1].trim().toDoubleOrNull() ?: return null
    return VesperHdrChromaticityPoint(x = x, y = y)
}

internal fun String?.detectDolbyVisionCodecInfo(): AndroidDolbyVisionCodecInfo? {
    if (isNullOrBlank()) {
        return null
    }
    val codec =
        split(',')
            .map { it.trim().lowercase().removePrefix("video/") }
            .firstOrNull { it.startsWith("dvh1") || it.startsWith("dvhe") || it == "dolbyvision" }
            ?: return null
    val match = Regex("^(?:dvh1|dvhe)\\.(\\d{1,2})(?:\\.(\\d{1,2}))?.*").matchEntire(codec)
    return AndroidDolbyVisionCodecInfo(
        codec = codec,
        profile = match?.groupValues?.getOrNull(1)?.takeIf(String::isNotBlank)?.toIntOrNull(),
        level = match?.groupValues?.getOrNull(2)?.takeIf(String::isNotBlank)?.toIntOrNull(),
    )
}

private val VesperPlayerSourceKind.wireName: String
    get() =
        when (this) {
            VesperPlayerSourceKind.Local -> "local"
            VesperPlayerSourceKind.Remote -> "remote"
        }

private val VesperPlayerSourceProtocol.wireName: String
    get() =
        when (this) {
            VesperPlayerSourceProtocol.Unknown -> "unknown"
            VesperPlayerSourceProtocol.File -> "file"
            VesperPlayerSourceProtocol.Content -> "content"
            VesperPlayerSourceProtocol.Progressive -> "progressive"
            VesperPlayerSourceProtocol.Hls -> "hls"
            VesperPlayerSourceProtocol.Dash -> "dash"
        }

private const val DISPLAY_HDR_PROBE_AVAILABLE_KEY = "displayHdrProbeAvailable"
private const val DISPLAY_FRAME_RATE_SUPPORTED_KEY = "displayFrameRateSupported"
private const val CODEC_FORMAT_PROBE_AVAILABLE_KEY = "codecFormatProbeAvailable"
private const val CODEC_FORMAT_SAMPLE_MIME_TYPE_KEY = "codecFormatSampleMimeType"
private const val CODEC_FORMAT_SUPPORTED_KEY = "codecFormatSupported"
private const val CODEC_FORMAT_MISSING_CAPABILITY_KEY = "codecFormatMissingCapability"
private const val SESSION_PROBE_TAG = "VesperSessionProbe"

private fun Context.primaryDisplayOrNull(): Display? =
    if (Build.VERSION.SDK_INT >= 30) {
        display
    } else {
        @Suppress("DEPRECATION")
        (getSystemService(Context.DISPLAY_SERVICE) as? DisplayManager)?.getDisplay(Display.DEFAULT_DISPLAY)
    }

private fun Display.supportedHdrKinds(): Set<VesperPlaybackCapabilityHdrKind> {
    if (Build.VERSION.SDK_INT < 24) {
        return emptySet()
    }
    val kinds = linkedSetOf<VesperPlaybackCapabilityHdrKind>()
    hdrCapabilities.supportedHdrTypesCompat().forEach { hdrType ->
        when (hdrType) {
                Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION ->
                    kinds += VesperPlaybackCapabilityHdrKind.DolbyVision
                Display.HdrCapabilities.HDR_TYPE_HDR10 ->
                    kinds += VesperPlaybackCapabilityHdrKind.Hdr10
                Display.HdrCapabilities.HDR_TYPE_HLG ->
                    kinds += VesperPlaybackCapabilityHdrKind.Hlg
        }
    }
    return kinds
}

@Suppress("DEPRECATION")
private fun Display.HdrCapabilities.supportedHdrTypesCompat(): IntArray = supportedHdrTypes

private fun Display.refreshRateDiagnostics(
    request: VesperPlaybackCapabilityProbeRequest,
): Map<String, String>? =
    refreshRateDiagnostics(
        requestedFrameRate = request.frameRate,
        displayRefreshRate = mode?.refreshRate?.takeIf { it > 0f } ?: refreshRate.takeIf { it > 0f },
    )

internal fun refreshRateDiagnostics(
    requestedFrameRate: Float?,
    displayRefreshRate: Float?,
): Map<String, String>? {
    val refreshRate = displayRefreshRate?.takeIf { it.isFinite() && it > 0f } ?: return null
    return buildMap {
        put("displayRefreshRate", refreshRate.toString())
        val frameRate = requestedFrameRate?.takeIf { it.isFinite() && it > 0f } ?: return@buildMap
        put("requestedFrameRate", frameRate.toString())
        put(DISPLAY_FRAME_RATE_SUPPORTED_KEY, (frameRate <= refreshRate + 0.01f).toString())
    }
}
