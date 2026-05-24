import Flutter
import Foundation
import VesperPlayerKit

extension TimelineUiState {
    func toMap() -> [String: Any] {
        [
            "kind": kind.toWireName(),
            "isSeekable": isSeekable,
            "seekableRange": flutterValue(seekableRange.map {
                [
                    "startMs": $0.startMs,
                    "endMs": $0.endMs,
                ]
            }),
            "liveEdgeMs": flutterValue(liveEdgeMs),
            "positionMs": positionMs,
            "durationMs": flutterValue(durationMs),
        ]
    }
}

extension VesperTrackCatalog {
    func toMap() -> [String: Any] {
        [
            "tracks": tracks.map(\.toMap),
            "adaptiveVideo": adaptiveVideo,
            "adaptiveAudio": adaptiveAudio,
        ]
    }
}

extension VesperMediaTrack {
    var toMap: [String: Any] {
        [
            "id": id,
            "kind": kind.toWireName(),
            "label": flutterValue(label),
            "language": flutterValue(language),
            "codec": flutterValue(codec),
            "bitRate": flutterValue(bitRate),
            "width": flutterValue(width),
            "height": flutterValue(height),
            "frameRate": flutterValue(frameRate),
            "channels": flutterValue(channels),
            "sampleRate": flutterValue(sampleRate),
            "isDefault": isDefault,
            "isForced": isForced,
        ]
    }
}

extension VesperTrackSelectionSnapshot {
    func toMap() -> [String: Any] {
        [
            "video": video.toMap(),
            "audio": audio.toMap(),
            "subtitle": subtitle.toMap(),
            "abrPolicy": abrPolicy.toMap(),
        ]
    }
}

extension VesperTrackSelection {
    func toMap() -> [String: Any] {
        [
            "mode": mode.toWireName(),
            "trackId": flutterValue(trackId),
        ]
    }
}

extension VesperAbrPolicy {
    func toMap() -> [String: Any] {
        [
            "mode": mode.toWireName(),
            "trackId": flutterValue(trackId),
            "maxBitRate": flutterValue(maxBitRate),
            "maxWidth": flutterValue(maxWidth),
            "maxHeight": flutterValue(maxHeight),
        ]
    }
}

extension VesperPlaybackResiliencePolicy {
    func toMap() -> [String: Any] {
        [
            "buffering": buffering.toMap(),
            "retry": retry.toMap(),
            "cache": cache.toMap(),
        ]
    }
}

extension VesperBufferingPolicy {
    func toMap() -> [String: Any] {
        [
            "preset": preset.toWireName(),
            "minBufferMs": flutterValue(minBufferMs),
            "maxBufferMs": flutterValue(maxBufferMs),
            "bufferForPlaybackMs": flutterValue(bufferForPlaybackMs),
            "bufferForPlaybackAfterRebufferMs": flutterValue(bufferForPlaybackAfterRebufferMs),
        ]
    }
}

extension VesperRetryPolicy {
    func toMap() -> [String: Any] {
        [
            "maxAttempts": flutterValue(maxAttempts),
            "baseDelayMs": baseDelayMs,
            "maxDelayMs": maxDelayMs,
            "backoff": backoff.toWireName(),
        ]
    }
}

extension VesperCachePolicy {
    func toMap() -> [String: Any] {
        [
            "preset": preset.toWireName(),
            "maxMemoryBytes": flutterValue(maxMemoryBytes),
            "maxDiskBytes": flutterValue(maxDiskBytes),
        ]
    }
}

extension VesperPlayerSource {
    func toMap() -> [String: Any] {
        [
            "uri": uri,
            "label": label,
            "kind": kind.rawValue,
            "protocol": `protocol`.rawValue,
            "headers": headers,
        ]
    }
}

extension VesperDownloadTaskSnapshot {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "assetId": assetId,
            "source": source.toMap,
            "profile": profile.toMap,
            "state": state.toWireName(),
            "progress": progress.toMap,
            "assetIndex": assetIndex.toMap,
            "error": flutterValue(error?.toMap),
        ]
    }
}

extension VesperDownloadTaskStatePatch {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "state": state.toWireName(),
            "progress": progress.toMap,
            "error": flutterValue(error?.toMap),
            "completedPath": flutterValue(completedPath),
        ]
    }
}

extension VesperDownloadTaskProgressPatch {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "progress": progress.toMap,
        ]
    }
}

extension VesperDownloadSource {
    var toMap: [String: Any] {
        [
            "source": source.toMap(),
            "contentFormat": contentFormat.toWireName(),
            "manifestUri": flutterValue(manifestUri),
        ]
    }
}

extension VesperDownloadProfile {
    var toMap: [String: Any] {
        [
            "variantId": flutterValue(variantId),
            "preferredAudioLanguage": flutterValue(preferredAudioLanguage),
            "preferredSubtitleLanguage": flutterValue(preferredSubtitleLanguage),
            "selectedTrackIds": selectedTrackIds,
            "targetOutputFormat": flutterValue(targetOutputFormat?.toWireName()),
            "targetDirectory": flutterValue(targetDirectory?.path),
            "allowMeteredNetwork": allowMeteredNetwork,
        ]
    }
}

