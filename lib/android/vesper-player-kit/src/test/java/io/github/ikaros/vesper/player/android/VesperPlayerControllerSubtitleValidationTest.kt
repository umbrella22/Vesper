package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Subtitle selection validation tests for [VesperPlayerController].
 *
 * The controller must reject a `.track(id)` selection whose id is not in
 * the current subtitle catalog BEFORE forwarding to JNI, surfacing
 * `subtitle_track_not_found` as a structured `VesperPlayerUnsupportedOperation`.
 */
class VesperPlayerControllerSubtitleValidationTest {
    @Test
    fun setSubtitleTrackSelection_unknownTrackId_throwsStructuredFailure() {
        val bridge = FakePlayerBridge()
        // Empty catalog: every id is unknown.
        val controller = VesperPlayerController(bridge)

        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            controller.setSubtitleTrackSelection(VesperTrackSelection.track("subtitle:dash:sub-zh"))
        }
        assertTrue(
            "error message must mention the unknown id: ${error.message}",
            error.message?.contains("subtitle:dash:sub-zh") == true,
        )
        assertEquals(
            "structured subtitle code must be carried in details",
            "subtitle_track_not_found",
            error.details["subtitleCode"],
        )
        assertEquals("selection", error.details["subtitlePhase"])
        assertEquals("subtitle:dash:sub-zh", error.details["trackId"])
    }

    @Test
    fun setSubtitleTrackSelection_blankTrackId_throwsStructuredFailure() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            controller.setSubtitleTrackSelection(VesperTrackSelection.track(""))
        }
        assertEquals(
            "subtitle_track_not_found",
            error.details["subtitleCode"],
        )
    }

    @Test
    fun setSubtitleTrackSelection_nullTrackIdInTrackMode_throwsStructuredFailure() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        // A Track-mode selection with a null trackId must also surface as
        // a structured failure rather than silently forwarding to JNI.
        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            controller.setSubtitleTrackSelection(VesperTrackSelection(VesperTrackSelectionMode.Track, null))
        }
        assertEquals("subtitle_track_not_found", error.details["subtitleCode"])
    }

    @Test
    fun setSubtitleTrackSelection_autoSkipsCatalogValidation() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        // Auto mode does not require a known id; it must forward directly
        // to the bridge without raising.
        controller.setSubtitleTrackSelection(VesperTrackSelection.auto())
    }

    @Test
    fun setSubtitleTrackSelection_disabledSkipsCatalogValidation() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        controller.setSubtitleTrackSelection(VesperTrackSelection.disabled())
    }
}
