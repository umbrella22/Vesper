import Foundation
import VesperPlayerKit

extension VesperPlaybackDiagnosticsSnapshot {
    func toMap() -> [String: Any] {
        [
            "playbackEpoch": playbackEpoch,
            "audio": audio.toMap(),
            "firstFrame": flutterValue(firstFrame?.toMap()),
        ]
    }
}

extension VesperFirstFrameObservation {
    func toMap() -> [String: Any] {
        [
            "playbackEpoch": playbackEpoch,
            "elapsedSinceLoadStartMs": elapsedSinceLoadStartMs,
            "kind": kind.rawValue,
            "mediaPositionMs": flutterValue(mediaPositionMs),
        ]
    }
}

extension VesperAudioPlaybackDiagnostics {
    func toMap() -> [String: Any] {
        [
            "trackId": flutterValue(trackId),
            "formatId": flutterValue(formatId),
            "codec": flutterValue(codec),
            "sampleMimeType": flutterValue(sampleMimeType),
            "decoderName": flutterValue(decoderName),
            "channels": flutterValue(channels),
            "sampleRate": flutterValue(sampleRate),
            "evidence": evidence.rawValue,
            "lastIssue": flutterValue(lastIssue.map { issue in
                [
                    "kind": issue.kind.rawValue,
                    "elapsedSinceLoadStartMs": issue.elapsedSinceLoadStartMs,
                    "platformCode": flutterValue(issue.platformCode),
                    "message": flutterValue(issue.message),
                ] as [String: Any]
            }),
        ]
    }
}
