package io.github.ikaros.vesper.example.androidcomposehost

import io.github.ikaros.vesper.player.android.SeekableRangeUi
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import org.junit.Assert.assertEquals
import org.junit.Test

class ExampleTimelineRegressionTest {
    @Test
    fun `go live falls back to seekable end for live dvr`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 10_000L, endMs = 60_000L),
                liveEdgeMs = null,
                positionMs = 55_000L,
                durationMs = 60_000L,
            )

        assertEquals(ExampleLiveButtonState.LiveBehind(5_000L), liveButtonState(timeline))
        assertEquals(
            ExampleTimelineSummaryState.Window(positionMs = 55_000L, endMs = 60_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }

    @Test
    fun `live edge tolerance keeps live badge active`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.Live,
                isSeekable = false,
                seekableRange = null,
                liveEdgeMs = 120_000L,
                positionMs = 119_100L,
                durationMs = null,
            )

        assertEquals(ExampleLiveButtonState.Live, liveButtonState(timeline))
        assertEquals(
            ExampleTimelineSummaryState.LiveEdge(liveEdgeMs = 120_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }

    @Test
    fun `pending ratio is clamped to seekable range`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 30_000L, endMs = 90_000L),
                liveEdgeMs = 90_000L,
                positionMs = 48_000L,
                durationMs = 90_000L,
            )

        assertEquals(90_000L, displayedTimelinePositionMs(timeline, pendingSeekRatio = 1.4f))
        assertEquals(
            ExampleTimelineSummaryState.Window(positionMs = 90_000L, endMs = 90_000L),
            timelineSummaryState(timeline, pendingSeekRatio = 1.4f),
        )
    }

    @Test
    fun `window shrink clamps stale position before rendering`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 40_000L, endMs = 70_000L),
                liveEdgeMs = null,
                positionMs = 82_000L,
                durationMs = 120_000L,
            )

        assertEquals(70_000L, displayedTimelinePositionMs(timeline, pendingSeekRatio = null))
        assertEquals(ExampleLiveButtonState.Live, liveButtonState(timeline))
        assertEquals(
            ExampleTimelineSummaryState.Window(positionMs = 70_000L, endMs = 70_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }
}
