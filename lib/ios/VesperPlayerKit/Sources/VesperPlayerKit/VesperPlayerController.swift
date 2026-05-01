import Combine
import Foundation
import UIKit

@MainActor
public final class VesperPlayerController: ObservableObject {
    public let backend: PlayerBridgeBackend

    @Published private(set) var publishedUiState: PlayerHostUiState
    @Published private(set) var publishedTrackCatalog: VesperTrackCatalog
    @Published private(set) var publishedTrackSelection: VesperTrackSelectionSnapshot
    @Published private(set) var publishedEffectiveVideoTrackId: String?
    @Published private(set) var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published private(set) var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published private(set) var publishedResiliencePolicy: VesperPlaybackResiliencePolicy
    @Published private(set) var publishedLastError: VesperPlayerError?

    public var uiState: PlayerHostUiState {
        publishedUiState
    }

    /// The latest media track catalog reported by the active source.
    public var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    /// The currently applied track-selection intent for the active source.
    public var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
    }

    /// The best-effort video variant currently rendered by the backend.
    ///
    /// On iOS this is inferred from the current HLS variant ladder, playback
    /// access logs, and presentation size. It may be `nil` until the player has
    /// enough runtime information to identify a matching variant.
    public var effectiveVideoTrackId: String? {
        publishedEffectiveVideoTrackId
    }

    /// The raw runtime video-variant evidence currently observed by the host.
    ///
    /// On iOS this is derived from AVPlayer access logs plus presentation size.
    /// The value may be `nil` until playback produces enough runtime evidence.
    public var videoVariantObservation: VesperVideoVariantObservation? {
        publishedVideoVariantObservation
    }

    /// The latest best-effort status for the active `fixedTrack` ABR request.
    ///
    /// This value is `nil` when no fixed-track request is active. On iOS the
    /// status is derived from the current HLS variant ladder plus playback
    /// runtime evidence, so `.pending` means the host is still waiting for
    /// enough evidence to identify the active variant.
    public var fixedTrackStatus: VesperFixedTrackStatus? {
        publishedFixedTrackStatus
    }

    public var resiliencePolicy: VesperPlaybackResiliencePolicy {
        publishedResiliencePolicy
    }

    public var lastError: VesperPlayerError? {
        publishedLastError
    }

    private var bridgeObservation: AnyCancellable?
    private let initializeImpl: () -> Void
    private let disposeImpl: () -> Void
    private let refreshImpl: () -> Void
    private let selectSourceImpl: (VesperPlayerSource) -> Void
    private let attachSurfaceHostImpl: (UIView) -> Void
    private let detachSurfaceHostImpl: () -> Void
    private let playImpl: () -> Void
    private let pauseImpl: () -> Void
    private let togglePauseImpl: () -> Void
    private let stopImpl: () -> Void
    private let seekByImpl: (Int64) -> Void
    private let seekToRatioImpl: (Double) -> Void
    private let seekToLiveEdgeImpl: () -> Void
    private let setPlaybackRateImpl: (Float) -> Void
    private let setVideoTrackSelectionImpl: (VesperTrackSelection) -> Void
    private let setAudioTrackSelectionImpl: (VesperTrackSelection) -> Void
    private let setSubtitleTrackSelectionImpl: (VesperTrackSelection) -> Void
    private let setAbrPolicyImpl: (VesperAbrPolicy) -> Void
    private let setResiliencePolicyImpl: (VesperPlaybackResiliencePolicy) -> Void
    private let drainBenchmarkEventsImpl: () -> [VesperBenchmarkEvent]
    private let benchmarkSummaryImpl: () -> VesperBenchmarkSummary

    init<Bridge: ObservablePlayerBridge>(_ bridge: Bridge) {
        backend = bridge.backend
        publishedUiState = bridge.publishedUiState
        publishedTrackCatalog = bridge.publishedTrackCatalog
        publishedTrackSelection = bridge.publishedTrackSelection
        publishedEffectiveVideoTrackId = bridge.publishedEffectiveVideoTrackId
        publishedVideoVariantObservation = bridge.publishedVideoVariantObservation
        publishedFixedTrackStatus = bridge.publishedFixedTrackStatus
        publishedResiliencePolicy = bridge.publishedResiliencePolicy
        publishedLastError = bridge.publishedLastError
        initializeImpl = bridge.initialize
        disposeImpl = bridge.dispose
        refreshImpl = bridge.refresh
        selectSourceImpl = bridge.selectSource
        attachSurfaceHostImpl = { host in
            bridge.attachSurfaceHost(host)
        }
        detachSurfaceHostImpl = bridge.detachSurfaceHost
        playImpl = bridge.play
        pauseImpl = bridge.pause
        togglePauseImpl = bridge.togglePause
        stopImpl = bridge.stop
        seekByImpl = { deltaMs in
            bridge.seek(by: deltaMs)
        }
        seekToRatioImpl = { ratio in
            bridge.seek(toRatio: ratio)
        }
        seekToLiveEdgeImpl = bridge.seekToLiveEdge
        setPlaybackRateImpl = bridge.setPlaybackRate
        setVideoTrackSelectionImpl = bridge.setVideoTrackSelection
        setAudioTrackSelectionImpl = bridge.setAudioTrackSelection
        setSubtitleTrackSelectionImpl = bridge.setSubtitleTrackSelection
        setAbrPolicyImpl = bridge.setAbrPolicy
        setResiliencePolicyImpl = bridge.setResiliencePolicy
        drainBenchmarkEventsImpl = bridge.drainBenchmarkEvents
        benchmarkSummaryImpl = bridge.benchmarkSummary
        bridgeObservation = bridge.objectWillChange.sink { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.publishedUiState = bridge.publishedUiState
                self.publishedTrackCatalog = bridge.publishedTrackCatalog
                self.publishedTrackSelection = bridge.publishedTrackSelection
                self.publishedEffectiveVideoTrackId = bridge.publishedEffectiveVideoTrackId
                self.publishedVideoVariantObservation = bridge.publishedVideoVariantObservation
                self.publishedFixedTrackStatus = bridge.publishedFixedTrackStatus
                self.publishedResiliencePolicy = bridge.publishedResiliencePolicy
                self.publishedLastError = bridge.publishedLastError
            }
        }
    }

    public func initialize() {
        initializeImpl()
    }

    public func dispose() {
        disposeImpl()
    }

    public func refresh() {
        refreshImpl()
    }

    public func selectSource(_ source: VesperPlayerSource) {
        selectSourceImpl(source)
    }

    public func attachSurfaceHost(_ host: UIView) {
        attachSurfaceHostImpl(host)
    }

    public func detachSurfaceHost() {
        detachSurfaceHostImpl()
    }

    public func play() {
        playImpl()
    }

    public func pause() {
        pauseImpl()
    }

    public func togglePause() {
        togglePauseImpl()
    }

    public func stop() {
        stopImpl()
    }

    public func seek(by deltaMs: Int64) {
        seekByImpl(deltaMs)
    }

    public func seek(toRatio ratio: Double) {
        seekToRatioImpl(ratio)
    }

    public func seekToLiveEdge() {
        seekToLiveEdgeImpl()
    }

    public func setPlaybackRate(_ rate: Float) {
        setPlaybackRateImpl(rate)
    }

    public func setVideoTrackSelection(_ selection: VesperTrackSelection) {
        setVideoTrackSelectionImpl(selection)
    }

    public func setAudioTrackSelection(_ selection: VesperTrackSelection) {
        setAudioTrackSelectionImpl(selection)
    }

    public func setSubtitleTrackSelection(_ selection: VesperTrackSelection) {
        setSubtitleTrackSelectionImpl(selection)
    }

    /// Applies adaptive bitrate behavior for the active source.
    ///
    /// On iOS, `fixedTrack` maps to best-effort HLS variant pinning. Single-axis
    /// constrained resolution requests also wait for the current HLS variant
    /// catalog before the missing dimension can be inferred.
    public func setAbrPolicy(_ policy: VesperAbrPolicy) {
        setAbrPolicyImpl(policy)
    }

    public func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        setResiliencePolicyImpl(policy)
    }

    public func drainBenchmarkEvents() -> [VesperBenchmarkEvent] {
        drainBenchmarkEventsImpl()
    }

    public func benchmarkSummary() -> VesperBenchmarkSummary {
        benchmarkSummaryImpl()
    }

    /// Playback rates exposed by the current iOS host surface.
    public static let supportedPlaybackRates: [Float] = [0.5, 1.0, 1.5, 2.0, 3.0]
}
