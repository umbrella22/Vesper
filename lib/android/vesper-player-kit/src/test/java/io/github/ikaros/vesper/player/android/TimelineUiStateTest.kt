package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TimelineUiStateTest {
    @Test
    fun liveDvrGoLiveFallsBackToSeekableWindowEnd() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 30_000, endMs = 120_000),
                liveEdgeMs = null,
                positionMs = 90_000,
                durationMs = null,
            )

        assertEquals(120_000L, timeline.goLivePositionMs)
        assertEquals(30_000L, timeline.liveOffsetMs)
        assertEquals(2f / 3f, timeline.displayedRatio ?: 0f, 0.0001f)
    }

    @Test
    fun liveDvrOffsetTracksLiveEdgeTolerance() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 30_000, endMs = 120_000),
                liveEdgeMs = 120_000,
                positionMs = 118_800,
                durationMs = null,
            )

        assertEquals(1_200L, timeline.liveOffsetMs)
        assertTrue(timeline.isAtLiveEdge())
        assertFalse(timeline.isAtLiveEdge(toleranceMs = 500L))
    }

    @Test
    fun liveDvrSliderDragClampsToWindowBounds() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 30_000, endMs = 120_000),
                liveEdgeMs = 120_000,
                positionMs = 90_000,
                durationMs = null,
            )

        assertEquals(30_000L, timeline.positionForRatio(-0.25f))
        assertEquals(75_000L, timeline.positionForRatio(0.5f))
        assertEquals(120_000L, timeline.positionForRatio(1.5f))
    }

    @Test
    fun liveDvrWindowShrinkClampsStalePositionToNewWindow() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 60_000, endMs = 100_000),
                liveEdgeMs = 100_000,
                positionMs = 120_000,
                durationMs = null,
            )

        assertEquals(100_000L, timeline.clampedPosition(timeline.positionMs))
        assertEquals(0L, timeline.liveOffsetMs)
        assertEquals(1f, timeline.displayedRatio ?: 0f, 0.0001f)
        assertTrue(timeline.isAtLiveEdge())
    }
}
