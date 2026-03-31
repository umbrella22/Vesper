import Foundation
import VesperPlayerKit

func qualityButtonLabel(_ policy: VesperAbrPolicy) -> String {
    switch policy.mode {
    case .auto:
        "Auto"
    case .constrained:
        if let maxBitRate = policy.maxBitRate {
            formatBitRate(maxBitRate)
        } else {
            "Capped"
        }
    case .fixedTrack:
        "Pinned"
    }
}

func audioButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> String {
    guard trackSelection.audio.mode == .track else { return "Audio" }
    return trackCatalog.audioTracks.first { $0.id == trackSelection.audio.trackId }.map(audioLabel) ?? "Audio"
}

func subtitleButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> String {
    switch trackSelection.subtitle.mode {
    case .disabled:
        "CC Off"
    case .auto:
        "CC Auto"
    case .track:
        trackCatalog.subtitleTracks.first { $0.id == trackSelection.subtitle.trackId }.map(subtitleLabel) ?? "Subtitles"
    }
}

func stageBadgeText(_ timeline: TimelineUiState) -> String {
    switch timeline.kind {
    case .vod:
        "Video on demand"
    case .live:
        "Live stream"
    case .liveDvr:
        "Live with DVR window"
    }
}

func liveButtonLabel(_ timeline: TimelineUiState) -> String {
    guard let liveEdge = timeline.liveEdgeMs else { return "Go Live" }
    let behindMs = max(liveEdge - timeline.positionMs, 0)
    if behindMs > 1_500 {
        return "LIVE -\(formatMillis(behindMs))"
    }
    return "LIVE"
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
            return "LIVE • Edge \(formatMillis(liveEdge))"
        }
        return "LIVE"
    case .liveDvr:
        return "\(formatMillis(displayedPosition)) / \(formatMillis(timeline.liveEdgeMs ?? timeline.durationMs ?? 0))"
    case .vod:
        return "\(formatMillis(displayedPosition)) / \(formatMillis(timeline.durationMs ?? 0))"
    }
}

func speedBadge(_ value: Float) -> String {
    String(format: "%.1fx", value)
}

func audioLabel(_ track: VesperMediaTrack) -> String {
    track.label ?? track.language?.uppercased() ?? "Audio Track"
}

func audioSubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.language?.uppercased(),
        track.channels.map { "\($0) ch" },
        track.sampleRate.map { "\($0 / 1000) kHz" },
        track.codec,
    ].compactMap { $0 }
    return parts.isEmpty ? "Audio program" : parts.joined(separator: " • ")
}

func subtitleLabel(_ track: VesperMediaTrack) -> String {
    track.label ?? track.language?.uppercased() ?? "Subtitle Track"
}

func subtitleSubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.language?.uppercased(),
        track.isForced ? "Forced" : nil,
        track.isDefault ? "Default" : nil,
    ].compactMap { $0 }
    return parts.isEmpty ? "Subtitle option" : parts.joined(separator: " • ")
}

func formatBitRate(_ value: Int64) -> String {
    if value >= 1_000_000 {
        return String(format: "%.1f Mbps", Double(value) / 1_000_000.0)
    }
    if value >= 1_000 {
        return String(format: "%.0f kbps", Double(value) / 1_000.0)
    }
    return "\(value) bps"
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
            title: "Data Saver",
            subtitle: "Cap bitrate near 800 kbps for constrained networks.",
            policy: .constrained(maxBitRate: 800_000)
        ),
        AbrPreset(
            id: "balanced",
            title: "Balanced",
            subtitle: "Cap bitrate near 2 Mbps for smoother playback.",
            policy: .constrained(maxBitRate: 2_000_000)
        ),
        AbrPreset(
            id: "high",
            title: "High",
            subtitle: "Cap bitrate near 5 Mbps for higher visual quality.",
            policy: .constrained(maxBitRate: 5_000_000)
        ),
    ]
}

func sheetTitle(_ sheet: ExamplePlayerSheet) -> String {
    switch sheet {
    case .menu:
        "Playback Tools"
    case .quality:
        "Quality"
    case .audio:
        "Audio"
    case .subtitle:
        "Subtitles"
    case .speed:
        "Playback Speed"
    }
}

func sheetSubtitle(_ sheet: ExamplePlayerSheet) -> String {
    switch sheet {
    case .menu:
        "Open track, subtitle, quality, and speed controls without crowding the player overlay."
    case .quality:
        "The current AVPlayer route uses bitrate caps instead of fixed video renditions."
    case .audio:
        "Choose from the audible media groups exposed by the stream."
    case .subtitle:
        "Turn subtitles off, keep them automatic, or pin a specific option."
    case .speed:
        "Preview playback behavior at different speeds."
    }
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
