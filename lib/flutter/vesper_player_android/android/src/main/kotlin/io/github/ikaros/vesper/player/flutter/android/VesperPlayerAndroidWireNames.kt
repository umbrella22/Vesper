package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperBufferingPreset
import io.github.ikaros.vesper.player.android.VesperCachePreset
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
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadStaleResource
import io.github.ikaros.vesper.player.android.VesperDownloadTaskProgressPatch
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadTaskStatePatch
import io.github.ikaros.vesper.player.android.VesperMediaTrackKind
import io.github.ikaros.vesper.player.android.VesperPlayerBackendFamily
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperRetryBackoff
import io.github.ikaros.vesper.player.android.VesperTrackSelectionMode

internal fun PlaybackStateUi.toWireName(): String =
    when (this) {
        PlaybackStateUi.Ready -> "ready"
        PlaybackStateUi.Playing -> "playing"
        PlaybackStateUi.Paused -> "paused"
        PlaybackStateUi.Finished -> "finished"
    }

internal fun TimelineKind.toWireName(): String =
    when (this) {
        TimelineKind.Vod -> "vod"
        TimelineKind.Live -> "live"
        TimelineKind.LiveDvr -> "liveDvr"
    }

internal fun VesperPlayerBackendFamily.toBackendFamilyWireName(): String =
    when (this) {
        VesperPlayerBackendFamily.FakeDemo -> "fakeDemo"
        VesperPlayerBackendFamily.AndroidHostKit -> "androidHostKit"
    }

internal fun VesperPlayerSourceKind.toWireName(): String =
    when (this) {
        VesperPlayerSourceKind.Local -> "local"
        VesperPlayerSourceKind.Remote -> "remote"
    }

internal fun VesperPlayerSourceProtocol.toWireName(): String =
    when (this) {
        VesperPlayerSourceProtocol.Unknown -> "unknown"
        VesperPlayerSourceProtocol.File -> "file"
        VesperPlayerSourceProtocol.Content -> "content"
        VesperPlayerSourceProtocol.Progressive -> "progressive"
        VesperPlayerSourceProtocol.Hls -> "hls"
        VesperPlayerSourceProtocol.Dash -> "dash"
    }

internal fun VesperBufferingPreset.toWireName(): String =
    when (this) {
        VesperBufferingPreset.Default -> "defaultPreset"
        VesperBufferingPreset.Balanced -> "balanced"
        VesperBufferingPreset.Streaming -> "streaming"
        VesperBufferingPreset.Resilient -> "resilient"
        VesperBufferingPreset.LowLatency -> "lowLatency"
    }

internal fun VesperRetryBackoff.toWireName(): String =
    when (this) {
        VesperRetryBackoff.Fixed -> "fixed"
        VesperRetryBackoff.Linear -> "linear"
        VesperRetryBackoff.Exponential -> "exponential"
    }

internal fun VesperCachePreset.toWireName(): String =
    when (this) {
        VesperCachePreset.Default -> "defaultPreset"
        VesperCachePreset.Disabled -> "disabled"
        VesperCachePreset.Streaming -> "streaming"
        VesperCachePreset.Resilient -> "resilient"
    }

internal fun VesperMediaTrackKind.toWireName(): String =
    when (this) {
        VesperMediaTrackKind.Video -> "video"
        VesperMediaTrackKind.Audio -> "audio"
        VesperMediaTrackKind.Subtitle -> "subtitle"
    }

internal fun VesperTrackSelectionMode.toWireName(): String =
    when (this) {
        VesperTrackSelectionMode.Auto -> "auto"
        VesperTrackSelectionMode.Disabled -> "disabled"
        VesperTrackSelectionMode.Track -> "track"
    }

internal fun VesperAbrMode.toWireName(): String =
    when (this) {
        VesperAbrMode.Auto -> "auto"
        VesperAbrMode.Constrained -> "constrained"
        VesperAbrMode.FixedTrack -> "fixedTrack"
    }

internal fun VesperPlayerSource.toMap(): Map<String, Any?> =
    mapOf(
        "uri" to uri,
        "label" to label,
        "kind" to kind.toWireName(),
        "protocol" to protocol.toWireName(),
        "headers" to headers,
    )

internal fun VesperDownloadTaskSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "taskId" to taskId,
        "assetId" to assetId,
        "source" to source.toMap(),
        "profile" to profile.toMap(),
        "state" to state.toWireName(),
        "progress" to progress.toMap(),
        "assetIndex" to assetIndex.toMap(),
        "error" to error?.toMap(),
    )

internal fun VesperDownloadTaskStatePatch.toMap(): Map<String, Any?> =
    mapOf(
        "taskId" to taskId,
        "state" to state.toWireName(),
        "progress" to progress.toMap(),
        "error" to error?.toMap(),
        "completedPath" to completedPath,
    )

