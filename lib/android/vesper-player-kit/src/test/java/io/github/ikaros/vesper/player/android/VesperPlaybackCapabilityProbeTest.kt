package io.github.ikaros.vesper.player.android

import androidx.media3.common.MimeTypes
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlaybackCapabilityProbeTest {
    @Test
    fun hevcNativeFrameSurfaceProbeCanBeSupportedForLocalSdr() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/local.mp4", "local.mp4"),
                        codec = "hvc1.1.6.L93.B0",
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
            )

        assertEquals(VesperPlaybackCapabilityProbeStatus.Supported, result.status)
        assertEquals(VesperPlaybackCodecFamily.Hevc, result.codecFamily)
        assertTrue(result.hardwareDecodeSupported)
        assertTrue(result.sdkManagedNativeFrameSupported)
        assertEquals(VesperRecommendedPlaybackPath.NativeFramePipeline, result.recommendedPlaybackPath)
        assertEquals(VesperPlaybackCapabilityOutputFormat.SurfaceOpaque, result.outputFormat)
        assertTrue(result.missingCapabilities.isEmpty())
    }

    @Test
    fun hdrNativeFrameReportsProgrammableProcessingFallbackOnAndroid() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/hdr.mp4", "hdr.mp4"),
                        codec = "dvh1.05.06",
                        sourceNormalizerConfiguration =
                            VesperSourceNormalizerConfiguration(
                                mode = VesperSourceNormalizerMode.PreferNormalized,
                                pluginReferences = listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
                            ),
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
            )

        assertEquals(VesperPlaybackCapabilityProbeStatus.FallbackRequired, result.status)
        assertEquals(VesperPlaybackCodecFamily.Hevc, result.codecFamily)
        assertEquals(VesperRecommendedPlaybackPath.SystemPlayer, result.recommendedPlaybackPath)
        assertEquals(VesperPlaybackCapabilityHdrKind.DolbyVision, result.hdrKind)
        assertEquals(VesperPlaybackCapabilityOutputFormat.SurfaceOpaque, result.outputFormat)
        assertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        assertEquals("hdrSystemPlaybackOnly", result.diagnostics["playbackPathPolicy"])
        assertEquals("hdrNativeFrameUnsupported", result.diagnostics["recommendedPlaybackPathReason"])
    }

    @Test
    fun dolbyVisionPreferNativeFrameRoutesToSystemPlaybackOnAndroid() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv.mp4", "dv.mp4"),
                        codec = "dvhe.05.06",
                        sourceNormalizerConfiguration =
                            VesperSourceNormalizerConfiguration(
                                mode = VesperSourceNormalizerMode.PreferNormalized,
                                pluginReferences = listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
                            ),
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
            )

        assertEquals(VesperPlaybackCapabilityProbeStatus.FallbackRequired, result.status)
        assertEquals(VesperPlaybackCapabilityHdrKind.DolbyVision, result.hdrKind)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.Unsupported, result.dolbyVisionMode)
        assertEquals("5", result.diagnostics["dolbyVisionProfile"])
        assertEquals("6", result.diagnostics["dolbyVisionLevel"])
        assertEquals("noCompatibleBaseLayer", result.diagnostics["dolbyVisionCompatibility"])
        assertEquals(VesperRecommendedPlaybackPath.SystemPlayer, result.recommendedPlaybackPath)
        assertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        assertEquals("hdrSystemPlaybackOnly", result.diagnostics["playbackPathPolicy"])
        assertEquals("hdrNativeFrameUnsupported", result.diagnostics["recommendedPlaybackPathReason"])
    }

    @Test
    fun dolbyVisionProfile8ReportsCompatibleBaseLayerCandidate() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv-profile8.mp4", "dv-profile8.mp4"),
                        codec = "dvhe.08.07",
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
            )

        assertEquals(VesperPlaybackCapabilityHdrKind.DolbyVision, result.hdrKind)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer, result.dolbyVisionMode)
        assertEquals("8", result.diagnostics["dolbyVisionProfile"])
        assertEquals("7", result.diagnostics["dolbyVisionLevel"])
        assertEquals("compatibleBaseLayerCandidate", result.diagnostics["dolbyVisionCompatibility"])
        assertEquals("profile8SingleLayerCompatible", result.diagnostics["dolbyVisionProfileFamily"])
        assertEquals("compatibleBaseLayerUnknown", result.diagnostics["dolbyVisionBaseLayer"])
        assertEquals("compatibleBaseLayerSystemPlayer", result.diagnostics["dolbyVisionFallbackTarget"])
        assertEquals(VesperRecommendedPlaybackPath.SystemPlayer, result.recommendedPlaybackPath)
        assertEquals(VesperPlaybackCapabilityHdrKind.DolbyVision, result.hdrMetadata?.hdrKind)
        assertEquals(
            VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
            result.hdrMetadata?.dolbyVisionMode,
        )
        assertEquals("dvhe.08.07", result.hdrMetadata?.dolbyVisionCodec)
        assertEquals(8, result.hdrMetadata?.dolbyVisionProfile)
        assertEquals(7, result.hdrMetadata?.dolbyVisionLevel)
        assertEquals("compatibleBaseLayerCandidate", result.hdrMetadata?.dolbyVisionCompatibility)
        assertEquals("profile8SingleLayerCompatible", result.hdrMetadata?.dolbyVisionProfileFamily)
        assertEquals("compatibleBaseLayerUnknown", result.hdrMetadata?.dolbyVisionBaseLayer)
        assertEquals("compatibleBaseLayerSystemPlayer", result.hdrMetadata?.dolbyVisionFallbackTarget)
    }

    @Test
    fun dolbyVisionProfile8UsesPqMetadataToRefineBaseLayer() {
        val metadata =
            VesperPlaybackCapabilityProbe.buildHdrMetadata(
                hdrKind = VesperPlaybackCapabilityHdrKind.DolbyVision,
                dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
                diagnostics =
                    mapOf(
                        "assetVideoHdrMetadataProbe" to "formatDescription",
                        "assetVideoTransferFunction" to "SMPTE_ST_2084_PQ",
                        "dolbyVisionCodec" to "dvhe.08.07",
                        "dolbyVisionProfile" to "8",
                        "dolbyVisionLevel" to "7",
                        "dolbyVisionCompatibility" to "compatibleBaseLayerCandidate",
                        "dolbyVisionProfileFamily" to "profile8SingleLayerCompatible",
                        "dolbyVisionBaseLayer" to "compatibleBaseLayerUnknown",
                        "dolbyVisionFallbackTarget" to "compatibleBaseLayerSystemPlayer",
                    ),
            )

        assertEquals("SMPTE_ST_2084_PQ", metadata?.transferFunction)
        assertEquals("profile8Hdr10BaseLayer", metadata?.dolbyVisionCompatibility)
        assertEquals("hdr10BaseLayer", metadata?.dolbyVisionBaseLayer)
        assertEquals("hdr10BaseLayerSystemPlayer", metadata?.dolbyVisionFallbackTarget)
        assertEquals("assetVideoTransferFunction", metadata?.dolbyVisionBaseLayerEvidence)
        assertEquals("SMPTE_ST_2084_PQ", metadata?.dolbyVisionBaseLayerTransferFunction)
    }

    @Test
    fun dolbyVisionProfile8UsesHlgMetadataToRefineBaseLayer() {
        val metadata =
            VesperPlaybackCapabilityProbe.buildHdrMetadata(
                hdrKind = VesperPlaybackCapabilityHdrKind.DolbyVision,
                dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
                diagnostics =
                    mapOf(
                        "assetVideoAlternativeTransferCharacteristics" to "ARIB_STD_B67_HLG",
                        "dolbyVisionCodec" to "dvhe.08.07",
                        "dolbyVisionProfile" to "8",
                        "dolbyVisionLevel" to "7",
                        "dolbyVisionCompatibility" to "compatibleBaseLayerCandidate",
                        "dolbyVisionProfileFamily" to "profile8SingleLayerCompatible",
                        "dolbyVisionBaseLayer" to "compatibleBaseLayerUnknown",
                        "dolbyVisionFallbackTarget" to "compatibleBaseLayerSystemPlayer",
                    ),
            )

        assertEquals("ARIB_STD_B67_HLG", metadata?.alternativeTransferCharacteristics)
        assertEquals("profile8HlgBaseLayer", metadata?.dolbyVisionCompatibility)
        assertEquals("hlgBaseLayer", metadata?.dolbyVisionBaseLayer)
        assertEquals("hlgBaseLayerSystemPlayer", metadata?.dolbyVisionFallbackTarget)
    }

    @Test
    fun dolbyVisionCodecInfoParsesProfileMatrixConservatively() {
        val profile5 = "dvh1.05.06".detectDolbyVisionCodecInfo()
        assertEquals(5, profile5?.profile)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.Unsupported, profile5?.dolbyVisionMode)
        assertEquals("noCompatibleBaseLayer", profile5?.diagnostics?.get("dolbyVisionCompatibility"))
        assertEquals("profile5SingleLayer", profile5?.diagnostics?.get("dolbyVisionProfileFamily"))
        assertEquals("none", profile5?.diagnostics?.get("dolbyVisionBaseLayer"))

        val profile7 = "dvhe.07.06".detectDolbyVisionCodecInfo()
        assertEquals(7, profile7?.profile)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer, profile7?.dolbyVisionMode)
        assertEquals("dualLayerBaseLayerCandidate", profile7?.diagnostics?.get("dolbyVisionCompatibility"))
        assertEquals("profile7DualLayer", profile7?.diagnostics?.get("dolbyVisionProfileFamily"))
        assertEquals("hdr10BaseLayerCandidate", profile7?.diagnostics?.get("dolbyVisionBaseLayer"))

        val profile8 = "video/dvhe.08.07,mp4a.40.2".detectDolbyVisionCodecInfo()
        assertEquals(8, profile8?.profile)
        assertEquals(7, profile8?.level)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer, profile8?.dolbyVisionMode)
        assertEquals("profile8SingleLayerCompatible", profile8?.diagnostics?.get("dolbyVisionProfileFamily"))

        val profile9 = "dvh1.09.01".detectDolbyVisionCodecInfo()
        assertEquals(9, profile9?.profile)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.Unsupported, profile9?.dolbyVisionMode)
        assertEquals("unknownProfile", profile9?.diagnostics?.get("dolbyVisionCompatibility"))
        assertEquals("profile9ConservativeUnknown", profile9?.diagnostics?.get("dolbyVisionProfileFamily"))
    }

    @Test
    fun dolbyVisionDisplaySessionProbeCanRaiseConfidence() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv.mp4", "dv.mp4"),
                        codec = "dvhe.05.06",
                        width = 3840,
                        height = 2160,
                        frameRate = 59.94f,
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
                sessionProbeProvider =
                    VesperAndroidSessionProbeProvider {
                        VesperAndroidSessionProbeResult(
                            supportedHdrKinds = setOf(VesperPlaybackCapabilityHdrKind.DolbyVision),
                            diagnostics = mapOf("sessionProbe" to "fakeDisplay"),
                        )
                    },
            )

        assertEquals(VesperPlaybackCapabilityConfidence.SessionProbe, result.confidence)
        assertEquals("fakeDisplay", result.diagnostics["sessionProbe"])
        assertFalse(result.missingCapabilities.contains("displayHdrCapability"))
    }

    @Test
    fun dolbyVisionDisplaySessionProbeReportsMissingDisplayCapability() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv.mp4", "dv.mp4"),
                        codec = "dvhe.05.06",
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
                sessionProbeProvider =
                    VesperAndroidSessionProbeProvider {
                        VesperAndroidSessionProbeResult(
                            supportedHdrKinds = setOf(VesperPlaybackCapabilityHdrKind.Hdr10),
                            diagnostics = mapOf("sessionProbe" to "fakeDisplay"),
                        )
                    },
            )

        assertEquals(VesperPlaybackCapabilityConfidence.SessionProbe, result.confidence)
        assertTrue(result.missingCapabilities.contains("displayHdrCapability"))
        assertEquals("false", result.diagnostics["displayHdrSupported"])
    }

    @Test
    fun dolbyVisionSessionProbeReportsMissingCodecProfileLevelCapability() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv.mp4", "dv.mp4"),
                        codec = "dvhe.05.06",
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
                sessionProbeProvider =
                    VesperAndroidSessionProbeProvider {
                        VesperAndroidSessionProbeResult(
                            supportedHdrKinds = setOf(VesperPlaybackCapabilityHdrKind.DolbyVision),
                            diagnostics =
                                mapOf(
                                    "sessionProbe" to "fakeCodecFormat",
                                    "codecFormatProbeAvailable" to "true",
                                    "codecFormatSupported" to "false",
                                    "codecFormatSampleMimeType" to MimeTypes.VIDEO_DOLBY_VISION,
                                    "codecFormatCodecs" to "dvhe.05.06",
                                    "codecFormatWidth" to "3840",
                                    "codecFormatHeight" to "2160",
                                    "codecFormatFrameRate" to "59.94",
                                    "codecFormatMissingCapability" to "codecProfileLevel",
                                ),
                        )
                    },
            )

        assertEquals(VesperPlaybackCapabilityConfidence.SessionProbe, result.confidence)
        assertTrue(result.missingCapabilities.contains("codecProfileLevel"))
        assertFalse(result.missingCapabilities.contains("displayHdrCapability"))
        assertEquals("false", result.diagnostics["codecFormatSupported"])
        assertEquals(MimeTypes.VIDEO_DOLBY_VISION, result.diagnostics["codecFormatSampleMimeType"])
        assertEquals("dvhe.05.06", result.diagnostics["codecFormatCodecs"])
        assertEquals("3840", result.diagnostics["codecFormatWidth"])
        assertEquals("2160", result.diagnostics["codecFormatHeight"])
        assertEquals("59.94", result.diagnostics["codecFormatFrameRate"])
    }

    @Test
    fun dolbyVisionSessionProbeReportsMissingDisplayFrameRateCapability() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv-120fps.mp4", "dv-120fps.mp4"),
                        codec = "dvhe.08.07",
                        frameRate = 120f,
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
                sessionProbeProvider =
                    VesperAndroidSessionProbeProvider {
                        VesperAndroidSessionProbeResult(
                            supportedHdrKinds = setOf(VesperPlaybackCapabilityHdrKind.DolbyVision),
                            diagnostics =
                                mapOf(
                                    "sessionProbe" to "fakeDisplay",
                                    "displayRefreshRate" to "60.0",
                                    "requestedFrameRate" to "120.0",
                                    "displayFrameRateSupported" to "false",
                                ),
                        )
                    },
            )

        assertEquals(VesperPlaybackCapabilityConfidence.SessionProbe, result.confidence)
        assertTrue(result.missingCapabilities.contains("displayFrameRate"))
        assertFalse(result.missingCapabilities.contains("displayHdrCapability"))
        assertEquals("false", result.diagnostics["displayFrameRateSupported"])
        assertEquals("60.0", result.diagnostics["displayRefreshRate"])
        assertEquals("120.0", result.diagnostics["requestedFrameRate"])
    }

    @Test
    fun dolbyVisionSessionProbeReportsMissingHardwareDecoderWhenFormatProbeFindsNoDecoder() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv.mp4", "dv.mp4"),
                        codec = "dvhe.05.06",
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
                sessionProbeProvider =
                    VesperAndroidSessionProbeProvider {
                        VesperAndroidSessionProbeResult(
                            supportedHdrKinds = setOf(VesperPlaybackCapabilityHdrKind.DolbyVision),
                            diagnostics =
                                mapOf(
                                    "sessionProbe" to "fakeCodecFormat",
                                    "codecFormatProbeAvailable" to "true",
                                    "codecFormatSupported" to "false",
                                    "codecFormatSampleMimeType" to MimeTypes.VIDEO_DOLBY_VISION,
                                    "codecFormatMissingCapability" to "deviceHardwareDecode",
                                ),
                        )
                    },
            )

        assertEquals(VesperPlaybackCapabilityConfidence.SessionProbe, result.confidence)
        assertTrue(result.hardwareDecodeSupported)
        assertTrue(result.missingCapabilities.contains("deviceHardwareDecode"))
        assertEquals("false", result.diagnostics["codecFormatSupported"])
    }

    @Test
    fun hdrMetadataModelParsesDiagnosticsIntoTypedFields() {
        val metadata =
            VesperPlaybackCapabilityProbe.buildHdrMetadata(
                hdrKind = VesperPlaybackCapabilityHdrKind.Hdr10,
                dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.None,
                diagnostics =
                    mapOf(
                        "runtimeFormatHdrMetadataProbe" to "media3FormatColorInfo",
                        "runtimeFormatSampleMimeType" to "video/hevc",
                        "runtimeFormatColorSpace" to "bt2020",
                        "runtimeFormatColorRange" to "limited",
                        "runtimeFormatColorTransfer" to "st2084",
                        "runtimeFormatLumaBitDepth" to "10",
                        "runtimeFormatChromaBitDepth" to "10",
                        "runtimeFormatHdrStaticInfoPresent" to "true",
                        "runtimeFormatHdrStaticInfoByteLength" to "25",
                        "runtimeFormatMaxContentLightLevelNits" to "1000",
                        "runtimeFormatMaxFrameAverageLightLevelNits" to "400",
                        "assetVideoMasteringDisplayPrimary0" to "0.38970,0.17204",
                        "assetVideoMasteringDisplayMaxLuminanceNits" to "1000.0",
                        "assetVideoMasteringDisplayMinLuminanceNits" to "0.0001",
                    ),
            )

        assertEquals(VesperPlaybackCapabilityHdrKind.Hdr10, metadata?.hdrKind)
        assertEquals("media3FormatColorInfo", metadata?.probe)
        assertEquals("video/hevc", metadata?.sampleMimeType)
        assertEquals("bt2020", metadata?.colorSpace)
        assertEquals("limited", metadata?.colorRange)
        assertEquals("st2084", metadata?.transferFunction)
        assertEquals(10, metadata?.lumaBitDepth)
        assertEquals(10, metadata?.chromaBitDepth)
        assertEquals(true, metadata?.hdrStaticInfoPresent)
        assertEquals(25, metadata?.hdrStaticInfoByteLength)
        assertEquals(1000, metadata?.maxContentLightLevelNits)
        assertEquals(400, metadata?.maxFrameAverageLightLevelNits)
        assertEquals(0.38970, metadata?.masteringDisplayPrimary0?.x ?: 0.0, 0.00001)
        assertEquals(0.17204, metadata?.masteringDisplayPrimary0?.y ?: 0.0, 0.00001)
        assertEquals(1000.0, metadata?.masteringDisplayMaxLuminanceNits ?: 0.0, 0.00001)
        assertEquals(0.0001, metadata?.masteringDisplayMinLuminanceNits ?: 0.0, 0.00001)
    }

    @Test
    fun dolbyVisionSessionProbeFormatUsesDolbyVisionMime() {
        val format = androidCodecFormatForSessionProbe("dvhe.05.06")

        assertEquals(MimeTypes.VIDEO_DOLBY_VISION, format?.sampleMimeType)
        assertEquals("dvhe.05.06", format?.codecs)
    }

    @Test
    fun hevcSessionProbeFormatUsesHevcMime() {
        val format = androidCodecFormatForSessionProbe("hvc1.1.6.L93.B0")

        assertEquals(MimeTypes.VIDEO_H265, format?.sampleMimeType)
        assertEquals("hvc1.1.6.L93.B0", format?.codecs)
    }

    @Test
    fun sessionProbeFormatUsesOptionalVideoDimensionsAndFrameRate() {
        val format =
            androidCodecFormatForSessionProbe(
                VesperPlaybackCapabilityProbeRequest(
                    codec = "dvhe.05.06",
                    width = 3840,
                    height = 2160,
                    frameRate = 59.94f,
                )
            )

        assertEquals(MimeTypes.VIDEO_DOLBY_VISION, format?.sampleMimeType)
        assertEquals(3840, format?.width)
        assertEquals(2160, format?.height)
        assertEquals(59.94f, format?.frameRate ?: 0f, 0.001f)
    }

    @Test
    fun refreshRateDiagnosticsReportsRequestedFrameRateAgainstDisplayRefreshRate() {
        val diagnostics =
            refreshRateDiagnostics(
                requestedFrameRate = 120f,
                displayRefreshRate = 60f,
            )

        assertEquals("60.0", diagnostics?.get("displayRefreshRate"))
        assertEquals("120.0", diagnostics?.get("requestedFrameRate"))
        assertEquals("false", diagnostics?.get("displayFrameRateSupported"))
    }

    @Test
    fun refreshRateDiagnosticsTreatsEqualFrameRateAsSupported() {
        val diagnostics =
            refreshRateDiagnostics(
                requestedFrameRate = 59.94f,
                displayRefreshRate = 60f,
            )

        assertEquals("60.0", diagnostics?.get("displayRefreshRate"))
        assertEquals("59.94", diagnostics?.get("requestedFrameRate"))
        assertEquals("true", diagnostics?.get("displayFrameRateSupported"))
    }

    @Test
    fun remoteNativeFrameProbeReportsHostManagedNetworkGap() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.hls("https://example.com/live.m3u8", "live"),
                        codec = "avc1.4d401f",
                        requiresNativeFrame = true,
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/avc"),
            )

        assertEquals(VesperPlaybackCapabilityProbeStatus.FallbackRequired, result.status)
        assertTrue(result.missingCapabilities.contains("hostManagedNetworkProbeNotImplemented"))
    }

    @Test
    fun unsupportedCodecIsRejectedBeforeNativeFrameRequirements() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request = VesperPlaybackCapabilityProbeRequest(codec = "vp9"),
                codecProbeProvider = hardwareCodecs("video/x-vnd.on2.vp9"),
            )

        assertEquals(VesperPlaybackCapabilityProbeStatus.Unsupported, result.status)
        assertEquals(VesperPlaybackCodecFamily.Unknown, result.codecFamily)
        assertFalse(result.systemPlaybackSupported)
    }

    private fun hardwareCodecs(vararg mimeTypes: String): VesperAndroidCodecProbeProvider =
        VesperAndroidCodecProbeProvider { mimeType -> mimeType in mimeTypes }
}
