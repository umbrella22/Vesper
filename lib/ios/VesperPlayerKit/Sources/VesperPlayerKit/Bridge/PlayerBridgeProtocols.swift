import AVFoundation
import CoreGraphics
import Foundation
import SwiftUI
import UIKit
internal import VesperPlayerKitBridgeShim
@MainActor
protocol PlayerBridge: AnyObject {
    var backend: PlayerBridgeBackend { get }
    var uiState: PlayerHostUiState { get }
    var trackCatalog: VesperTrackCatalog { get }
    var trackSelection: VesperTrackSelectionSnapshot { get }
    var effectiveVideoTrackId: String? { get }
    var videoVariantObservation: VesperVideoVariantObservation? { get }
    var fixedTrackStatus: VesperFixedTrackStatus? { get }
    var resiliencePolicy: VesperPlaybackResiliencePolicy { get }
    var lastError: VesperPlayerError? { get }
    var pluginDiagnostics: [[String: Any]] { get }
    var routePickerPlayer: AVPlayer? { get }

    func initialize()
    func initializeAsync() async
    func dispose()
    func refresh()
    func selectSource(_ source: VesperPlayerSource)
    func selectSourceAsync(_ source: VesperPlayerSource) async

    func attachSurfaceHost(_ host: UIView)
    func detachSurfaceHost()
    func detachSurfaceHost(_ host: UIView)

    func play()
    func pause()
    func togglePause()
    func stop()
    func seek(by deltaMs: Int64)
    func seek(toRatio ratio: Double)
    func seekToLiveEdge()
    func setPlaybackRate(_ rate: Float)
    func setVideoTrackSelection(_ selection: VesperTrackSelection)
    func setAudioTrackSelection(_ selection: VesperTrackSelection)
    /// Applies a subtitle selection.
    ///
    /// Immediate (synchronous) validation failures — missing legible
    /// group, unknown track id, no auto candidate — `throw` so the iOS
    /// Flutter plugin surfaces them through `FlutterError` and the Dart
    /// `Future<void>` fails. Race failures after the platform `select` still
    /// surface through `publishedSubtitleState` because they cannot be
    /// observed synchronously.
    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) throws
    func setSubtitleStyle(_ style: VesperSubtitleStyle)
    func setAbrPolicy(_ policy: VesperAbrPolicy)
    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy)
    func setAudioSessionInterrupted(_ interrupted: Bool)
    func drainBenchmarkEvents() -> [VesperBenchmarkEvent]
    func benchmarkSummary() -> VesperBenchmarkSummary
}

@MainActor
protocol ObservablePlayerBridge: PlayerBridge, ObservableObject {
    var publishedUiState: PlayerHostUiState { get }
    var publishedTrackCatalog: VesperTrackCatalog { get }
    var publishedTrackSelection: VesperTrackSelectionSnapshot { get }
    var publishedEffectiveVideoTrackId: String? { get }
    var publishedVideoVariantObservation: VesperVideoVariantObservation? { get }
    var publishedFixedTrackStatus: VesperFixedTrackStatus? { get }
    var publishedResiliencePolicy: VesperPlaybackResiliencePolicy { get }
    var publishedLastError: VesperPlayerError? { get }
    var publishedSubtitleState: VesperSubtitleState { get }
}

extension PlayerBridge {
    var routePickerPlayer: AVPlayer? {
        nil
    }

    func initializeAsync() async {
        initialize()
    }

    func selectSourceAsync(_ source: VesperPlayerSource) async {
        selectSource(source)
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