internal fun VesperDownloadTaskProgressPatch.toMap(): Map<String, Any?> =
    mapOf(
        "taskId" to taskId,
        "progress" to progress.toMap(),
    )

internal fun VesperDownloadSource.toMap(): Map<String, Any?> =
    mapOf(
        "source" to source.toMap(),
        "contentFormat" to contentFormat.toWireName(),
        "manifestUri" to manifestUri,
    )

internal fun VesperDownloadProfile.toMap(): Map<String, Any?> =
    mapOf(
        "variantId" to variantId,
        "preferredAudioLanguage" to preferredAudioLanguage,
        "preferredSubtitleLanguage" to preferredSubtitleLanguage,
        "selectedTrackIds" to selectedTrackIds,
        "targetOutputFormat" to targetOutputFormat?.toWireName(),
        "targetDirectory" to targetDirectory,
        "allowMeteredNetwork" to allowMeteredNetwork,
    )

internal fun VesperDownloadProgressSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "receivedBytes" to receivedBytes,
        "totalBytes" to totalBytes,
        "receivedSegments" to receivedSegments,
        "totalSegments" to totalSegments,
    )

internal fun VesperDownloadAssetIndex.toMap(): Map<String, Any?> =
    mapOf(
        "contentFormat" to contentFormat.toWireName(),
        "version" to version,
        "etag" to etag,
        "checksum" to checksum,
        "totalSizeBytes" to totalSizeBytes,
        "resources" to resources.map(VesperDownloadResourceRecord::toMap),
        "segments" to segments.map(VesperDownloadSegmentRecord::toMap),
        "streams" to streams.map(VesperDownloadAssetStream::toMap),
        "completedPath" to completedPath,
    )

internal fun VesperDownloadResourceRecord.toMap(): Map<String, Any?> =
    mapOf(
        "resourceId" to resourceId,
        "uri" to uri,
        "relativePath" to relativePath,
        "byteRange" to byteRange?.toMap(),
        "generatedText" to null,
        "sizeBytes" to sizeBytes,
        "etag" to etag,
        "checksum" to checksum,
    )

internal fun VesperDownloadStaleResource.toMap(): Map<String, Any?> =
    mapOf(
        "taskId" to taskId,
        "resourceId" to resourceId,
        "segmentId" to segmentId,
        "uri" to uri,
        "phase" to phase.name.replaceFirstChar { it.lowercase() },
        "statusCode" to statusCode,
        "receivedBytes" to receivedBytes,
        "message" to message,
    )

internal fun VesperDownloadSegmentRecord.toMap(): Map<String, Any?> =
    mapOf(
        "segmentId" to segmentId,
        "uri" to uri,
        "relativePath" to relativePath,
        "sequence" to sequence,
        "byteRange" to byteRange?.toMap(),
        "sizeBytes" to sizeBytes,
        "checksum" to checksum,
    )

internal fun VesperDownloadAssetStream.toMap(): Map<String, Any?> =
    mapOf(
        "streamId" to streamId,
        "kind" to kind.name.replaceFirstChar { it.lowercase() },
        "language" to language,
        "codec" to codec,
        "label" to label,
        "qualityRank" to qualityRank,
        "resourceIds" to resourceIds,
        "segmentIds" to segmentIds,
        "metadata" to metadata,
    )

internal fun VesperDownloadByteRange.toMap(): Map<String, Any?> =
    mapOf(
        "offset" to offset,
        "length" to length,
    )

internal fun VesperDownloadError.toMap(): Map<String, Any?> =
    mapOf(
        "code" to code.wireName,
        "category" to category.wireName,
        "retriable" to retriable,
        "message" to message,
    )

internal fun VesperDownloadState.toWireName(): String =
    when (this) {
        VesperDownloadState.Queued -> "queued"
        VesperDownloadState.Preparing -> "preparing"
        VesperDownloadState.Downloading -> "downloading"
        VesperDownloadState.Paused -> "paused"
        VesperDownloadState.Completed -> "completed"
        VesperDownloadState.Failed -> "failed"
        VesperDownloadState.Removed -> "removed"
    }

internal fun VesperDownloadContentFormat.toWireName(): String =
    when (this) {
        VesperDownloadContentFormat.HlsSegments -> "hlsSegments"
        VesperDownloadContentFormat.DashSegments -> "dashSegments"
        VesperDownloadContentFormat.FlvSegments -> "flvSegments"
        VesperDownloadContentFormat.SingleFile -> "singleFile"
        VesperDownloadContentFormat.Unknown -> "unknown"
    }

internal fun VesperDownloadOutputFormat.toWireName(): String =
    when (this) {
        VesperDownloadOutputFormat.Mp4 -> "mp4"
        VesperDownloadOutputFormat.Mkv -> "mkv"
        VesperDownloadOutputFormat.Original -> "original"
    }
