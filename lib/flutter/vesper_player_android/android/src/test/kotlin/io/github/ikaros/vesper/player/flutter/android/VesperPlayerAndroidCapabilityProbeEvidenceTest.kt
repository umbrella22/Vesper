package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityConfidence
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityDolbyVisionMode
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrKind
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityOutputFormat
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeRequest
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeResult
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeStatus
import io.github.ikaros.vesper.player.android.VesperPlaybackCodecFamily
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperRecommendedPlaybackPath
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlayerAndroidCapabilityProbeEvidenceTest {
    @Test
    fun runtimeCapabilityWarningIncludesMatchingAppProbeEvidence() {
        val request = probeRequest("https://example.com/movie-dv.mp4")
        val recentProbe = request.toSourceBoundProbe(probeResult())
        val enriched = runtimePayload("https://example.com/movie-dv.mp4")
            .withAppProbeConvergence(recentProbe)

        assertEquals("fallbackRequired", enriched["appProbeStatus"])
        assertEquals("systemPlayer", enriched["appProbeRecommendedPlaybackPath"])
        assertEquals("sessionProbe", enriched["appProbeConfidence"])
        assertEquals("dolbyVision", enriched["appProbeHdrKind"])
        assertEquals("compatibleBaseLayer", enriched["appProbeDolbyVisionMode"])
        assertEquals(
            "hdrProgrammableProcessingNotSupported,displayHdrCapability",
            enriched["appProbeMissingCapabilities"],
        )
        assertEquals("https://example.com/movie-dv.mp4", enriched["appProbeSourceUri"])
        assertEquals("progressive", enriched["appProbeSourceProtocol"])
        assertEquals(true, enriched["appProbeSourceMatchesRuntime"])
        assertEquals(true, enriched["appProbeRuntimeRecommendedPathMatches"])
        assertEquals(true, enriched["appProbeRuntimeHdrKindMatches"])
        assertEquals(true, enriched["appProbeRuntimeDolbyVisionModeMatches"])
        assertEquals(true, enriched["appProbeRuntimeSystemPlayerRecommendationConfirmed"])
        assertEquals(true, enriched["appProbeRuntimeHdrKindPresent"])
        assertEquals(true, enriched["appProbeRuntimeDolbyVisionModePresent"])
        assertEquals("false", enriched["appProbeDisplayHdrSupported"])
        assertEquals("false", enriched["appProbeCodecFormatSupported"])
        assertEquals("video/dolby-vision", enriched["appProbeCodecFormatSampleMimeType"])
        assertEquals("dvhe.08.07", enriched["appProbeCodecFormatCodecs"])
    }

    @Test
    fun runtimeCapabilityWarningDoesNotUseMismatchedAppProbeSource() {
        val request = probeRequest("https://example.com/other.mp4")
        val recentProbe = request.toSourceBoundProbe(probeResult())
        val enriched = runtimePayload("https://example.com/movie-dv.mp4")
            .withAppProbeConvergence(recentProbe)

        assertNull(enriched["appProbeStatus"])
        assertNull(enriched["appProbeRecommendedPlaybackPath"])
    }

    @Test
    fun runtimeCapabilityWarningWithoutSourceUsesSessionRecentProbeBasis() {
        val request = probeRequest("https://example.com/movie-dv.mp4")
        val recentProbe = request.toSourceBoundProbe(probeResult())
        val enriched = runtimePayloadWithoutSource().withAppProbeConvergence(recentProbe)

        assertEquals("fallbackRequired", enriched["appProbeStatus"])
        assertEquals("sessionRecentProbe", enriched["appProbeSourceMatchBasis"])
        assertNull(enriched["appProbeSourceMatchesRuntime"])
    }

    @Test
    fun sourceBoundProbeMatchesSourceRequest() {
        val request = probeRequest("https://example.com/movie-dv.mp4")
        val recentProbe = request.toSourceBoundProbe(probeResult())

        assertTrue(recentProbe.sourceMatches(request))
    }

    private fun probeRequest(uri: String): VesperPlaybackCapabilityProbeRequest =
        VesperPlaybackCapabilityProbeRequest(
            source =
                VesperPlayerSource(
                    uri = uri,
                    label = "Movie",
                    kind = VesperPlayerSourceKind.Remote,
                    protocol = VesperPlayerSourceProtocol.Progressive,
                ),
            codec = "dvhe.08.07",
            width = 3840,
            height = 2160,
            frameRate = 60f,
        )

    private fun probeResult(): VesperPlaybackCapabilityProbeResult =
        VesperPlaybackCapabilityProbeResult(
            status = VesperPlaybackCapabilityProbeStatus.FallbackRequired,
            codecFamily = VesperPlaybackCodecFamily.Hevc,
            systemPlaybackSupported = true,
            hardwareDecodeSupported = true,
            sdkManagedNativeFrameSupported = false,
            recommendedPlaybackPath = VesperRecommendedPlaybackPath.SystemPlayer,
            outputFormat = VesperPlaybackCapabilityOutputFormat.SurfaceOpaque,
            hdrKind = VesperPlaybackCapabilityHdrKind.DolbyVision,
            dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
            confidence = VesperPlaybackCapabilityConfidence.SessionProbe,
            missingCapabilities =
                listOf(
                    "hdrProgrammableProcessingNotSupported",
                    "displayHdrCapability",
                ),
            diagnostics =
                mapOf(
                    "displayHdrSupported" to "false",
                    "displayFrameRateSupported" to "true",
                    "codecFormatSupported" to "false",
                    "codecFormatMissingCapability" to "codecProfileLevel",
                    "codecFormatSampleMimeType" to "video/dolby-vision",
                    "codecFormatCodecs" to "dvhe.08.07",
                    "codecFormatWidth" to "3840",
                    "codecFormatHeight" to "2160",
                    "codecFormatFrameRate" to "60.0",
                ),
        )

    private fun runtimePayload(uri: String): Map<String, Any?> =
        mapOf(
            "reason" to "hdrNativeFrameUnsupported",
            "recommendedPlaybackPath" to "systemPlayer",
            "hdrKind" to "dolbyVision",
            "likelyHdrCapabilityIssue" to true,
            "confidence" to "sessionProbe",
            "sourceUri" to uri,
            "hdrMetadata" to
                mapOf(
                    "dolbyVisionMode" to "compatibleBaseLayer",
                ),
        )

    private fun runtimePayloadWithoutSource(): Map<String, Any?> =
        runtimePayload("https://example.com/movie-dv.mp4") - "sourceUri"
}
