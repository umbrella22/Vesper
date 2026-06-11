import Foundation
import VesperPlayerKit

enum VesperPlayerLiveButtonState: Equatable {
    case goLive
    case live
    case liveBehind(Int64)
}

enum VesperPlayerTimelineSummaryState: Equatable {
    case live
    case liveEdge(Int64)
    case window(positionMs: Int64, endMs: Int64)
}

func displayedTimelinePositionMs(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> Int64 {
    if let pendingSeekRatio {
        return timeline.position(forRatio: pendingSeekRatio)
    }
    return timeline.clampedPosition(timeline.positionMs)
}

func liveButtonState(_ timeline: TimelineUiState) -> VesperPlayerLiveButtonState {
    guard let liveEdge = timeline.goLivePositionMs else { return .goLive }
    let behindMs = max(liveEdge - timeline.clampedPosition(timeline.positionMs), 0)
    if behindMs > 1_500 {
        return .liveBehind(behindMs)
    }
    return .live
}

func timelineSummaryState(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> VesperPlayerTimelineSummaryState {
    let displayedPosition = displayedTimelinePositionMs(timeline, pendingSeekRatio: pendingSeekRatio)

    switch timeline.kind {
    case .live:
        if let liveEdge = timeline.goLivePositionMs {
            return .liveEdge(liveEdge)
        }
        return .live
    case .liveDvr:
        return liveDvrWindowSummary(timeline, displayedPosition: displayedPosition)
    case .vod:
        return .window(
            positionMs: displayedPosition,
            endMs: timeline.durationMs ?? 0
        )
    }
}

private func liveDvrWindowSummary(
    _ timeline: TimelineUiState,
    displayedPosition: Int64
) -> VesperPlayerTimelineSummaryState {
    let rangeStart = timeline.seekableRange?.startMs ?? 0
    let windowEnd = timeline.goLivePositionMs ?? timeline.durationMs ?? 0
    return .window(
        positionMs: max(displayedPosition - rangeStart, 0),
        endMs: max(windowEnd - rangeStart, 0)
    )
}

func qualityButtonLabel(_ policy: VesperAbrPolicy) -> String {
    switch policy.mode {
    case .auto:
        return VesperPlayerStageStrings.auto
    case .constrained:
        if let maxWidth = policy.maxWidth, let maxHeight = policy.maxHeight {
            let resolutionLabel = "\(maxWidth)x\(maxHeight)"
            if let maxBitRate = policy.maxBitRate {
                return "\(resolutionLabel) / \(formatBitRate(maxBitRate))"
            } else {
                return resolutionLabel
            }
        } else if let maxBitRate = policy.maxBitRate {
            return formatBitRate(maxBitRate)
        } else {
            return VesperPlayerStageStrings.qualityButtonCapped
        }
    case .fixedTrack:
        return VesperPlayerStageStrings.qualityButtonPinned
    }
}

func qualityButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?
) -> String {
    let requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection)
    let effectiveTrack = effectiveVideoTrack(trackCatalog, effectiveVideoTrackId)
    let resolvedStatus = currentFixedTrackStatus(
        trackCatalog,
        trackSelection,
        effectiveVideoTrackId: effectiveVideoTrackId,
        fixedTrackStatus: fixedTrackStatus
    )

    switch trackSelection.abrPolicy.mode {
    case .fixedTrack:
        guard let requestedTrack else {
            return VesperPlayerStageStrings.quality
        }
        switch resolvedStatus {
        case .pending, .fallback:
            return "\(VesperPlayerStageStrings.qualityButtonLocking) · \(qualityLabel(requestedTrack))"
        case .locked, nil:
            return "\(VesperPlayerStageStrings.qualityButtonPinned) · \(qualityLabel(requestedTrack))"
        }
    case .constrained, .auto:
        if let effectiveTrack {
            return "\(VesperPlayerStageStrings.auto) · \(qualityLabel(effectiveTrack))"
        }
        return qualityButtonLabel(trackSelection.abrPolicy)
    }
}

