import AVFoundation
import CoreGraphics
import Foundation
import SwiftUI
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim
@MainActor
protocol PlayerBridge: AnyObject {
    var backend: PlayerBridgeBackend { get }
    var uiState: PlayerHostUiState { get }
    var trackCatalog: VesperTrackCatalog { get }
    var trackSelection: VesperTrackSelectionSnapshot { get }
    var requestedSubtitleSelection: VesperTrackSelection { get }
    var confirmedSubtitleSelection: VesperTrackSelection { get }
    var effectiveVideoTrackId: String? { get }
    var videoVariantObservation: VesperVideoVariantObservation? { get }
    var fixedTrackStatus: VesperFixedTrackStatus? { get }
    var resiliencePolicy: VesperPlaybackResiliencePolicy { get }
    var lastError: VesperPlayerError? { get }
    var pluginDiagnostics: [[String: Any]] { get }
    var routePickerPlayer: AVPlayer? { get }

    func initialize()
    func initializeAsync() async throws
    func dispose()
    func refresh()
    /// Returns a timeline projection without reconciling the complete player state.
    ///
    /// Bridges that do not expose a low-cost sampler return `nil`, allowing the
    /// Flutter platform adapter to use its full-refresh compatibility path.
    func sampleTimeline() -> TimelineUiState?
    /// Consumes the marker for a native periodic update that changed only the
    /// timeline and playback presentation state. Flutter uses this to avoid
    /// serializing a complete snapshot for every native progress tick.
    func consumeTimelineOnlyUpdate() -> Bool
    func selectSource(_ source: VesperPlayerSource)
    func startSourceSelection(_ source: VesperPlayerSource) -> Task<Void, Error>
    func selectSourceAsync(_ source: VesperPlayerSource) async throws

    func attachSurfaceHost(_ host: UIView)
    func detachSurfaceHost()
    func detachSurfaceHost(_ host: UIView)

    func play()
    func pause()
    func togglePause()
    func stop()
    func seek(by deltaMs: Int64)
    func seekAsync(by deltaMs: Int64) async throws
    func seek(toRatio ratio: Double)
    func seekAsync(toRatio ratio: Double) async throws
    func seekToLiveEdge()
    func seekToLiveEdgeAsync() async throws
    func setPlaybackRate(_ rate: Float)
    func setVideoTrackSelection(_ selection: VesperTrackSelection)
    func setAudioTrackSelection(_ selection: VesperTrackSelection)
    /// Applies a subtitle selection and waits for AVPlayer to confirm it.
    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) async throws
    func setSubtitleStyle(_ style: VesperSubtitleStyle)
    func setAbrPolicy(
        _ policy: VesperAbrPolicy,
        expectedCatalogRevision: Int64?
    ) throws
    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy)
    func setAudioSessionInterrupted(_ interrupted: Bool)
    func drainBenchmarkEvents() -> [VesperBenchmarkEvent]
    func drainPipelineEventHookReports() -> VesperPipelineEventHookReportBatch
    func benchmarkSummary() -> VesperBenchmarkSummary
    func awaitBenchmarkSinkShutdown(timeout: TimeInterval) async -> Bool
}

@MainActor
protocol ObservablePlayerBridge: PlayerBridge, ObservableObject {
    var publishedUiState: PlayerHostUiState { get }
    var publishedTrackCatalog: VesperTrackCatalog { get }
    var publishedTrackSelection: VesperTrackSelectionSnapshot { get }
    var publishedRequestedSubtitleSelection: VesperTrackSelection { get }
    var publishedConfirmedSubtitleSelection: VesperTrackSelection { get }
    var publishedEffectiveVideoTrackId: String? { get }
    var publishedVideoVariantObservation: VesperVideoVariantObservation? { get }
    var publishedFixedTrackStatus: VesperFixedTrackStatus? { get }
    var publishedResiliencePolicy: VesperPlaybackResiliencePolicy { get }
    var publishedLastError: VesperPlayerError? { get }
    var publishedSubtitleState: VesperSubtitleState { get }
    var publishedEffectiveSubtitleTrackId: String? { get }
}

extension PlayerBridge {
    /// Backward-compatible envelope for callers that do not carry a catalog
    /// revision. The throwing overload is used by the native command bridge.
    func setAbrPolicy(_ policy: VesperAbrPolicy) {
        try? setAbrPolicy(policy, expectedCatalogRevision: nil)
    }

    func drainPipelineEventHookReports() -> VesperPipelineEventHookReportBatch {
        VesperPipelineEventHookReportBatch()
    }

    func awaitBenchmarkSinkShutdown(timeout: TimeInterval) async -> Bool {
        true
    }

    func sampleTimeline() -> TimelineUiState? {
        nil
    }

    func consumeTimelineOnlyUpdate() -> Bool {
        false
    }

    var routePickerPlayer: AVPlayer? {
        nil
    }

    func initializeAsync() async throws {
        initialize()
    }

    func selectSourceAsync(_ source: VesperPlayerSource) async throws {
        selectSource(source)
    }

    func startSourceSelection(_ source: VesperPlayerSource) -> Task<Void, Error> {
        Task { @MainActor in
            try await selectSourceAsync(source)
        }
    }

    func seekAsync(by deltaMs: Int64) async throws {
        seek(by: deltaMs)
    }

    func seekAsync(toRatio ratio: Double) async throws {
        seek(toRatio: ratio)
    }

    func seekToLiveEdgeAsync() async throws {
        seekToLiveEdge()
    }

    func detachSurfaceHost(_ host: UIView) {
        detachSurfaceHost()
    }
}

extension PlayerBridge {
    var isPlaying: Bool {
        uiState.playbackState == .playing
    }
}
