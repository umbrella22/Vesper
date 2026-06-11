@preconcurrency import AVFoundation
import Foundation

enum PendingNativeFrameSeek {
    case position(Int64)
    case ratio(Double)

    func resolve(using timeline: TimelineUiState) -> Int64 {
        switch self {
        case .position(let positionMs):
            return timeline.clampedPosition(positionMs)
        case .ratio(let ratio):
            return timeline.position(forRatio: ratio)
        }
    }
}

func timeControlStatusName(_ status: AVPlayer.TimeControlStatus) -> String {
    switch status {
    case .paused:
        return "paused"
    case .waitingToPlayAtSpecifiedRate:
        return "waiting"
    case .playing:
        return "playing"
    @unknown default:
        return "unknown"
    }
}

func itemStatusName(_ status: AVPlayerItem.Status) -> String {
    switch status {
    case .unknown:
        return "unknown"
    case .readyToPlay:
        return "readyToPlay"
    case .failed:
        return "failed"
    @unknown default:
        return "unknown"
    }
}

let maxPlayerItemErrorLogDetailLength = 256
let maxPlayerItemErrorLogEvents = 5

struct VesperNativePlayerItemStatusEvidence {
    let status: AVPlayerItem.Status

    var details: [String: String] {
        [
            "avPlayerItemStatusEvidenceSource": "avPlayerItemStatus",
            "avPlayerItemStatus": itemStatusName(status),
        ]
    }
}

func playerItemStatusDetailsForTesting(_ status: AVPlayerItem.Status) -> [String: String] {
    playerItemStatusDetails(status)
}

func playerItemStatusDetails(_ status: AVPlayerItem.Status) -> [String: String] {
    VesperNativePlayerItemStatusEvidence(status: status).details
}

struct VesperNativePlayerItemErrorLogEvidence {
    let eventCount: Int
    let events: [VesperNativePlayerItemErrorLogEventEvidence]

    var details: [String: String] {
        guard let latest = events.last else {
            return [:]
        }
        var details = [
            "avPlayerItemErrorLogEvidenceSource": "avPlayerItemErrorLog",
            "avPlayerItemErrorLogEventCount": String(eventCount),
            "avPlayerItemErrorLogRecentEventCount": String(events.count),
            "avPlayerItemErrorStatusCode": String(latest.errorStatusCode),
            "avPlayerItemErrorDomain": truncatedErrorLogValue(latest.errorDomain),
        ]
        putTruncated(latest.uri, for: "avPlayerItemErrorUri", into: &details)
        putTruncated(latest.serverAddress, for: "avPlayerItemErrorServerAddress", into: &details)
        putTruncated(latest.playbackSessionID, for: "avPlayerItemErrorPlaybackSessionID", into: &details)
        putTruncated(latest.errorComment, for: "avPlayerItemErrorComment", into: &details)
        if let eventsSummary {
            details["avPlayerItemErrorLogEvents"] = eventsSummary
        }
        return details
    }

    private var eventsSummary: String? {
        let eventObjects = events.map { $0.summaryObject }
        guard JSONSerialization.isValidJSONObject(eventObjects),
            let data = try? JSONSerialization.data(withJSONObject: eventObjects, options: [.sortedKeys]),
            let value = String(data: data, encoding: .utf8)
        else {
            return nil
        }
        return value
    }

    private func putTruncated(
        _ value: String?,
        for key: String,
        into details: inout [String: String]
    ) {
        guard let value, !value.isEmpty else {
            return
        }
        details[key] = truncatedErrorLogValue(value)
    }
}

struct VesperNativePlayerItemErrorLogEventEvidence {
    let uri: String?
    let serverAddress: String?
    let playbackSessionID: String?
    let errorStatusCode: Int
    let errorDomain: String
    let errorComment: String?

    var summaryObject: [String: Any] {
        var values: [String: Any] = [
            "errorStatusCode": errorStatusCode,
            "errorDomain": truncatedErrorLogValue(errorDomain),
        ]
        putTruncated(uri, for: "uri", into: &values)
        putTruncated(serverAddress, for: "serverAddress", into: &values)
        putTruncated(playbackSessionID, for: "playbackSessionID", into: &values)
        putTruncated(errorComment, for: "errorComment", into: &values)
        return values
    }

    private func putTruncated(
        _ value: String?,
        for key: String,
        into values: inout [String: Any]
    ) {
        guard let value, !value.isEmpty else {
            return
        }
        values[key] = truncatedErrorLogValue(value)
    }
}

func playerItemErrorLogDetails(_ item: AVPlayerItem) -> [String: String] {
    guard let events = item.errorLog()?.events, !events.isEmpty else {
        return [:]
    }
    let recentEvents = Array(events.suffix(maxPlayerItemErrorLogEvents))
    for event in recentEvents {
        iosHostLog(
            "itemErrorLog uri=\(event.uri ?? "nil") status=\(event.errorStatusCode) domain=\(event.errorDomain) comment=\(event.errorComment ?? "nil")"
        )
    }
    return playerItemErrorLogDetails(
        eventCount: events.count,
        events: recentEvents.map {
            VesperNativePlayerItemErrorLogEventEvidence(
                uri: $0.uri,
                serverAddress: $0.serverAddress,
                playbackSessionID: $0.playbackSessionID,
                errorStatusCode: $0.errorStatusCode,
                errorDomain: $0.errorDomain,
                errorComment: $0.errorComment
            )
        }
    )
}

func playerItemErrorLogDetailsForTesting(
    eventCount: Int,
    uri: String?,
    serverAddress: String?,
    playbackSessionID: String?,
    errorStatusCode: Int,
    errorDomain: String,
    errorComment: String?
) -> [String: String] {
    playerItemErrorLogDetails(
        eventCount: eventCount,
        events: [
            VesperNativePlayerItemErrorLogEventEvidence(
                uri: uri,
                serverAddress: serverAddress,
                playbackSessionID: playbackSessionID,
                errorStatusCode: errorStatusCode,
                errorDomain: errorDomain,
                errorComment: errorComment
            ),
        ]
    )
}

func playerItemErrorLogDetailsForTesting(
    eventCount: Int,
    events: [[String: Any?]]
) -> [String: String] {
    playerItemErrorLogDetails(
        eventCount: eventCount,
        events: events.suffix(maxPlayerItemErrorLogEvents).map {
            VesperNativePlayerItemErrorLogEventEvidence(
                uri: $0["uri"] as? String,
                serverAddress: $0["serverAddress"] as? String,
                playbackSessionID: $0["playbackSessionID"] as? String,
                errorStatusCode: $0["errorStatusCode"] as? Int ?? 0,
                errorDomain: $0["errorDomain"] as? String ?? "unknown",
                errorComment: $0["errorComment"] as? String
            )
        }
    )
}

func playerItemErrorLogDetails(
    eventCount: Int,
    events: [VesperNativePlayerItemErrorLogEventEvidence]
) -> [String: String] {
    VesperNativePlayerItemErrorLogEvidence(
        eventCount: eventCount,
        events: events
    ).details
}

func truncatedErrorLogValue(_ value: String) -> String {
    guard value.count > maxPlayerItemErrorLogDetailLength else {
        return value
    }
    let endIndex = value.index(
        value.startIndex,
        offsetBy: maxPlayerItemErrorLogDetailLength - 3
    )
    return String(value[..<endIndex]) + "..."
}
