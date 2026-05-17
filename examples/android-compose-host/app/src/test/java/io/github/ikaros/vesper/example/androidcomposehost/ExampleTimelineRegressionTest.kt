package io.github.ikaros.vesper.example.androidcomposehost

import io.github.ikaros.vesper.player.android.SeekableRangeUi
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRoute
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRouteKind
import org.junit.Assert.assertEquals
import org.junit.Test

class ExampleTimelineRegressionTest {
    @Test
    fun `live dvr acceptance source is hls and queueable`() {
        val source = androidLiveDvrAcceptanceSource(context = null)

        assertEquals(ANDROID_LIVE_DVR_ACCEPTANCE_URL, source.uri)
        assertEquals(VesperPlayerSourceProtocol.Hls, source.protocol)
    }

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
            ExampleTimelineSummaryState.Window(positionMs = 45_000L, endMs = 50_000L),
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
            ExampleTimelineSummaryState.Window(positionMs = 60_000L, endMs = 60_000L),
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
            ExampleTimelineSummaryState.Window(positionMs = 30_000L, endMs = 30_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }

    @Test
    fun `external route label includes kind and device metadata`() {
        val route =
            VesperExternalPlaybackRoute(
                routeId = "dlna:living-room",
                name = "Living Room",
                kind = VesperExternalPlaybackRouteKind.Dlna,
                manufacturer = "Acme",
                modelName = "Panel",
            )

        assertEquals("Living Room · DLNA · Acme Panel", exampleExternalRouteLabel(route))
    }

    @Test
    fun `external optimistic position advances only while playing and clamps to duration`() {
        val source = VesperPlayerSource.hls(ANDROID_HLS_DEMO_URL, "HLS")
        val session =
            ExampleExternalPlaybackSession(
                routeId = "cast:active",
                routeName = "Cast device",
                routeKind = VesperExternalPlaybackRouteKind.Cast,
                status = ExampleExternalPlaybackStatus.Playing,
                source = source,
                basePositionMs = 9_000L,
                durationMs = 10_000L,
                seekableRange = null,
                startedAtMillis = 1_000L,
            )

        assertEquals(10_000L, exampleEstimatedExternalPositionMs(session, nowMillis = 4_500L))
        assertEquals(
            9_000L,
            exampleEstimatedExternalPositionMs(
                session.copy(status = ExampleExternalPlaybackStatus.Paused),
                nowMillis = 4_500L,
            ),
        )
    }

    @Test
    fun `external disconnect returns latest estimated remote position`() {
        val session =
            ExampleExternalPlaybackSession(
                routeId = "dlna:tv",
                routeName = "TV",
                routeKind = VesperExternalPlaybackRouteKind.Dlna,
                status = ExampleExternalPlaybackStatus.Playing,
                source = androidHlsDemoSource(context = null),
                basePositionMs = 12_000L,
                durationMs = 60_000L,
                seekableRange = null,
                startedAtMillis = 2_000L,
            )

        assertEquals(17_000L, exampleDisconnectLocalPositionMs(session, nowMillis = 7_000L))
    }

    @Test
    fun `external timeline uses seekable range for clamped live dvr progress`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 40_000L, endMs = 70_000L),
                liveEdgeMs = 70_000L,
                positionMs = 45_000L,
                durationMs = 120_000L,
            )
        val session =
            ExampleExternalPlaybackSession(
                routeId = "dlna:tv",
                routeName = "TV",
                routeKind = VesperExternalPlaybackRouteKind.Dlna,
                status = ExampleExternalPlaybackStatus.Playing,
                source = androidLiveDvrAcceptanceSource(context = null),
                basePositionMs = 68_000L,
                durationMs = 120_000L,
                seekableRange = 40_000L to 70_000L,
                startedAtMillis = 1_000L,
            )

        assertEquals(70_000L, exampleExternalTimeline(timeline, session, nowMillis = 6_000L).positionMs)
        assertEquals(55_000L, exampleExternalPositionForRatio(timeline, ratio = 0.5f))
    }
}
