import Foundation
import VesperPlayerKit

enum ExampleLiveButtonState: Equatable {
    case goLive
    case live
    case liveBehind(Int64)
}

enum ExampleTimelineSummaryState: Equatable {
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

func liveButtonState(_ timeline: TimelineUiState) -> ExampleLiveButtonState {
    guard let liveEdge = timeline.goLivePositionMs else { return .goLive }
    let behindMs = max(liveEdge - timeline.clampedPosition(timeline.positionMs), 0)
    if behindMs > 1_500 {
        return .liveBehind(behindMs)
    }
    return .live
}

func timelineSummaryState(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> ExampleTimelineSummaryState {
    let displayedPosition = displayedTimelinePositionMs(timeline, pendingSeekRatio: pendingSeekRatio)

    switch timeline.kind {
    case .live:
        if let liveEdge = timeline.goLivePositionMs {
            return .liveEdge(liveEdge)
        }
        return .live
    case .liveDvr:
        return .window(
            positionMs: displayedPosition,
            endMs: timeline.goLivePositionMs ?? timeline.durationMs ?? 0
        )
    case .vod:
        return .window(
            positionMs: displayedPosition,
            endMs: timeline.durationMs ?? 0
        )
    }
}

func qualityButtonLabel(_ policy: VesperAbrPolicy) -> String {
    switch policy.mode {
    case .auto:
        ExampleI18n.auto
    case .constrained:
        if let maxBitRate = policy.maxBitRate {
            formatBitRate(maxBitRate)
        } else {
            ExampleI18n.qualityButtonCapped
        }
    case .fixedTrack:
        ExampleI18n.qualityButtonPinned
    }
}

func audioButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> String {
    guard trackSelection.audio.mode == .track else { return ExampleI18n.audio }
    return trackCatalog.audioTracks.first { $0.id == trackSelection.audio.trackId }.map(audioLabel) ?? ExampleI18n.audio
}

func subtitleButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> String {
    switch trackSelection.subtitle.mode {
    case .disabled:
        ExampleI18n.captionsOff
    case .auto:
        ExampleI18n.captionsAuto
    case .track:
        trackCatalog.subtitleTracks.first { $0.id == trackSelection.subtitle.trackId }.map(subtitleLabel) ?? ExampleI18n.subtitles
    }
}

func stageBadgeText(_ timeline: TimelineUiState) -> String {
    switch timeline.kind {
    case .vod:
        ExampleI18n.stageVideoOnDemand
    case .live:
        ExampleI18n.stageLiveStream
    case .liveDvr:
        ExampleI18n.stageLiveWithDvrWindow
    }
}

func playlistHintLabel(_ kind: VesperPlaylistViewportHintKind) -> String {
    switch kind {
    case .visible:
        ExampleI18n.playlistStatusVisible
    case .nearVisible:
        ExampleI18n.playlistStatusNearVisible
    case .prefetchOnly:
        ExampleI18n.playlistStatusPrefetch
    case .hidden:
        ExampleI18n.playlistStatusHidden
    }
}

func liveButtonLabel(_ timeline: TimelineUiState) -> String {
    switch liveButtonState(timeline) {
    case .goLive:
        return ExampleI18n.goLive
    case .live:
        return ExampleI18n.live
    case let .liveBehind(behindMs):
        return ExampleI18n.liveBehind(formatMillis(behindMs))
    }
}

func timelineSummary(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> String {
    switch timelineSummaryState(timeline, pendingSeekRatio: pendingSeekRatio) {
    case .live:
        return ExampleI18n.live
    case let .liveEdge(liveEdge):
        return ExampleI18n.liveEdge(formatMillis(liveEdge))
    case let .window(positionMs, endMs):
        return "\(formatMillis(positionMs)) / \(formatMillis(endMs))"
    }
}

func speedBadge(_ value: Float) -> String {
    ExampleI18n.playbackRate(Double(value))
}

func resilienceBufferingValue(_ policy: VesperBufferingPolicy) -> String {
    "\(bufferingPresetLabel(policy.preset)) · \(bufferWindowLabel(policy))"
}

func resilienceRetryValue(_ policy: VesperRetryPolicy) -> String {
    let attempts = policy.maxAttempts.map(ExampleI18n.resilienceRetryAttempts) ?? ExampleI18n.resilienceRetryUnlimited
    return ExampleI18n.resilienceRetryValue(attempts, retryBackoffLabel(policy.backoff))
}

func resilienceCacheValue(_ policy: VesperCachePolicy) -> String {
    ExampleI18n.resilienceCacheValue(
        cachePresetLabel(policy.preset),
        formatStorageBytes(policy.maxMemoryBytes),
        formatStorageBytes(policy.maxDiskBytes)
    )
}

func bufferingPresetLabel(_ preset: VesperBufferingPreset) -> String {
    switch preset {
    case .default:
        ExampleI18n.resiliencePresetDefault
    case .balanced:
        ExampleI18n.resiliencePresetBalanced
    case .streaming:
        ExampleI18n.resiliencePresetStreaming
    case .resilient:
        ExampleI18n.resiliencePresetResilient
    case .lowLatency:
        ExampleI18n.resiliencePresetLowLatency
    }
}

func cachePresetLabel(_ preset: VesperCachePreset) -> String {
    switch preset {
    case .default:
        ExampleI18n.resiliencePresetDefault
    case .disabled:
        ExampleI18n.resiliencePresetDisabled
    case .streaming:
        ExampleI18n.resiliencePresetStreaming
    case .resilient:
        ExampleI18n.resiliencePresetResilient
    }
}

func retryBackoffLabel(_ backoff: VesperRetryBackoff) -> String {
    switch backoff {
    case .fixed:
        ExampleI18n.resilienceBackoffFixed
    case .linear:
        ExampleI18n.resilienceBackoffLinear
    case .exponential:
        ExampleI18n.resilienceBackoffExponential
    }
}

func bufferWindowLabel(_ policy: VesperBufferingPolicy) -> String {
    guard let minBufferMs = policy.minBufferMs, let maxBufferMs = policy.maxBufferMs else {
        return ExampleI18n.resilienceWindowDefault
    }
    return ExampleI18n.resilienceWindowRange(minBufferMs, maxBufferMs)
}

func formatStorageBytes(_ value: Int64?) -> String {
    guard let value else {
        return ExampleI18n.resilienceWindowDefault
    }
    if value == 0 {
        return "0 B"
    }
    if value >= 1024 * 1024 * 1024 {
        return String(format: "%.1f GB", Double(value) / (1024.0 * 1024.0 * 1024.0))
    }
    if value >= 1024 * 1024 {
        return String(format: "%.0f MB", Double(value) / (1024.0 * 1024.0))
    }
    if value >= 1024 {
        return String(format: "%.0f KB", Double(value) / 1024.0)
    }
    return "\(value) B"
}

func audioLabel(_ track: VesperMediaTrack) -> String {
    track.label ?? track.language?.uppercased() ?? ExampleI18n.audioTrack
}

func audioSubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.language?.uppercased(),
        track.channels.map(ExampleI18n.audioChannels),
        track.sampleRate.map { ExampleI18n.audioSampleRateKhz($0 / 1000) },
        track.codec,
    ].compactMap { $0 }
    return parts.isEmpty ? ExampleI18n.audioProgram : parts.joined(separator: " • ")
}

func subtitleLabel(_ track: VesperMediaTrack) -> String {
    track.label ?? track.language?.uppercased() ?? ExampleI18n.subtitleTrack
}

func subtitleSubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.language?.uppercased(),
        track.isForced ? ExampleI18n.subtitleForced : nil,
        track.isDefault ? ExampleI18n.subtitleDefault : nil,
    ].compactMap { $0 }
    return parts.isEmpty ? ExampleI18n.subtitleOption : parts.joined(separator: " • ")
}

