package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperAbrPolicy
import io.github.ikaros.vesper.player.android.VesperBufferingPolicy
import io.github.ikaros.vesper.player.android.VesperCachePolicy
import io.github.ikaros.vesper.player.android.VesperDownloadAssetIndex
import io.github.ikaros.vesper.player.android.VesperDownloadAssetStream
import io.github.ikaros.vesper.player.android.VesperDownloadByteRange
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadError
import io.github.ikaros.vesper.player.android.VesperDownloadOutputFormat
import io.github.ikaros.vesper.player.android.VesperDownloadProfile
import io.github.ikaros.vesper.player.android.VesperDownloadProgressSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadResourceRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSegmentRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperDownloadStaleResource
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadTaskProgressPatch
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadTaskStatePatch
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperRetryPolicy
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelection
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot

internal fun TimelineUiState.toMap(): Map<String, Any?> =
    mapOf(
        "kind" to kind.toWireName(),
        "isSeekable" to isSeekable,
        "seekableRange" to seekableRange?.let { range ->
            mapOf(
                "startMs" to range.startMs,
                "endMs" to range.endMs,
            )
        },
        "liveEdgeMs" to liveEdgeMs,
        "positionMs" to positionMs,
        "durationMs" to durationMs,
    )

internal fun VesperTrackCatalog.toMap(): Map<String, Any?> =
    mapOf(
        "tracks" to tracks.map(VesperMediaTrack::toMap),
        "adaptiveVideo" to adaptiveVideo,
        "adaptiveAudio" to adaptiveAudio,
    )

internal fun VesperMediaTrack.toMap(): Map<String, Any?> =
    mapOf(
        "id" to id,
        "kind" to kind.toWireName(),
        "label" to label,
        "language" to language,
        "codec" to codec,
        "bitRate" to bitRate,
        "width" to width,
        "height" to height,
        "frameRate" to frameRate?.toDouble(),
        "channels" to channels,
        "sampleRate" to sampleRate,
        "isDefault" to isDefault,
        "isForced" to isForced,
    )

internal fun VesperTrackSelectionSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "video" to video.toMap(),
        "audio" to audio.toMap(),
        "subtitle" to subtitle.toMap(),
        "abrPolicy" to abrPolicy.toMap(),
    )

internal fun VesperTrackSelection.toMap(): Map<String, Any?> =
    mapOf(
        "mode" to mode.toWireName(),
        "trackId" to trackId,
    )

internal fun VesperAbrPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "mode" to mode.toWireName(),
        "trackId" to trackId,
        "maxBitRate" to maxBitRate,
        "maxWidth" to maxWidth,
        "maxHeight" to maxHeight,
    )

internal fun VesperPlaybackResiliencePolicy.toMap(): Map<String, Any?> =
    mapOf(
        "buffering" to buffering.toMap(),
        "retry" to retry.toMap(),
        "cache" to cache.toMap(),
    )

internal fun VesperBufferingPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "preset" to preset.toWireName(),
        "minBufferMs" to minBufferMs,
        "maxBufferMs" to maxBufferMs,
        "bufferForPlaybackMs" to bufferForPlaybackMs,
        "bufferForPlaybackAfterRebufferMs" to bufferForPlaybackAfterRebufferMs,
    )

internal fun VesperRetryPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "maxAttempts" to maxAttempts,
        "baseDelayMs" to baseDelayMs,
        "maxDelayMs" to maxDelayMs,
        "backoff" to backoff.toWireName(),
    )

internal fun VesperCachePolicy.toMap(): Map<String, Any?> =
    mapOf(
        "preset" to preset.toWireName(),
        "maxMemoryBytes" to maxMemoryBytes,
        "maxDiskBytes" to maxDiskBytes,
    )

internal fun Throwable.toErrorMap(): Map<String, Any?> =
    mapOf(
        "message" to (message ?: toString()),
        "code" to "backendFailure",
        "category" to "platform",
        "retriable" to false,
        "details" to mapOf(
            "exception" to this::class.java.name,
        ),
    )

internal fun Throwable.toDownloadErrorMap(): Map<String, Any?> =
    mapOf(
        "code" to "backendFailure",
        "category" to "platform",
        "retriable" to false,
        "message" to (message ?: toString()),
        "details" to mapOf(
            "exception" to this::class.java.name,
        ),
    )