extension VesperDownloadProgressSnapshot {
    var toMap: [String: Any] {
        [
            "receivedBytes": receivedBytes,
            "totalBytes": flutterValue(totalBytes),
            "receivedSegments": receivedSegments,
            "totalSegments": flutterValue(totalSegments),
        ]
    }
}

extension VesperDownloadAssetIndex {
    var toMap: [String: Any] {
        [
            "contentFormat": contentFormat.toWireName(),
            "version": flutterValue(version),
            "etag": flutterValue(etag),
            "checksum": flutterValue(checksum),
            "totalSizeBytes": flutterValue(totalSizeBytes),
            "resources": resources.map(\.toMap),
            "segments": segments.map(\.toMap),
            "streams": streams.map(\.toMap),
            "completedPath": flutterValue(completedPath),
        ]
    }
}

extension VesperDownloadResourceRecord {
    var toMap: [String: Any] {
        [
            "resourceId": resourceId,
            "uri": uri,
            "relativePath": flutterValue(relativePath),
            "byteRange": flutterValue(byteRange?.toMap),
            "generatedText": NSNull(),
            "sizeBytes": flutterValue(sizeBytes),
            "etag": flutterValue(etag),
            "checksum": flutterValue(checksum),
        ]
    }
}

extension VesperDownloadStaleResource {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "resourceId": flutterValue(resourceId),
            "segmentId": flutterValue(segmentId),
            "uri": flutterValue(uri),
            "phase": phase == .download ? "download" : "prepare",
            "statusCode": flutterValue(statusCode),
            "receivedBytes": receivedBytes,
            "message": message,
        ]
    }
}

extension VesperDownloadSegmentRecord {
    var toMap: [String: Any] {
        [
            "segmentId": segmentId,
            "uri": uri,
            "relativePath": flutterValue(relativePath),
            "sequence": flutterValue(sequence),
            "byteRange": flutterValue(byteRange?.toMap),
            "sizeBytes": flutterValue(sizeBytes),
            "checksum": flutterValue(checksum),
        ]
    }
}

extension VesperDownloadAssetStream {
    var toMap: [String: Any] {
        [
            "streamId": streamId,
            "kind": kind.toWireName(),
            "language": flutterValue(language),
            "codec": flutterValue(codec),
            "label": flutterValue(label),
            "qualityRank": flutterValue(qualityRank),
            "resourceIds": resourceIds,
            "segmentIds": segmentIds,
            "metadata": metadata,
        ]
    }
}

extension VesperDownloadByteRange {
    var toMap: [String: Any] {
        [
            "offset": offset,
            "length": length,
        ]
    }
}

extension VesperDownloadError {
    var toMap: [String: Any] {
        [
            "code": code.rawValue,
            "category": category.rawValue,
            "retriable": retriable,
            "message": message,
        ]
    }
}

extension VesperPlayerError {
    var toMap: [String: Any] {
        [
            "message": message,
            "code": code.rawValue,
            "category": category.rawValue,
            "retriable": retriable,
        ]
    }
}

func flutterValue(_ value: Any?) -> Any {
    value ?? NSNull()
}

func errorMap(from error: Error) -> [String: Any] {
    let code: String
    let category: String
    if let pluginError = error as? PluginError {
        switch pluginError {
        case .invalidSource:
            code = "invalidSource"
            category = "source"
        case .invalidTrackSelection, .invalidAbrPolicy:
            code = "unsupported"
            category = "capability"
        case .unsupported:
            code = "unsupported"
            category = "capability"
        default:
            code = "backendFailure"
            category = "platform"
        }
    } else {
        code = "backendFailure"
        category = "platform"
    }
    return [
        "message": error.localizedDescription,
        "code": code,
        "category": category,
        "retriable": false,
        "details": [
            "exception": String(describing: type(of: error)),
        ],
    ]
}

func downloadErrorMap(from error: Error) -> [String: Any] {
    [
        "code": "backendFailure",
        "category": "platform",
        "retriable": false,
        "message": error.localizedDescription,
        "details": [
            "exception": String(describing: type(of: error)),
        ],
    ]
}

func asFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: errorMap(from: error)
    )
}

func downloadOutputFormat(from raw: String?) -> VesperDownloadOutputFormat? {
    switch raw {
    case "mp4":
        return .mp4
    case "mkv":
        return .mkv
    case "original":
        return .original
    default:
        return nil
    }
}

func downloadStreamKind(from raw: String?) -> VesperDownloadStreamKind {
    switch raw {
    case "video":
        return .video
    case "audio":
        return .audio
    case "secondaryAudio":
        return .secondaryAudio
    case "subtitle":
        return .subtitle
    case "auxiliary":
        return .auxiliary
    default:
        return .combined
    }
}

func asDownloadFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: downloadErrorMap(from: error)
    )
}

