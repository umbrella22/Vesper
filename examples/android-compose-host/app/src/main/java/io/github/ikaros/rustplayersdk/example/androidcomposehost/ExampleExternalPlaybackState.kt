package io.github.ikaros.vesper.example.androidcomposehost

import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRoute
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRouteKind

internal enum class ExampleExternalPlaybackStatus {
    Idle,
    Discovering,
    Connecting,
    Connected,
    Loading,
    Playing,
    Paused,
    Error,
}

internal data class ExampleExternalPlaybackSession(
    val routeId: String,
    val routeName: String,
    val routeKind: VesperExternalPlaybackRouteKind,
    val status: ExampleExternalPlaybackStatus,
    val source: VesperPlayerSource?,
    val basePositionMs: Long,
    val durationMs: Long?,
    val seekableRange: Pair<Long, Long>?,
    val startedAtMillis: Long?,
    val relayEnabled: Boolean = false,
    val message: String? = null,
)

internal fun ExampleExternalPlaybackSession?.isActiveRemotePlayback(): Boolean =
    this != null && status != ExampleExternalPlaybackStatus.Idle

internal fun exampleExternalRouteLabel(route: VesperExternalPlaybackRoute): String =
    buildString {
        append(route.name)
        when (route.kind) {
            VesperExternalPlaybackRouteKind.Cast -> append(" · Cast")
            VesperExternalPlaybackRouteKind.Dlna -> append(" · DLNA")
        }
        val model = listOfNotNull(route.manufacturer, route.modelName)
            .joinToString(" ")
            .trim()
        if (model.isNotBlank()) {
            append(" · ")
            append(model)
        }
    }

internal fun exampleEstimatedExternalPositionMs(
    session: ExampleExternalPlaybackSession,
    nowMillis: Long,
): Long {
    val elapsedMs =
        if (session.status == ExampleExternalPlaybackStatus.Playing) {
            (nowMillis - (session.startedAtMillis ?: nowMillis)).coerceAtLeast(0L)
        } else {
            0L
        }
    return exampleClampExternalPosition(
        positionMs = session.basePositionMs + elapsedMs,
        durationMs = session.durationMs,
        seekableRange = session.seekableRange,
    )
}

internal fun exampleExternalTimeline(
    localTimeline: TimelineUiState,
    session: ExampleExternalPlaybackSession?,
    nowMillis: Long,
): TimelineUiState {
    if (session == null) {
        return localTimeline
    }
    val estimatedPosition = exampleEstimatedExternalPositionMs(session, nowMillis)
    return localTimeline.copy(
        positionMs = estimatedPosition,
        durationMs = session.durationMs ?: localTimeline.durationMs,
    )
}

internal fun exampleExternalPlaybackState(session: ExampleExternalPlaybackSession?): PlaybackStateUi =
    when (session?.status) {
        ExampleExternalPlaybackStatus.Playing -> PlaybackStateUi.Playing
        ExampleExternalPlaybackStatus.Paused,
        ExampleExternalPlaybackStatus.Connected,
        ExampleExternalPlaybackStatus.Loading,
        -> PlaybackStateUi.Paused
        ExampleExternalPlaybackStatus.Idle,
        ExampleExternalPlaybackStatus.Discovering,
        ExampleExternalPlaybackStatus.Connecting,
        ExampleExternalPlaybackStatus.Error,
        null,
        -> PlaybackStateUi.Ready
    }

internal fun exampleDisconnectLocalPositionMs(
    session: ExampleExternalPlaybackSession?,
    nowMillis: Long,
): Long? = session?.let { exampleEstimatedExternalPositionMs(it, nowMillis) }

internal fun exampleExternalPositionForRatio(
    timeline: TimelineUiState,
    ratio: Float,
): Long = timeline.positionForRatio(ratio)

internal fun examplePausedExternalSession(
    session: ExampleExternalPlaybackSession,
    nowMillis: Long,
): ExampleExternalPlaybackSession =
    session.copy(
        status = ExampleExternalPlaybackStatus.Paused,
        basePositionMs = exampleEstimatedExternalPositionMs(session, nowMillis),
        startedAtMillis = null,
    )

internal fun examplePlayingExternalSession(
    session: ExampleExternalPlaybackSession,
    nowMillis: Long,
): ExampleExternalPlaybackSession =
    session.copy(
        status = ExampleExternalPlaybackStatus.Playing,
        basePositionMs = exampleEstimatedExternalPositionMs(session, nowMillis),
        startedAtMillis = nowMillis,
    )

internal fun exampleSeekedExternalSession(
    session: ExampleExternalPlaybackSession,
    positionMs: Long,
    nowMillis: Long,
): ExampleExternalPlaybackSession =
    session.copy(
        basePositionMs = exampleClampExternalPosition(
            positionMs = positionMs,
            durationMs = session.durationMs,
            seekableRange = session.seekableRange,
        ),
        startedAtMillis =
            if (session.status == ExampleExternalPlaybackStatus.Playing) {
                nowMillis
            } else {
                null
            },
    )

internal fun exampleSeekableRangePair(timeline: TimelineUiState): Pair<Long, Long>? =
    timeline.seekableRange?.let { range -> range.startMs to range.endMs }

private fun exampleClampExternalPosition(
    positionMs: Long,
    durationMs: Long?,
    seekableRange: Pair<Long, Long>?,
): Long {
    val range = seekableRange
    if (range != null && range.second >= range.first) {
        return positionMs.coerceIn(range.first, range.second)
    }
    val duration = durationMs
    if (duration != null && duration > 0L) {
        return positionMs.coerceIn(0L, duration)
    }
    return positionMs.coerceAtLeast(0L)
}

internal fun exampleExternalStatusLabel(status: ExampleExternalPlaybackStatus): String =
    when (status) {
        ExampleExternalPlaybackStatus.Idle -> "Idle"
        ExampleExternalPlaybackStatus.Discovering -> "Scanning"
        ExampleExternalPlaybackStatus.Connecting -> "Connecting"
        ExampleExternalPlaybackStatus.Connected -> "Connected"
        ExampleExternalPlaybackStatus.Loading -> "Loading"
        ExampleExternalPlaybackStatus.Playing -> "Playing"
        ExampleExternalPlaybackStatus.Paused -> "Paused"
        ExampleExternalPlaybackStatus.Error -> "Error"
    }

internal fun TimelineUiState.externalStartPositionMs(): Long =
    clampedPosition(positionMs.takeIf { kind != TimelineKind.Live } ?: (goLivePositionMs ?: positionMs))
