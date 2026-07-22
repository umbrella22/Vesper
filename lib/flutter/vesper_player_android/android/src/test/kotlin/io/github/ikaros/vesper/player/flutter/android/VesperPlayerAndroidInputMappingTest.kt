package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import io.github.ikaros.vesper.player.android.VesperPlayerUnsupportedOperation
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class VesperPlayerAndroidInputMappingTest {
    @Test
    fun autoSurfaceKindUsesSurfaceView() {
        assertEquals(VesperVideoSurfaceKind.SurfaceView, null.toVesperVideoSurfaceKind())
        assertEquals(VesperVideoSurfaceKind.SurfaceView, "auto".toVesperVideoSurfaceKind())
    }

    @Test
    fun explicitSurfaceKindsArePreserved() {
        assertEquals(VesperVideoSurfaceKind.TextureView, "textureView".toVesperVideoSurfaceKind())
        assertEquals(VesperVideoSurfaceKind.SurfaceView, "surfaceView".toVesperVideoSurfaceKind())
    }

    @Test
    fun unknownSurfaceKindFails() {
        try {
            "unknown".toVesperVideoSurfaceKind()
            fail("Expected unknown renderSurfaceKind to throw.")
        } catch (_: IllegalArgumentException) {
        }
    }

    @Test
    fun canonicalExternalSubtitlesMapToAndroidSideLoads() {
        val source =
            mapOf<String, Any?>(
                "uri" to "https://example.com/video.m3u8",
                "label" to "Video",
                "kind" to "remote",
                "protocol" to "hls",
                "externalSubtitles" to
                    listOf(
                        mapOf(
                            "id" to "sub-en",
                            "uri" to "https://example.com/subtitles/en.vtt",
                            "mimeType" to "text/vtt",
                            "language" to "en",
                            "label" to "English",
                        ),
                    ),
            ).toVesperPlayerSource()

        val subtitle = source.externalSubtitles.single()
        assertEquals("https://example.com/subtitles/en.vtt", subtitle.uri)
        assertEquals("text/vtt", subtitle.mimeType)
        assertEquals("en", subtitle.language)
        assertEquals("English", subtitle.label)
    }

    @Test
    fun externalSubtitlesPreserveUnknownMimeType() {
        val source =
            mapOf<String, Any?>(
                "uri" to "https://example.com/video.m3u8",
                "kind" to "remote",
                "protocol" to "hls",
                "externalSubtitles" to
                    listOf(
                        mapOf(
                            "id" to "future-format",
                            "uri" to "https://example.com/subtitles/future.sub",
                            "mimeType" to "application/vnd.example.subtitle",
                        ),
                    ),
            ).toVesperPlayerSource()

        assertEquals(
            "application/vnd.example.subtitle",
            source.externalSubtitles.single().mimeType,
        )
    }

    @Test
    fun subtitleSelectionRejectsUnknownModeWithCanonicalDetails() {
        try {
            mapOf<String, Any?>("mode" to "future-mode")
                .toTrackSelection(isSubtitle = true)
            fail("Expected unknown subtitle selection mode to throw.")
        } catch (error: VesperPlayerUnsupportedOperation) {
            assertEquals("subtitle", error.details["domain"])
            assertEquals("subtitle_selection_invalid", error.details["code"])
            assertEquals("selection", error.details["phase"])
            assertEquals(false, error.details["retriable"])
        }
    }

    @Test
    fun subtitleTrackSelectionRejectsBlankTrackIdWithCanonicalDetails() {
        try {
            mapOf<String, Any?>("mode" to "track", "trackId" to "  ")
                .toTrackSelection(isSubtitle = true)
            fail("Expected blank subtitle track id to throw.")
        } catch (error: VesperPlayerUnsupportedOperation) {
            assertEquals("subtitle_selection_invalid", error.details["code"])
            assertEquals("  ", error.details["trackId"])
        }
    }

    @Test
    fun subtitleErrorDomainPreservesUnknownFutureCode() {
        val error =
            VesperPlayerUnsupportedOperation(
                "future subtitle failure",
                mapOf(
                    "domain" to "subtitle",
                    "code" to "future_caption_failure",
                    "phase" to "future_phase",
                    "retriable" to true,
                ),
            )

        val mapped = error.toErrorMap()

        assertEquals("subtitle", mapped["domain"])
        assertEquals("future_caption_failure", mapped["code"])
        assertEquals("future_phase", mapped["phase"])
        assertEquals(true, mapped["retriable"])
    }
}
