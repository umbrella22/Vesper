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

data class VesperPlaybackCapabilityProbeRequest(
    val source: VesperPlayerSource? = null,
    val codec: String? = null,
    val requiresNativeFrame: Boolean = false,
    val requiresHdrNativeFrame: Boolean = false,
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
    val hdrNativeFrameSupported: Boolean,
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
        val isDolbyVision = request.codec.looksDolbyVision()
        val rejectsHdrNativeFrame = request.requiresHdrNativeFrame || (isDolbyVision && effectiveRequiresNativeFrame)
        val missing = mutableListOf<String>()
        val diagnostics =
            linkedMapOf(
                "probeVersion" to "1",
                "sourceKind" to (request.source?.kind?.wireName ?: "unknown"),
                "sourceProtocol" to (request.source?.protocol?.wireName ?: "unknown"),
                "androidNativeFrameHdrFullChain" to "unavailable",
            )

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
        if (rejectsHdrNativeFrame) {
            missing += "hdrProgrammableProcessingNotSupported"
            diagnostics["hdrNativeFramePolicy"] = "systemPlaybackOnly"
            if (request.nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
                diagnostics["nativeFrameRejectedForHdrProcessing"] = "true"
            } else {
                diagnostics["systemPlaybackSelectedForHdr"] = "true"
            }
            if (request.sourceNormalizerConfiguration.mode == VesperSourceNormalizerMode.Disabled) {
                missing += "SourceNormalizerPacketHdrMetadata"
            }
        }

        val nativeFrameSupported =
            effectiveRequiresNativeFrame &&
                !sourceIsRemote &&
                hardwareDecodeSupported &&
                request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isNotEmpty() &&
                !rejectsHdrNativeFrame
        val status =
            when {
                request.codec.isNullOrBlank() -> VesperPlaybackCapabilityProbeStatus.Unknown
                codecFamily == VesperPlaybackCodecFamily.Unknown -> VesperPlaybackCapabilityProbeStatus.Unsupported
                rejectsHdrNativeFrame &&
                    request.nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame ->
                    VesperPlaybackCapabilityProbeStatus.Unsupported
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
            hdrNativeFrameSupported = false,
            outputFormat =
                if (rejectsHdrNativeFrame) {
                    VesperPlaybackCapabilityOutputFormat.Unknown
                } else if (effectiveRequiresNativeFrame) {
                    VesperPlaybackCapabilityOutputFormat.SurfaceOpaque
                } else {
                    VesperPlaybackCapabilityOutputFormat.Unknown
                },
            hdrKind =
                if (isDolbyVision) {
                    VesperPlaybackCapabilityHdrKind.DolbyVision
                } else if (rejectsHdrNativeFrame) {
                    VesperPlaybackCapabilityHdrKind.Unknown
                } else {
                    VesperPlaybackCapabilityHdrKind.None
                },
            dolbyVisionMode =
                if (isDolbyVision && effectiveRequiresNativeFrame) {
                    VesperPlaybackCapabilityDolbyVisionMode.Unsupported
                } else {
                    VesperPlaybackCapabilityDolbyVisionMode.None
                },
            confidence =
                if (sourceIsLocal) {
                    VesperPlaybackCapabilityConfidence.SourceMetadata
                } else {
                    VesperPlaybackCapabilityConfidence.CodecOnly
                },
            missingCapabilities = missing.distinct(),
            diagnostics = diagnostics,
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

private fun String?.looksDolbyVision(): Boolean {
    if (isNullOrBlank()) {
        return false
    }
    return split(',')
        .map { it.trim().lowercase() }
        .filter(String::isNotBlank)
        .any { rawCodec ->
            val normalized =
                if (rawCodec.startsWith("video/")) {
                    rawCodec.removePrefix("video/")
                } else {
                    rawCodec
                }
            normalized.startsWith("dvh1") || normalized.startsWith("dvhe")
        }
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
