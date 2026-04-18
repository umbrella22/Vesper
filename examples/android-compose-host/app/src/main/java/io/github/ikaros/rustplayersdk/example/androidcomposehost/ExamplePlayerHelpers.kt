package io.github.ikaros.vesper.example.androidcomposehost

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import android.provider.OpenableColumns
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperBufferingPolicy
import io.github.ikaros.vesper.player.android.VesperBufferingPreset
import io.github.ikaros.vesper.player.android.VesperCachePolicy
import io.github.ikaros.vesper.player.android.VesperCachePreset
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperPlaylistViewportHintKind
import io.github.ikaros.vesper.player.android.VesperRetryBackoff
import io.github.ikaros.vesper.player.android.VesperRetryPolicy
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelectionMode
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot
import java.util.Locale

internal sealed interface ExampleLiveButtonState {
    data object GoLive : ExampleLiveButtonState

    data object Live : ExampleLiveButtonState

    data class LiveBehind(
        val behindMs: Long,
    ) : ExampleLiveButtonState
}

internal sealed interface ExampleTimelineSummaryState {
    data object Live : ExampleTimelineSummaryState

    data class LiveEdge(
        val liveEdgeMs: Long,
    ) : ExampleTimelineSummaryState

    data class Window(
        val positionMs: Long,
        val endMs: Long,
    ) : ExampleTimelineSummaryState
}

internal fun displayedTimelinePositionMs(
    timeline: TimelineUiState,
    pendingSeekRatio: Float?,
): Long = pendingSeekRatio?.let(timeline::positionForRatio) ?: timeline.clampedPosition(timeline.positionMs)

internal fun liveButtonState(timeline: TimelineUiState): ExampleLiveButtonState {
    val liveEdge = timeline.goLivePositionMs ?: return ExampleLiveButtonState.GoLive
    val behindMs = (liveEdge - timeline.clampedPosition(timeline.positionMs)).coerceAtLeast(0L)
    return if (behindMs > 1_500L) {
        ExampleLiveButtonState.LiveBehind(behindMs)
    } else {
        ExampleLiveButtonState.Live
    }
}

internal fun timelineSummaryState(
    timeline: TimelineUiState,
    pendingSeekRatio: Float?,
): ExampleTimelineSummaryState {
    val displayedPosition = displayedTimelinePositionMs(timeline, pendingSeekRatio)
    return when (timeline.kind) {
        TimelineKind.Live ->
            timeline.goLivePositionMs?.let(ExampleTimelineSummaryState::LiveEdge)
                ?: ExampleTimelineSummaryState.Live

        TimelineKind.LiveDvr ->
            ExampleTimelineSummaryState.Window(
                positionMs = displayedPosition,
                endMs = timeline.goLivePositionMs ?: timeline.durationMs ?: 0L,
            )

        TimelineKind.Vod ->
            ExampleTimelineSummaryState.Window(
                positionMs = displayedPosition,
                endMs = timeline.durationMs ?: 0L,
            )
    }
}

@Composable
internal fun speedBadge(rate: Float): String =
    stringResource(R.string.example_unit_playback_rate, formatRate(rate))

@Composable
internal fun qualityButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String {
    val selectedTrack =
        trackCatalog.videoTracks.firstOrNull { it.id == trackSelection.abrPolicy.trackId }

    return when (trackSelection.abrPolicy.mode) {
        VesperAbrMode.FixedTrack ->
            if (selectedTrack != null) {
                qualityLabel(selectedTrack)
            } else {
                stringResource(R.string.example_common_quality)
            }
        VesperAbrMode.Constrained,
        VesperAbrMode.Auto,
        -> stringResource(R.string.example_common_auto)
    }
}

@Composable
internal fun audioButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String {
    val selectedTrack =
        trackCatalog.audioTracks.firstOrNull { it.id == trackSelection.audio.trackId }

    return when (trackSelection.audio.mode) {
        VesperTrackSelectionMode.Track ->
            if (selectedTrack != null) {
                audioLabel(selectedTrack)
            } else {
                stringResource(R.string.example_common_audio)
            }
        else -> stringResource(R.string.example_common_audio)
    }
}

@Composable
internal fun subtitleButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String {
    val selectedTrack =
        trackCatalog.subtitleTracks.firstOrNull { it.id == trackSelection.subtitle.trackId }

    return when (trackSelection.subtitle.mode) {
        VesperTrackSelectionMode.Disabled -> stringResource(R.string.example_common_cc_off)
        VesperTrackSelectionMode.Track ->
            if (selectedTrack != null) {
                subtitleLabel(selectedTrack)
            } else {
                stringResource(R.string.example_common_subtitles)
            }

        VesperTrackSelectionMode.Auto -> stringResource(R.string.example_common_cc_auto)
    }
}

