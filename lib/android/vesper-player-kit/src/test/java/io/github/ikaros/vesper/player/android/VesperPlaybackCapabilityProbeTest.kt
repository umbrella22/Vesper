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
                        requiresHdrNativeFrame = true,
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

        assertEquals(VesperPlaybackCapabilityProbeStatus.Unsupported, result.status)
        assertEquals(VesperPlaybackCodecFamily.Hevc, result.codecFamily)
        assertFalse(result.hdrNativeFrameSupported)
        assertEquals(VesperPlaybackCapabilityOutputFormat.Unknown, result.outputFormat)
        assertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        assertEquals("systemPlaybackOnly", result.diagnostics["hdrNativeFramePolicy"])
        assertEquals("true", result.diagnostics["nativeFrameRejectedForHdrProcessing"])
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
        assertFalse(result.hdrNativeFrameSupported)
        assertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        assertEquals("systemPlaybackOnly", result.diagnostics["hdrNativeFramePolicy"])
        assertEquals("true", result.diagnostics["systemPlaybackSelectedForHdr"])
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