func formatBitRate(_ value: Int64) -> String {
    if value >= 1_000_000 {
        return ExampleI18n.bitRateMbps(Double(value) / 1_000_000.0)
    }
    if value >= 1_000 {
        return ExampleI18n.bitRateKbps(Double(value) / 1_000.0)
    }
    return ExampleI18n.bitRateBps(value)
}

func formatMillis(_ value: Int64) -> String {
    let totalSeconds = value / 1000
    let minutes = totalSeconds / 60
    let seconds = totalSeconds % 60
    return String(format: "%02d:%02d", minutes, seconds)
}

func abrPresets() -> [AbrPreset] {
    [
        AbrPreset(
            id: "data-saver",
            title: ExampleI18n.abrPresetDataSaverTitle,
            subtitle: ExampleI18n.abrPresetDataSaverSubtitle,
            policy: .constrained(maxBitRate: 800_000)
        ),
        AbrPreset(
            id: "balanced",
            title: ExampleI18n.abrPresetBalancedTitle,
            subtitle: ExampleI18n.abrPresetBalancedSubtitle,
            policy: .constrained(maxBitRate: 2_000_000)
        ),
        AbrPreset(
            id: "high",
            title: ExampleI18n.abrPresetHighTitle,
            subtitle: ExampleI18n.abrPresetHighSubtitle,
            policy: .constrained(maxBitRate: 5_000_000)
        ),
    ]
}

func sheetTitle(_ sheet: ExamplePlayerSheet) -> String {
    ExampleI18n.sheetTitle(sheet)
}

func sheetSubtitle(_ sheet: ExamplePlayerSheet) -> String {
    ExampleI18n.sheetSubtitle(sheet)
}

func sheetHeight(for sheet: ExamplePlayerSheet) -> CGFloat {
    switch sheet {
    case .menu:
        360
    case .quality:
        420
    case .audio:
        440
    case .subtitle:
        470
    case .speed:
        360
    }
}

func exampleIosHostLog(_ message: String) {
    print("[VesperPlayerIOSExample] \(message)")
}

extension Comparable {
    func clamped(to limits: ClosedRange<Self>) -> Self {
        min(max(self, limits.lowerBound), limits.upperBound)
    }
}
