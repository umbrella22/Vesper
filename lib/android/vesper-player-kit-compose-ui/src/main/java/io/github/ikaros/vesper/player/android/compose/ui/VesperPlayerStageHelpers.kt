package io.github.ikaros.vesper.player.android.compose.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot
import java.util.Locale

@Composable
internal fun speedBadge(rate: Float): String =
    stringResource(R.string.vesper_player_stage_rate, formatRate(rate))

@Composable
internal fun qualityButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String {
    val selectedTrack = trackCatalog.videoTracks.firstOrNull { it.id == trackSelection.abrPolicy.trackId }
    return when (trackSelection.abrPolicy.mode) {
        VesperAbrMode.FixedTrack ->
            selectedTrack?.let { stringResource(R.string.vesper_player_stage_quality_locked, qualityLabel(it)) }
                ?: stringResource(R.string.vesper_player_stage_quality)
        VesperAbrMode.Constrained,
        VesperAbrMode.Auto,
        -> stringResource(R.string.vesper_player_stage_auto)
    }
}

@Composable
internal fun stageBadgeText(timeline: TimelineUiState): String =
    when (timeline.kind) {
        TimelineKind.Live -> stringResource(R.string.vesper_player_stage_live_stream)
        TimelineKind.LiveDvr -> stringResource(R.string.vesper_player_stage_live_dvr)
        TimelineKind.Vod -> stringResource(R.string.vesper_player_stage_vod)
    }

@Composable
internal fun liveButtonLabel(timeline: TimelineUiState): String {
    val liveEdge = timeline.goLivePositionMs ?: return stringResource(R.string.vesper_player_stage_go_live)
    val behindMs = (liveEdge - timeline.clampedPosition(timeline.positionMs)).coerceAtLeast(0L)
    if (behindMs <= 1_500L) {
        return stringResource(R.string.vesper_player_stage_live)
    }
    return stringResource(R.string.vesper_player_stage_live_behind, formatMillis(behindMs))
}

@Composable
internal fun timelineSummary(
    timeline: TimelineUiState,
    pendingSeekRatio: Float?,
): String {
    val displayedPosition =
        pendingSeekRatio?.let(timeline::positionForRatio)
            ?: timeline.clampedPosition(timeline.positionMs)
    return when (timeline.kind) {
        TimelineKind.Live -> {
            val liveEdge = timeline.goLivePositionMs
            if (liveEdge == null) {
                stringResource(R.string.vesper_player_stage_live)
            } else {
                stringResource(R.string.vesper_player_stage_live_edge, formatMillis(liveEdge))
            }
        }
        TimelineKind.LiveDvr -> liveDvrSummary(timeline, displayedPosition, spaced = true)
        TimelineKind.Vod -> "${formatMillis(displayedPosition)} / ${formatMillis(timeline.durationMs ?: 0L)}"
    }
}

@Composable
internal fun compactTimelineSummary(
    timeline: TimelineUiState,
    pendingSeekRatio: Float?,
): String {
    val displayedPosition =
        pendingSeekRatio?.let(timeline::positionForRatio)
            ?: timeline.clampedPosition(timeline.positionMs)
    return when (timeline.kind) {
        TimelineKind.Live -> stringResource(R.string.vesper_player_stage_live)
        TimelineKind.LiveDvr -> liveDvrSummary(timeline, displayedPosition, spaced = false)
        TimelineKind.Vod -> "${formatMillis(displayedPosition)}/${formatMillis(timeline.durationMs ?: 0L)}"
    }
}

private fun liveDvrSummary(
    timeline: TimelineUiState,
    displayedPosition: Long,
    spaced: Boolean,
): String {
    val rangeStart = timeline.seekableRange?.startMs ?: 0L
    val windowEnd = timeline.goLivePositionMs ?: timeline.durationMs ?: 0L
    val position = (displayedPosition - rangeStart).coerceIn(0L, (windowEnd - rangeStart).coerceAtLeast(0L))
    val end = (windowEnd - rangeStart).coerceAtLeast(0L)
    return if (spaced) {
        "${formatMillis(position)} / ${formatMillis(end)}"
    } else {
        "${formatMillis(position)}/${formatMillis(end)}"
    }
}

private fun qualityLabel(track: VesperMediaTrack): String {
    val height = track.height
    val width = track.width
    val bitRate = track.bitRate
    return when {
        height != null -> "${height}p"
        width != null -> "${width}w"
        bitRate != null -> formatBitRate(bitRate)
        else -> track.label ?: track.id
    }
}

private fun formatBitRate(value: Long): String =
    when {
        value >= 1_000_000L -> String.format(Locale.getDefault(), "%.1f Mbps", value / 1_000_000.0)
        value >= 1_000L -> String.format(Locale.getDefault(), "%.0f Kbps", value / 1_000.0)
        else -> "$value bps"
    }

private fun formatMillis(value: Long): String {
    val safeValue = value.coerceAtLeast(0L)
    val totalSeconds = safeValue / 1000L
    val minutes = totalSeconds / 60L
    val seconds = totalSeconds % 60L
    return String.format(Locale.getDefault(), "%02d:%02d", minutes, seconds)
}

private fun formatRate(value: Float): String = String.format(Locale.getDefault(), "%.1f", value)
