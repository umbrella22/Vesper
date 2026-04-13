import Combine
import Foundation
import UIKit

@MainActor
public final class VesperPlayerController: ObservableObject {
    public let backend: PlayerBridgeBackend

    @Published private(set) var publishedUiState: PlayerHostUiState
    @Published private(set) var publishedTrackCatalog: VesperTrackCatalog
    @Published private(set) var publishedTrackSelection: VesperTrackSelectionSnapshot

    public var uiState: PlayerHostUiState {
        publishedUiState
    }

    public var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    public var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
    }

    private var bridgeObservation: AnyCancellable?
    private let initializeImpl: () -> Void
    private let disposeImpl: () -> Void
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

    init<Bridge: ObservablePlayerBridge>(_ bridge: Bridge) {
        backend = bridge.backend
        publishedUiState = bridge.publishedUiState
        publishedTrackCatalog = bridge.publishedTrackCatalog
        publishedTrackSelection = bridge.publishedTrackSelection
        initializeImpl = bridge.initialize
        disposeImpl = bridge.dispose
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
        bridgeObservation = bridge.objectWillChange.sink { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.publishedUiState = bridge.publishedUiState
                self.publishedTrackCatalog = bridge.publishedTrackCatalog
                self.publishedTrackSelection = bridge.publishedTrackSelection
            }
        }
    }

    public func initialize() {
        initializeImpl()
    }

    public func dispose() {
        disposeImpl()
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

    public func setAbrPolicy(_ policy: VesperAbrPolicy) {
        setAbrPolicyImpl(policy)
    }

    public func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        setResiliencePolicyImpl(policy)
    }

    public static let supportedPlaybackRates: [Float] = [0.5, 1.0, 1.5, 2.0, 3.0]
}
