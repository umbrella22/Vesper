import Foundation
import SwiftUI
import UIKit

@MainActor
final class FakePlayerBridge: ObservableObject, ObservablePlayerBridge {
    private var currentSource: VesperPlayerSource?

    @Published private(set) var publishedUiState: PlayerHostUiState
    @Published private(set) var publishedTrackCatalog: VesperTrackCatalog
    @Published private(set) var publishedTrackSelection: VesperTrackSelectionSnapshot

    let backend: PlayerBridgeBackend = .fakeDemo

    var uiState: PlayerHostUiState {
        publishedUiState
    }

    var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
    }

    init(initialSource: VesperPlayerSource? = nil) {
        currentSource = initialSource
        publishedUiState = PlayerHostUiState(
            title: "Vesper",
            subtitle: initialSource.map(previewSourceSubtitle) ?? "iOS host preview bridge",
            sourceLabel: initialSource?.label ?? "No source selected",
            playbackState: .ready,
            playbackRate: 1.0,
            isBuffering: false,
            isInterrupted: false,
            timeline: TimelineUiState(
                kind: .vod,
                isSeekable: true,
                seekableRange: SeekableRangeUi(startMs: 0, endMs: 134_100),
                liveEdgeMs: nil,
                positionMs: 0,
                durationMs: 134_100
            )
        )
        publishedTrackCatalog = .empty
        publishedTrackSelection = VesperTrackSelectionSnapshot()
    }

    func initialize() {}

    func dispose() {}

    func selectSource(_ source: VesperPlayerSource) {
        currentSource = source
        update { current in
            PlayerHostUiState(
                title: current.title,
                subtitle: previewSourceSubtitle(source),
                sourceLabel: source.label,
                playbackState: .ready,
                playbackRate: current.playbackRate,
                isBuffering: false,
                isInterrupted: current.isInterrupted,
                timeline: TimelineUiState(
                    kind: current.timeline.kind,
                    isSeekable: current.timeline.isSeekable,
                    seekableRange: current.timeline.seekableRange,
                    liveEdgeMs: current.timeline.liveEdgeMs,
                    positionMs: 0,
                    durationMs: current.timeline.durationMs
                )
            )
        }
    }

    func attachSurfaceHost(_ host: UIView) {
        if host.subviews.isEmpty {
            let placeholder = UIView(frame: host.bounds)
            placeholder.translatesAutoresizingMaskIntoConstraints = false
            placeholder.backgroundColor = UIColor(white: 0.05, alpha: 1.0)
            placeholder.layer.cornerRadius = 24
            placeholder.layer.masksToBounds = true
            host.addSubview(placeholder)

            NSLayoutConstraint.activate([
                placeholder.leadingAnchor.constraint(equalTo: host.leadingAnchor),
                placeholder.trailingAnchor.constraint(equalTo: host.trailingAnchor),
                placeholder.topAnchor.constraint(equalTo: host.topAnchor),
                placeholder.bottomAnchor.constraint(equalTo: host.bottomAnchor),
            ])
        }
    }

    func detachSurfaceHost() {}

    func play() {
        update {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: .playing,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
    }

    func pause() {
        update {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: .paused,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
    }

    func togglePause() {
        switch publishedUiState.playbackState {
        case .playing:
            pause()
        case .ready, .paused, .finished:
            play()
        }
    }

    func stop() {
        update { current in
            PlayerHostUiState(
                title: current.title,
                subtitle: current.subtitle,
                sourceLabel: current.sourceLabel,
                playbackState: .ready,
                playbackRate: current.playbackRate,
                isBuffering: false,
                isInterrupted: current.isInterrupted,
                timeline: TimelineUiState(
                    kind: current.timeline.kind,
                    isSeekable: current.timeline.isSeekable,
                    seekableRange: current.timeline.seekableRange,
                    liveEdgeMs: current.timeline.liveEdgeMs,
                    positionMs: 0,
                    durationMs: current.timeline.durationMs
                )
            )
        }
    }

    func seek(by deltaMs: Int64) {
        update { current in
            let range = current.timeline.seekableRange
            let minimum = range?.startMs ?? 0
            let maximum = range?.endMs ?? (current.timeline.durationMs ?? 0)
            let target = min(max(current.timeline.positionMs + deltaMs, minimum), maximum)
            return PlayerHostUiState(
                title: current.title,
                subtitle: current.subtitle,
                sourceLabel: current.sourceLabel,
                playbackState: current.playbackState,
                playbackRate: current.playbackRate,
                isBuffering: current.isBuffering,
                isInterrupted: current.isInterrupted,
                timeline: TimelineUiState(
                    kind: current.timeline.kind,
                    isSeekable: current.timeline.isSeekable,
                    seekableRange: current.timeline.seekableRange,
                    liveEdgeMs: current.timeline.liveEdgeMs,
                    positionMs: target,
                    durationMs: current.timeline.durationMs
                )
            )
        }
    }

    func seek(toRatio ratio: Double) {
        update { current in
            let normalized = min(max(ratio, 0.0), 1.0)
            let position: Int64

            if let range = current.timeline.seekableRange, range.endMs >= range.startMs {
                let width = Double(range.endMs - range.startMs)
                position = range.startMs + Int64(width * normalized)
            } else {
                position = Int64(Double(current.timeline.durationMs ?? 0) * normalized)
            }

            return PlayerHostUiState(
                title: current.title,
                subtitle: current.subtitle,
                sourceLabel: current.sourceLabel,
                playbackState: current.playbackState,
                playbackRate: current.playbackRate,
                isBuffering: current.isBuffering,
                isInterrupted: current.isInterrupted,
                timeline: TimelineUiState(
                    kind: current.timeline.kind,
                    isSeekable: current.timeline.isSeekable,
                    seekableRange: current.timeline.seekableRange,
                    liveEdgeMs: current.timeline.liveEdgeMs,
                    positionMs: position,
                    durationMs: current.timeline.durationMs
                )
            )
        }
    }

    func seekToLiveEdge() {
        update { current in
            let target = current.timeline.liveEdgeMs
                ?? current.timeline.seekableRange?.endMs
                ?? current.timeline.positionMs
            return PlayerHostUiState(
                title: current.title,
                subtitle: current.subtitle,
                sourceLabel: current.sourceLabel,
                playbackState: current.playbackState,
                playbackRate: current.playbackRate,
                isBuffering: current.isBuffering,
                isInterrupted: current.isInterrupted,
                timeline: TimelineUiState(
                    kind: current.timeline.kind,
                    isSeekable: current.timeline.isSeekable,
                    seekableRange: current.timeline.seekableRange,
                    liveEdgeMs: current.timeline.liveEdgeMs,
                    positionMs: target,
                    durationMs: current.timeline.durationMs
                )
            )
        }
    }

    func setPlaybackRate(_ rate: Float) {
        update { current in
            PlayerHostUiState(
                title: current.title,
                subtitle: current.subtitle,
                sourceLabel: current.sourceLabel,
                playbackState: current.playbackState,
                playbackRate: rate,
                isBuffering: current.isBuffering,
                isInterrupted: current.isInterrupted,
                timeline: current.timeline
            )
        }
    }

    func setVideoTrackSelection(_ selection: VesperTrackSelection) {}

    func setAudioTrackSelection(_ selection: VesperTrackSelection) {}

    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) {}

    func setAbrPolicy(_ policy: VesperAbrPolicy) {}

    private func update(_ transform: (PlayerHostUiState) -> PlayerHostUiState) {
        publishedUiState = transform(publishedUiState)
    }
}

private func previewSourceSubtitle(_ source: VesperPlayerSource) -> String {
    switch source.kind {
    case .local:
        return "iOS host preview bridge (local source)"
    case .remote:
        return "iOS host preview bridge (\(source.protocol.rawValue) remote source)"
    }
}