@Composable
internal fun qualityLabel(track: VesperMediaTrack): String =
    buildString {
        when {
            track.height != null -> append("${track.height}p")
            track.width != null && track.height != null -> append("${track.width}×${track.height}")
            track.label != null -> append(track.label)
            else -> append(stringResource(R.string.example_common_video_track))
        }
    }

@Composable
internal fun qualitySubtitle(track: VesperMediaTrack): String {
    val bitRate = track.bitRate
    val bitRateText = if (bitRate != null) formatBitRate(bitRate) else null
    return listOfNotNull(track.codec, bitRateText)
        .joinToString(" • ")
        .ifBlank { stringResource(R.string.example_common_fixed_video_variant) }
}

@Composable
internal fun audioLabel(track: VesperMediaTrack): String =
    track.label ?: track.language?.uppercase() ?: stringResource(R.string.example_common_audio_track)

@Composable
internal fun audioSubtitle(track: VesperMediaTrack): String {
    val channelCount = track.channels
    val sampleRateHz = track.sampleRate
    val channels =
        if (channelCount != null) {
            stringResource(R.string.example_unit_audio_channels, channelCount)
        } else {
            null
        }
    val sampleRate =
        if (sampleRateHz != null) {
            stringResource(R.string.example_unit_audio_sample_rate_khz, sampleRateHz / 1000)
        } else {
            null
        }
    return listOfNotNull(
        track.language?.uppercase(),
        channels,
        sampleRate,
        track.codec,
    ).joinToString(" • ").ifBlank { stringResource(R.string.example_common_audio_program) }
}

@Composable
internal fun subtitleLabel(track: VesperMediaTrack): String =
    track.label ?: track.language?.uppercase() ?: stringResource(R.string.example_common_subtitle_track)

@Composable
internal fun subtitleSubtitle(track: VesperMediaTrack): String =
    listOfNotNull(
        track.language?.uppercase(),
        if (track.isForced) stringResource(R.string.example_common_forced) else null,
        if (track.isDefault) stringResource(R.string.example_common_default) else null,
    ).joinToString(" • ").ifBlank { stringResource(R.string.example_common_subtitle_option) }

@Composable
internal fun stageBadgeText(timeline: TimelineUiState): String =
    when (timeline.kind) {
        TimelineKind.Live -> stringResource(R.string.example_stage_live_stream)
        TimelineKind.LiveDvr -> stringResource(R.string.example_stage_live_with_dvr_window)
        TimelineKind.Vod -> stringResource(R.string.example_stage_video_on_demand)
    }

@Composable
internal fun playlistHintLabel(kind: VesperPlaylistViewportHintKind): String =
    when (kind) {
        VesperPlaylistViewportHintKind.Visible -> stringResource(R.string.example_playlist_status_visible)
        VesperPlaylistViewportHintKind.NearVisible -> stringResource(R.string.example_playlist_status_near_visible)
        VesperPlaylistViewportHintKind.PrefetchOnly -> stringResource(R.string.example_playlist_status_prefetch)
        VesperPlaylistViewportHintKind.Hidden -> stringResource(R.string.example_playlist_status_hidden)
    }

@Composable
internal fun liveButtonLabel(timeline: TimelineUiState): String {
    return when (val state = liveButtonState(timeline)) {
        ExampleLiveButtonState.GoLive -> stringResource(R.string.example_stage_go_live)
        ExampleLiveButtonState.Live -> stringResource(R.string.example_stage_live)
        is ExampleLiveButtonState.LiveBehind ->
            stringResource(R.string.example_stage_live_behind, formatMillis(state.behindMs))
    }
}

@Composable
internal fun timelineSummary(
    timeline: TimelineUiState,
    pendingSeekRatio: Float?,
): String =
    when (val state = timelineSummaryState(timeline, pendingSeekRatio)) {
        ExampleTimelineSummaryState.Live -> stringResource(R.string.example_stage_live)
        is ExampleTimelineSummaryState.LiveEdge ->
            stringResource(R.string.example_stage_live_edge, formatMillis(state.liveEdgeMs))
        is ExampleTimelineSummaryState.Window ->
            "${formatMillis(state.positionMs)} / ${formatMillis(state.endMs)}"
    }

@Composable
internal fun formatBitRate(value: Long): String =
    when {
        value >= 1_000_000L -> stringResource(R.string.example_unit_bitrate_mbps, value / 1_000_000.0)
        value >= 1_000L -> stringResource(R.string.example_unit_bitrate_kbps, value / 1_000.0)
        else -> stringResource(R.string.example_unit_bitrate_bps, value)
    }

internal fun formatMillis(value: Long): String {
    val totalSeconds = value / 1000L
    val minutes = totalSeconds / 60L
    val seconds = totalSeconds % 60L
    return String.format(Locale.getDefault(), "%02d:%02d", minutes, seconds)
}

