package io.github.ikaros.vesper.player.android

import android.content.Context
import android.hardware.display.DisplayManager
import android.os.Build
import android.view.Display

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
    val requiresNativeFrame: Boolean = false,
    val sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
        VesperSourceNormalizerConfiguration(),
    val frameProcessorConfiguration: VesperFrameProcessorConfiguration =
        VesperFrameProcessorConfiguration(),
    val nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(),
)

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
)

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
            if (sessionProbeResult != null && hdrKind !in sessionProbeResult.supportedHdrKinds) {
                missing += "displayHdrCapability"
                diagnostics["displayHdrSupported"] = "false"
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
            dolbyVisionMode =
                if (hdrKind == VesperPlaybackCapabilityHdrKind.DolbyVision) {
                    VesperPlaybackCapabilityDolbyVisionMode.Unsupported
                } else {
                    VesperPlaybackCapabilityDolbyVisionMode.None
                },
            confidence =
                if (sessionProbeResult != null) {
                    VesperPlaybackCapabilityConfidence.SessionProbe
                } else if (sourceIsLocal) {
                    VesperPlaybackCapabilityConfidence.SourceMetadata
                } else {
                    VesperPlaybackCapabilityConfidence.CodecOnly
                },
            missingCapabilities = missing.distinct(),
            diagnostics = diagnostics,
        )
    }
}

object VesperAndroidDisplaySessionProbeProvider {
    fun fromContext(context: Context): VesperAndroidSessionProbeProvider =
        VesperAndroidSessionProbeProvider {
            val display = context.primaryDisplayOrNull() ?: return@VesperAndroidSessionProbeProvider null
            VesperAndroidSessionProbeResult(
                supportedHdrKinds = display.supportedHdrKinds(),
                diagnostics =
                    mapOf(
                        "sessionProbe" to "androidDisplayHdrCapabilities",
                        "displayName" to (display.name ?: "unknown"),
                    ),
            )
        }
}

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
    hdrCapabilities.supportedHdrTypes.forEach { hdrType ->
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
