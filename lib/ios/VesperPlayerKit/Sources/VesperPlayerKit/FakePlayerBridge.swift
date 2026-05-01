import Foundation
import SwiftUI
import UIKit

@MainActor
final class FakePlayerBridge: ObservableObject, ObservablePlayerBridge {
    private var currentSource: VesperPlayerSource?

    @Published private(set) var publishedUiState: PlayerHostUiState
    @Published private(set) var publishedTrackCatalog: VesperTrackCatalog
    @Published private(set) var publishedTrackSelection: VesperTrackSelectionSnapshot
    @Published private(set) var publishedEffectiveVideoTrackId: String?
    @Published private(set) var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published private(set) var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published private(set) var publishedResiliencePolicy: VesperPlaybackResiliencePolicy
    @Published private(set) var publishedLastError: VesperPlayerError?

    let backend: PlayerBridgeBackend = .fakeDemo
    private let benchmarkRecorder: VesperBenchmarkRecorder

    var uiState: PlayerHostUiState {
        publishedUiState
    }

    var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
    }

    var effectiveVideoTrackId: String? {
        publishedEffectiveVideoTrackId
    }

    var videoVariantObservation: VesperVideoVariantObservation? {
        publishedVideoVariantObservation
    }

    var fixedTrackStatus: VesperFixedTrackStatus? {
        publishedFixedTrackStatus
    }

    var resiliencePolicy: VesperPlaybackResiliencePolicy {
        publishedResiliencePolicy
    }

    var lastError: VesperPlayerError? {
        publishedLastError
    }

    private func recordBenchmark(
        _ eventName: String,
        attributes: [String: String] = [:]
    ) {
        benchmarkRecorder.record(
            eventName,
            sourceProtocol: currentSource?.protocol,
            attributes: attributes
        )
    }

    init(
        initialSource: VesperPlayerSource? = nil,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        benchmarkConfiguration: VesperBenchmarkConfiguration = .disabled
    ) {
        _ = trackPreferencePolicy
        _ = preloadBudgetPolicy
        benchmarkRecorder = VesperBenchmarkRecorder(configuration: benchmarkConfiguration)
        currentSource = initialSource
        publishedUiState = PlayerHostUiState(
            title: VesperPlayerI18n.playerTitle,
            subtitle: initialSource.map(previewSourceSubtitle) ?? VesperPlayerI18n.previewBridgeReady,
            sourceLabel: initialSource?.label ?? VesperPlayerI18n.noSourceSelected,
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
        publishedEffectiveVideoTrackId = nil
        publishedVideoVariantObservation = nil
        publishedFixedTrackStatus = nil
        publishedResiliencePolicy = resiliencePolicy
        publishedLastError = nil
    }

    func initialize() {
        recordBenchmark("initialize_start")
        if currentSource == nil {
            recordBenchmark("initialize_without_source")
        } else {
            recordBenchmark("initialize_completed")
        }
    }

    func dispose() {
        recordBenchmark("dispose_command")
        benchmarkRecorder.dispose()
    }

    func refresh() {}

    func selectSource(_ source: VesperPlayerSource) {
        recordBenchmark(
            "select_source_start",
            attributes: ["targetProtocol": source.protocol.rawValue]
        )
        currentSource = source
        publishedEffectiveVideoTrackId = nil
        publishedVideoVariantObservation = nil
        publishedFixedTrackStatus = nil
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
        recordBenchmark("play_command")
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
        recordBenchmark("pause_command")
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
        recordBenchmark("stop_command")
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
            let target = current.timeline.clampedPosition(current.timeline.positionMs + deltaMs)
            recordBenchmark("seek_start", attributes: ["positionMs": "\(target)"])
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
            let position = current.timeline.position(forRatio: ratio)
            recordBenchmark("seek_start", attributes: ["positionMs": "\(position)"])

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
            let target = current.timeline.goLivePositionMs ?? current.timeline.positionMs
            recordBenchmark("seek_start", attributes: ["positionMs": "\(target)"])
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
        recordBenchmark("set_playback_rate_command", attributes: ["rate": "\(rate)"])
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

    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        publishedResiliencePolicy = policy
    }

    func drainBenchmarkEvents() -> [VesperBenchmarkEvent] {
        benchmarkRecorder.drainEvents()
    }

    func benchmarkSummary() -> VesperBenchmarkSummary {
        benchmarkRecorder.summary()
    }

    private func update(_ transform: (PlayerHostUiState) -> PlayerHostUiState) {
        publishedUiState = transform(publishedUiState)
    }
}

private func previewSourceSubtitle(_ source: VesperPlayerSource) -> String {
    switch source.kind {
    case .local:
        return VesperPlayerI18n.previewLocalSourceSubtitle()
    case .remote:
        return VesperPlayerI18n.previewRemoteSourceSubtitle(source.protocol.rawValue)
    }
}