internal fun formatBytes(value: Long?): String {
    if (value == null || value <= 0L) {
        return "-"
    }
    if (value >= 1024L * 1024L * 1024L) {
        return String.format(Locale.getDefault(), "%.1f GB", value / (1024f * 1024f * 1024f))
    }
    if (value >= 1024L * 1024L) {
        return String.format(Locale.getDefault(), "%.1f MB", value / (1024f * 1024f))
    }
    if (value >= 1024L) {
        return String.format(Locale.getDefault(), "%.0f KB", value / 1024f)
    }
    return "$value B"
}

internal fun formatRate(value: Float): String = String.format(Locale.getDefault(), "%.1f", value)

@Composable
internal fun resilienceBufferingValue(policy: VesperBufferingPolicy): String =
    "${bufferingPresetLabel(policy.preset)} · ${bufferWindowLabel(policy)}"

@Composable
internal fun resilienceRetryValue(policy: VesperRetryPolicy): String {
    val attempts =
        policy.maxAttempts?.let {
            stringResource(R.string.example_resilience_retry_attempts, it)
        } ?: stringResource(R.string.example_resilience_retry_unlimited)
    return stringResource(
        R.string.example_resilience_retry_value,
        attempts,
        retryBackoffLabel(policy.backoff),
    )
}

@Composable
internal fun resilienceCacheValue(policy: VesperCachePolicy): String =
    stringResource(
        R.string.example_resilience_cache_value,
        cachePresetLabel(policy.preset),
        formatStorageBytes(policy.maxMemoryBytes),
        formatStorageBytes(policy.maxDiskBytes),
    )

@Composable
internal fun bufferingPresetLabel(preset: VesperBufferingPreset): String =
    when (preset) {
        VesperBufferingPreset.Default -> stringResource(R.string.example_resilience_preset_default)
        VesperBufferingPreset.Balanced -> stringResource(R.string.example_resilience_preset_balanced)
        VesperBufferingPreset.Streaming -> stringResource(R.string.example_resilience_preset_streaming)
        VesperBufferingPreset.Resilient -> stringResource(R.string.example_resilience_preset_resilient)
        VesperBufferingPreset.LowLatency -> stringResource(R.string.example_resilience_preset_low_latency)
    }

@Composable
internal fun cachePresetLabel(preset: VesperCachePreset): String =
    when (preset) {
        VesperCachePreset.Default -> stringResource(R.string.example_resilience_preset_default)
        VesperCachePreset.Disabled -> stringResource(R.string.example_resilience_preset_disabled)
        VesperCachePreset.Streaming -> stringResource(R.string.example_resilience_preset_streaming)
        VesperCachePreset.Resilient -> stringResource(R.string.example_resilience_preset_resilient)
    }

@Composable
internal fun retryBackoffLabel(backoff: VesperRetryBackoff): String =
    when (backoff) {
        VesperRetryBackoff.Fixed -> stringResource(R.string.example_resilience_backoff_fixed)
        VesperRetryBackoff.Linear -> stringResource(R.string.example_resilience_backoff_linear)
        VesperRetryBackoff.Exponential -> stringResource(R.string.example_resilience_backoff_exponential)
    }

@Composable
internal fun bufferWindowLabel(policy: VesperBufferingPolicy): String {
    val min = policy.minBufferMs
    val max = policy.maxBufferMs
    if (min == null || max == null) {
        return stringResource(R.string.example_resilience_window_default)
    }
    return stringResource(R.string.example_resilience_window_range, min, max)
}

@Composable
internal fun formatStorageBytes(value: Long?): String {
    if (value == null) {
        return stringResource(R.string.example_resilience_window_default)
    }
    if (value == 0L) {
        return "0 B"
    }
    if (value >= 1024L * 1024L * 1024L) {
        return String.format(Locale.getDefault(), "%.1f GB", value / (1024.0 * 1024.0 * 1024.0))
    }
    if (value >= 1024L * 1024L) {
        return String.format(Locale.getDefault(), "%.0f MB", value / (1024.0 * 1024.0))
    }
    if (value >= 1024L) {
        return String.format(Locale.getDefault(), "%.0f KB", value / 1024.0)
    }
    return "$value B"
}

internal fun displayNameForUri(context: Context, uri: Uri): String {
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
            val columnIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (columnIndex >= 0 && cursor.moveToFirst()) {
                cursor.getString(columnIndex)?.takeIf { it.isNotBlank() }?.let { return it }
            }
        }

    return uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
        ?: uri.toString()
}

internal tailrec fun Context.findActivity(): Activity? =
    when (this) {
        is Activity -> this
        is ContextWrapper -> baseContext.findActivity()
        else -> null
    }
