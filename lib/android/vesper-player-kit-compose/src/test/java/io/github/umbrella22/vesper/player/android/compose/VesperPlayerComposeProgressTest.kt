package io.github.umbrella22.vesper.player.android.compose

import io.github.umbrella22.vesper.player.android.PlaybackStateUi
import io.github.umbrella22.vesper.player.android.PlayerHostUiState
import io.github.umbrella22.vesper.player.android.TimelineKind
import io.github.umbrella22.vesper.player.android.TimelineUiState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test

class VesperPlayerComposeProgressTest {
    @Test
    fun timelineSampleOnlyPatchesItsAuthoritativeState() {
        val authoritative = playerState(positionMs = 1_000L)
        val sampledTimeline = authoritative.timeline.copy(positionMs = 2_000L)
        val sample = PresentedTimelineSample(authoritative, sampledTimeline)

        assertEquals(sampledTimeline, authoritative.withTimelineSample(sample).timeline)

        val changedState = authoritative.copy(sourceLabel = "replacement")
        assertSame(changedState, changedState.withTimelineSample(sample))
    }

    @Test
    fun progressRefreshBackoffIsBounded() {
        assertEquals(2_000L, nextProgressRefreshDelay(1_000L, 8_000L))
        assertEquals(4_000L, nextProgressRefreshDelay(2_000L, 8_000L))
        assertEquals(8_000L, nextProgressRefreshDelay(4_000L, 8_000L))
        assertEquals(8_000L, nextProgressRefreshDelay(8_000L, 8_000L))
    }

    private fun playerState(positionMs: Long): PlayerHostUiState =
        PlayerHostUiState(
            title = "title",
            subtitle = "subtitle",
            sourceLabel = "source",
            playbackState = PlaybackStateUi.Playing,
            playbackRate = 1f,
            isBuffering = false,
            isInterrupted = false,
            timeline =
                TimelineUiState(
                    kind = TimelineKind.Vod,
                    isSeekable = true,
                    seekableRange = null,
                    liveEdgeMs = null,
                    positionMs = positionMs,
                    durationMs = 10_000L,
                ),
        )
}
