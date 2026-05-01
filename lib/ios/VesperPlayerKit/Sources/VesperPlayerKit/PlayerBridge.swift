import CoreGraphics
import Foundation
import SwiftUI
import UIKit

public enum PlayerBridgeBackend: String {
    case fakeDemo = "fake_demo"
    case rustNativeStub = "rust_native_stub"
}

public enum TimelineKindUi: String {
    case vod = "vod"
    case live = "live"
    case liveDvr = "live_dvr"
}

public struct SeekableRangeUi {
    public let startMs: Int64
    public let endMs: Int64

    public init(startMs: Int64, endMs: Int64) {
        self.startMs = startMs
        self.endMs = endMs
    }
}

public struct TimelineUiState {
    public let kind: TimelineKindUi
    public let isSeekable: Bool
    public let seekableRange: SeekableRangeUi?
    public let liveEdgeMs: Int64?
    public let positionMs: Int64
    public let durationMs: Int64?

    public init(
        kind: TimelineKindUi,
        isSeekable: Bool,
        seekableRange: SeekableRangeUi?,
        liveEdgeMs: Int64?,
        positionMs: Int64,
        durationMs: Int64?
    ) {
        self.kind = kind
        self.isSeekable = isSeekable
        self.seekableRange = seekableRange
        self.liveEdgeMs = liveEdgeMs
        self.positionMs = positionMs
        self.durationMs = durationMs
    }

    public var displayedRatio: Double? {
        if let range = seekableRange, range.endMs > range.startMs {
            let clamped = min(max(positionMs, range.startMs), range.endMs)
            let width = Double(range.endMs - range.startMs)
            if width <= 0 {
                return nil
            }
            return min(max(Double(clamped - range.startMs) / width, 0.0), 1.0)
        }

        guard let durationMs, durationMs > 0 else {
            return nil
        }

        return min(max(Double(positionMs) / Double(durationMs), 0.0), 1.0)
    }

    public var goLivePositionMs: Int64? {
        switch kind {
        case .vod:
            nil
        case .live:
            liveEdgeMs
        case .liveDvr:
            liveEdgeMs ?? seekableRange?.endMs
        }
    }

    public var liveOffsetMs: Int64? {
        guard let liveEdgeMs = goLivePositionMs else {
            return nil
        }

        return max(liveEdgeMs - clampedPosition(positionMs), 0)
    }

    public func clampedPosition(_ positionMs: Int64) -> Int64 {
        if let range = seekableRange, range.endMs >= range.startMs {
            return min(max(positionMs, range.startMs), range.endMs)
        }

        guard let durationMs else {
            return max(positionMs, 0)
        }

        return min(max(positionMs, 0), max(durationMs, 0))
    }

    public func position(forRatio ratio: Double) -> Int64 {
        let normalized = min(max(ratio, 0.0), 1.0)
        if let range = seekableRange, range.endMs >= range.startMs {
            let width = Double(range.endMs - range.startMs)
            return clampedPosition(range.startMs + Int64(width * normalized))
        }

        return clampedPosition(Int64(Double(durationMs ?? 0) * normalized))
    }

    public func isAtLiveEdge(toleranceMs: Int64 = 1_500) -> Bool {
        guard let liveEdgeMs = goLivePositionMs else {
            return false
        }

        return abs(liveEdgeMs - clampedPosition(positionMs)) <= max(toleranceMs, 0)
    }
}

public enum PlaybackStateUi: String {
    case ready = "Ready"
    case playing = "Playing"
    case paused = "Paused"
    case finished = "Finished"
}

public struct PlayerHostUiState {
    public let title: String
    public let subtitle: String
    public let sourceLabel: String
    public let playbackState: PlaybackStateUi
    public let playbackRate: Float
    public let isBuffering: Bool
    public let isInterrupted: Bool
    public let timeline: TimelineUiState

    public init(
        title: String,
        subtitle: String,
        sourceLabel: String,
        playbackState: PlaybackStateUi,
        playbackRate: Float,
        isBuffering: Bool,
        isInterrupted: Bool,
        timeline: TimelineUiState
    ) {
        self.title = title
        self.subtitle = subtitle
        self.sourceLabel = sourceLabel
        self.playbackState = playbackState
        self.playbackRate = playbackRate
        self.isBuffering = isBuffering
        self.isInterrupted = isInterrupted
        self.timeline = timeline
    }
}

public enum VesperPlayerErrorCategory: String, Equatable {
    case input
    case source
    case network
    case decode
    case audioOutput
    case playback
    case capability
    case unsupported
    case platform
}

public struct VesperPlayerError: Equatable {
    public let message: String
    public let category: VesperPlayerErrorCategory
    public let retriable: Bool

    public init(message: String, category: VesperPlayerErrorCategory, retriable: Bool) {
        self.message = message
        self.category = category
        self.retriable = retriable
    }
}

/// Describes the raw runtime evidence currently observed for the active video
/// variant.
public struct VesperVideoVariantObservation: Equatable {
    public let bitRate: Int64?
    public let width: Int?
    public let height: Int?

    public init(
        bitRate: Int64? = nil,
        width: Int? = nil,
        height: Int? = nil
    ) {
        self.bitRate = bitRate
        self.width = width
        self.height = height
    }
}

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

    func initialize()
    func dispose()
    func refresh()
    func selectSource(_ source: VesperPlayerSource)

    func attachSurfaceHost(_ host: UIView)
    func detachSurfaceHost()

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
    func setSubtitleTrackSelection(_ selection: VesperTrackSelection)
    func setAbrPolicy(_ policy: VesperAbrPolicy)
    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy)
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
}

extension PlayerBridge {
    var isPlaying: Bool {
        uiState.playbackState == .playing
    }
}
