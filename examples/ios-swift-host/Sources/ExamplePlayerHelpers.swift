import Foundation
import VesperPlayerKit

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

func liveButtonLabel(_ timeline: TimelineUiState) -> String {
    guard let liveEdge = timeline.liveEdgeMs else { return ExampleI18n.goLive }
    let behindMs = max(liveEdge - timeline.positionMs, 0)
    if behindMs > 1_500 {
        return ExampleI18n.liveBehind(formatMillis(behindMs))
    }
    return ExampleI18n.live
}

func timelineSummary(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> String {
    let displayedPosition: Int64 = {
        guard let pendingSeekRatio else { return timeline.positionMs }
        if let range = timeline.seekableRange {
            return range.startMs + Int64(Double(range.endMs - range.startMs) * pendingSeekRatio)
        }
        return Int64(Double(timeline.durationMs ?? 0) * pendingSeekRatio)
    }()

    switch timeline.kind {
    case .live:
        if let liveEdge = timeline.liveEdgeMs {
            return ExampleI18n.liveEdge(formatMillis(liveEdge))
        }
        return ExampleI18n.live
    case .liveDvr:
        return "\(formatMillis(displayedPosition)) / \(formatMillis(timeline.liveEdgeMs ?? timeline.durationMs ?? 0))"
    case .vod:
        return "\(formatMillis(displayedPosition)) / \(formatMillis(timeline.durationMs ?? 0))"
    }
}

func speedBadge(_ value: Float) -> String {
    ExampleI18n.playbackRate(Double(value))
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
