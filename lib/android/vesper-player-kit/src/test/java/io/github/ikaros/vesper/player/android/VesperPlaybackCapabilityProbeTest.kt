package io.github.ikaros.vesper.player.android

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
                                decoderPluginLibraryPaths = listOf("/tmp/libmediacodec.so"),
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
                                pluginLibraryPaths = listOf("/tmp/libnormalizer.so"),
                            ),
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                                decoderPluginLibraryPaths = listOf("/tmp/libmediacodec.so"),
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
                                pluginLibraryPaths = listOf("/tmp/libnormalizer.so"),
                            ),
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginLibraryPaths = listOf("/tmp/libmediacodec.so"),
                            ),
                    ),
                codecProbeProvider = hardwareCodecs("video/hevc"),
            )

        assertEquals(VesperPlaybackCapabilityProbeStatus.FallbackRequired, result.status)
        assertEquals(VesperPlaybackCapabilityHdrKind.DolbyVision, result.hdrKind)
        assertEquals(VesperPlaybackCapabilityDolbyVisionMode.Unsupported, result.dolbyVisionMode)
        assertEquals(VesperRecommendedPlaybackPath.SystemPlayer, result.recommendedPlaybackPath)
        assertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        assertEquals("hdrSystemPlaybackOnly", result.diagnostics["playbackPathPolicy"])
        assertEquals("hdrNativeFrameUnsupported", result.diagnostics["recommendedPlaybackPathReason"])
    }

    @Test
    fun dolbyVisionDisplaySessionProbeCanRaiseConfidence() {
        val result =
            VesperPlaybackCapabilityProbe.probe(
                request =
                    VesperPlaybackCapabilityProbeRequest(
                        source = VesperPlayerSource.local("file:///tmp/dv.mp4", "dv.mp4"),
                        codec = "dvhe.05.06",
                        nativeFramePipelineConfiguration =
                            VesperNativeFramePipelineConfiguration(
                                mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                                decoderPluginLibraryPaths = listOf("/tmp/libmediacodec.so"),
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
                                decoderPluginLibraryPaths = listOf("/tmp/libmediacodec.so"),
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
                                decoderPluginLibraryPaths = listOf("/tmp/libmediacodec.so"),
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
