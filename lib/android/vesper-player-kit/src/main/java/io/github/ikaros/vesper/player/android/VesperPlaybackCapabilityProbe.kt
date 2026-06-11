package io.github.ikaros.vesper.player.android

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
