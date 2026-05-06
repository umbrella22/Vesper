package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Test

class VesperSystemPlaybackControlsTest {
    @Test
    fun videoDefaultUsesTenSecondSeekButtons() {
        val controls = VesperSystemPlaybackControls.videoDefault().normalized()

        assertEquals(
            listOf(
                VesperSystemPlaybackControlKind.SeekBack,
                VesperSystemPlaybackControlKind.PlayPause,
                VesperSystemPlaybackControlKind.SeekForward,
            ),
            controls.compactButtons.map { it.kind },
        )
        assertEquals(10_000L, controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekBack))
        assertEquals(10_000L, controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekForward))
    }

    @Test
    fun compactButtonsClampOffsetsAndForceCenterPlayPause() {
        val controls =
            VesperSystemPlaybackControls(
                compactButtons =
                    listOf(
                        VesperSystemPlaybackControlButton.seekBack(500L),
                        VesperSystemPlaybackControlButton.seekForward(15_000L),
                        VesperSystemPlaybackControlButton.seekForward(90_000L),
                    ),
            ).normalized()

        assertEquals(VesperSystemPlaybackControlKind.PlayPause, controls.compactButtons[1].kind)
        assertEquals(1_000L, controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekBack))
        assertEquals(60_000L, controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekForward))
    }

    @Test
    fun disabledSeekLeavesOnlyPlayPauseControl() {
        val controls = VesperSystemPlaybackControls.videoDefault().normalized(showSeekActions = false)

        assertEquals(listOf(VesperSystemPlaybackControlKind.PlayPause), controls.compactButtons.map { it.kind })
        assertEquals(null, controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekBack))
        assertEquals(null, controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekForward))
    }
}
