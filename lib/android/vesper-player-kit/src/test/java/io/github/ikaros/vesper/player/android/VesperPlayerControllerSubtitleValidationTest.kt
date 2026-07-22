package io.github.ikaros.vesper.player.android

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain

/**
 * Subtitle selection validation tests for [VesperPlayerController].
 *
 * The controller must reject a `.track(id)` selection whose id is not in
 * the current subtitle catalog BEFORE forwarding to JNI, surfacing
 * `subtitle_track_not_found` as a structured `VesperPlayerUnsupportedOperation`.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VesperPlayerControllerSubtitleValidationTest {
    @Before
    fun installMainDispatcher() {
        Dispatchers.setMain(UnconfinedTestDispatcher())
    }

    @After
    fun resetMainDispatcher() {
        Dispatchers.resetMain()
    }

    @Test
    fun setSubtitleTrackSelection_unknownTrackId_throwsStructuredFailure() {
        val bridge = FakePlayerBridge()
        // Empty catalog: every id is unknown.
        val controller = VesperPlayerController(bridge)

        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            runBlocking {
                controller.setSubtitleTrackSelection(VesperTrackSelection.track("subtitle:dash:sub-zh"))
            }
        }
        assertTrue(
            "error message must mention the unknown id: ${error.message}",
            error.message?.contains("subtitle:dash:sub-zh") == true,
        )
        assertEquals(
            "structured subtitle code must be carried in details",
            "subtitle_track_not_found",
            error.details["code"],
        )
        assertEquals("selection", error.details["phase"])
        assertEquals("subtitle:dash:sub-zh", error.details["trackId"])
    }

    @Test
    fun setSubtitleTrackSelection_blankTrackId_throwsStructuredFailure() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            runBlocking { controller.setSubtitleTrackSelection(VesperTrackSelection.track("")) }
        }
        assertEquals(
            "subtitle_track_not_found",
            error.details["code"],
        )
    }

    @Test
    fun setSubtitleTrackSelection_nullTrackIdInTrackMode_throwsStructuredFailure() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        // A Track-mode selection with a null trackId must also surface as
        // a structured failure rather than silently forwarding to JNI.
        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            runBlocking {
                controller.setSubtitleTrackSelection(
                    VesperTrackSelection(VesperTrackSelectionMode.Track, null),
                )
            }
        }
        assertEquals("subtitle_track_not_found", error.details["code"])
    }

    @Test
    fun setSubtitleTrackSelection_autoSkipsCatalogValidation() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        // Auto mode does not require a known id; it must forward directly
        // to the bridge without raising.
        runBlocking { controller.setSubtitleTrackSelection(VesperTrackSelection.auto()) }
    }

    @Test
    fun setSubtitleTrackSelection_disabledSkipsCatalogValidation() {
        val bridge = FakePlayerBridge()
        val controller = VesperPlayerController(bridge)

        runBlocking { controller.setSubtitleTrackSelection(VesperTrackSelection.disabled()) }
    }
}
