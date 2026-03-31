package io.github.ikaros.vesper.example.androidcomposehost

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import android.provider.OpenableColumns
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelectionMode
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot

internal fun speedBadge(rate: Float): String = "${formatRate(rate)}x"

internal fun qualityButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String =
    when (trackSelection.abrPolicy.mode) {
        VesperAbrMode.FixedTrack ->
            trackCatalog.videoTracks.firstOrNull { it.id == trackSelection.abrPolicy.trackId }
                ?.let(::qualityLabel)
                ?: "Quality"

        VesperAbrMode.Constrained,
        VesperAbrMode.Auto,
        -> "Auto"
    }

internal fun audioButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String =
    when (trackSelection.audio.mode) {
        VesperTrackSelectionMode.Track ->
            trackCatalog.audioTracks.firstOrNull { it.id == trackSelection.audio.trackId }
                ?.let(::audioLabel)
                ?: "Audio"

        else -> "Audio"
    }

internal fun subtitleButtonLabel(
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
): String =
    when (trackSelection.subtitle.mode) {
        VesperTrackSelectionMode.Disabled -> "CC Off"
        VesperTrackSelectionMode.Track ->
            trackCatalog.subtitleTracks.firstOrNull { it.id == trackSelection.subtitle.trackId }
                ?.let(::subtitleLabel)
                ?: "Subtitles"

        VesperTrackSelectionMode.Auto -> "CC Auto"
    }

internal fun qualityLabel(track: VesperMediaTrack): String =
    buildString {
        when {
            track.height != null -> append("${track.height}p")
            track.width != null && track.height != null -> append("${track.width}×${track.height}")
            track.label != null -> append(track.label)
            else -> append("Video Track")
        }
    }

internal fun qualitySubtitle(track: VesperMediaTrack): String =
    listOfNotNull(
        track.codec,
        track.bitRate?.let(::formatBitRate),
    ).joinToString(" • ").ifBlank { "Fixed video variant" }

internal fun audioLabel(track: VesperMediaTrack): String =
    track.label ?: track.language?.uppercase() ?: "Audio Track"

internal fun audioSubtitle(track: VesperMediaTrack): String =
    listOfNotNull(
        track.language?.uppercase(),
        track.channels?.let { "$it ch" },
        track.sampleRate?.let { "${it / 1000} kHz" },
        track.codec,
    ).joinToString(" • ").ifBlank { "Audio program" }

internal fun subtitleLabel(track: VesperMediaTrack): String =
    track.label ?: track.language?.uppercase() ?: "Subtitle Track"

internal fun subtitleSubtitle(track: VesperMediaTrack): String =
    listOfNotNull(
        track.language?.uppercase(),
        if (track.isForced) "Forced" else null,
        if (track.isDefault) "Default" else null,
    ).joinToString(" • ").ifBlank { "Subtitle option" }

internal fun stageBadgeText(timeline: TimelineUiState): String =
    when (timeline.kind) {
        TimelineKind.Live -> "Live stream"
        TimelineKind.LiveDvr -> "Live with DVR window"
        TimelineKind.Vod -> "Video on demand"
    }

internal fun liveButtonLabel(timeline: TimelineUiState): String {
    val liveEdge = timeline.liveEdgeMs ?: return "Go Live"
    val behindMs = (liveEdge - timeline.positionMs).coerceAtLeast(0L)
    return if (behindMs > 1_500L) {
        "LIVE -${formatMillis(behindMs)}"
    } else {
        "LIVE"
    }
}

internal fun timelineSummary(
    timeline: TimelineUiState,
    pendingSeekRatio: Float?,
): String {
    val displayedPosition =
        pendingSeekRatio?.let { ratio ->
            val range = timeline.seekableRange
            if (range != null) {
                (range.startMs + ((range.endMs - range.startMs).toFloat() * ratio)).toLong()
            } else {
                (((timeline.durationMs ?: 0L).toFloat()) * ratio).toLong()
            }
        } ?: timeline.positionMs

    return when (timeline.kind) {
        TimelineKind.Live ->
            timeline.liveEdgeMs?.let { "LIVE • Edge ${formatMillis(it)}" } ?: "LIVE"

        TimelineKind.LiveDvr -> {
            val liveEdge = timeline.liveEdgeMs ?: timeline.durationMs ?: 0L
            "${formatMillis(displayedPosition)} / ${formatMillis(liveEdge)}"
        }

        TimelineKind.Vod ->
            "${formatMillis(displayedPosition)} / ${formatMillis(timeline.durationMs ?: 0L)}"
    }
}

internal fun formatBitRate(value: Long): String =
    when {
        value >= 1_000_000L -> String.format("%.1f Mbps", value / 1_000_000.0)
        value >= 1_000L -> String.format("%.0f kbps", value / 1_000.0)
        else -> "$value bps"
    }

internal fun formatMillis(value: Long): String {
    val totalSeconds = value / 1000L
    val minutes = totalSeconds / 60L
    val seconds = totalSeconds % 60L
    return "%02d:%02d".format(minutes, seconds)
}

internal fun formatRate(value: Float): String = "%.1f".format(value)

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