func effectiveVideoTrack(
    _ trackCatalog: VesperTrackCatalog,
    _ effectiveVideoTrackId: String?
) -> VesperMediaTrack? {
    guard let effectiveVideoTrackId else { return nil }
    return trackCatalog.videoTracks.first { $0.id == effectiveVideoTrackId }
}

func requestedFixedVideoTrack(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> VesperMediaTrack? {
    guard
        trackSelection.abrPolicy.mode == .fixedTrack,
        let trackId = trackSelection.abrPolicy.trackId
    else {
        return nil
    }
    return trackCatalog.videoTracks.first { $0.id == trackId }
}

func currentFixedTrackStatus(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?
) -> VesperFixedTrackStatus? {
    guard trackSelection.abrPolicy.mode == .fixedTrack else { return nil }
    if let fixedTrackStatus {
        return fixedTrackStatus
    }
    guard let requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection) else {
        return .pending
    }
    guard let effectiveVideoTrackId else {
        return .pending
    }
    return effectiveVideoTrackId == requestedTrack.id ? .locked : .fallback
}

func stageBadgeText(_ timeline: TimelineUiState) -> String {
    switch timeline.kind {
    case .vod:
        return VesperPlayerStageStrings.stageVideoOnDemand
    case .live:
        return VesperPlayerStageStrings.stageLiveStream
    case .liveDvr:
        return VesperPlayerStageStrings.stageLiveWithDvrWindow
    }
}

func liveButtonLabel(_ timeline: TimelineUiState) -> String {
    switch liveButtonState(timeline) {
    case .goLive:
        return VesperPlayerStageStrings.goLive
    case .live:
        return VesperPlayerStageStrings.live
    case let .liveBehind(behindMs):
        return VesperPlayerStageStrings.liveBehind(formatMillis(behindMs))
    }
}

func timelineSummary(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> String {
    switch timelineSummaryState(timeline, pendingSeekRatio: pendingSeekRatio) {
    case .live:
        return VesperPlayerStageStrings.live
    case let .liveEdge(liveEdge):
        return VesperPlayerStageStrings.liveEdge(formatMillis(liveEdge))
    case let .window(positionMs, endMs):
        return "\(formatMillis(positionMs)) / \(formatMillis(endMs))"
    }
}

func compactTimelineSummary(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> String {
    switch timelineSummaryState(timeline, pendingSeekRatio: pendingSeekRatio) {
    case .live, .liveEdge:
        return VesperPlayerStageStrings.live
    case let .window(positionMs, endMs):
        return "\(formatMillis(positionMs))/\(formatMillis(endMs))"
    }
}

func speedBadge(_ value: Float) -> String {
    VesperPlayerStageStrings.playbackRate(Double(value))
}

func qualityLabel(_ track: VesperMediaTrack) -> String {
    if let height = track.height {
        return "\(height)p"
    }
    if let width = track.width {
        return "\(width)w"
    }
    if let label = track.label, !label.isEmpty {
        return label
    }
    if let bitRate = track.bitRate {
        return formatBitRate(bitRate)
    }
    return track.id
}

func formatBitRate(_ value: Int64) -> String {
    if value >= 1_000_000 {
        return VesperPlayerStageStrings.bitRateMbps(Double(value) / 1_000_000.0)
    }
    if value >= 1_000 {
        return VesperPlayerStageStrings.bitRateKbps(Double(value) / 1_000.0)
    }
    return VesperPlayerStageStrings.bitRateBps(value)
}

func formatMillis(_ value: Int64) -> String {
    let totalSeconds = max(value, 0) / 1000
    let minutes = totalSeconds / 60
    let seconds = totalSeconds % 60
    return String(format: "%02d:%02d", minutes, seconds)
}

extension Comparable {
    func clamped(to limits: ClosedRange<Self>) -> Self {
        min(max(self, limits.lowerBound), limits.upperBound)
    }
}
