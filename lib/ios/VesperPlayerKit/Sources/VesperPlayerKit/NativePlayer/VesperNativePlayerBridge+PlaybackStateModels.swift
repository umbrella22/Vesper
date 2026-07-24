@preconcurrency import AVFoundation
import Foundation
import os

enum VesperPlaybackTrace {
    private static let signposter = OSSignposter(
        subsystem: "io.github.ikaros.vesper.player.ios",
        category: "Playback"
    )

    static func interval<Result>(
        _ name: StaticString,
        operation: () throws -> Result
    ) rethrows -> Result {
        let state = signposter.beginInterval(name)
        defer { signposter.endInterval(name, state) }
        return try operation()
    }
}

struct ResolvedBufferingPolicy {
    let preferredForwardBufferDuration: TimeInterval
    let automaticallyWaitsToMinimizeStalling: Bool
}

struct ResolvedCachePolicy {
    let enabled: Bool
    let memoryCapacity: Int
    let diskCapacity: Int

    static let disabled = ResolvedCachePolicy(
        enabled: false,
        memoryCapacity: 0,
        diskCapacity: 0
    )
}

enum PendingResilienceRestorePhase {
    case coreState
    case trackSelection
}

struct PendingResilienceRestore {
    let sourceUri: String
    let state: PreservedPlaybackState
    var needsCoreStateRestore = true
    var needsTrackSelectionRestore = true
}

struct StopSeekStateSnapshot: Equatable {
    let isSeekingToStartAfterStop: Bool
    let pendingPlayAfterStopSeek: Bool
}

struct PreservedPlaybackState {
    let positionMs: Int64
    let restorePosition: Bool
    let seekToLiveEdge: Bool
    let playbackRate: Float
    let playbackState: PlaybackStateUi
    let shouldResumePlayback: Bool
    let audioSelection: VesperTrackSelection
    let subtitleSelection: VesperTrackSelection
    let abrPolicy: VesperAbrPolicy

    static func capture(
        uiState: PlayerHostUiState,
        trackSelection: VesperTrackSelectionSnapshot,
        confirmedSubtitleSelection: VesperTrackSelection
    ) -> PreservedPlaybackState {
        let seekToLiveEdge =
            uiState.timeline.kind == .liveDvr &&
                uiState.timeline.isAtLiveEdge()
        return PreservedPlaybackState(
            positionMs: uiState.timeline.positionMs,
            restorePosition: uiState.timeline.isSeekable || uiState.timeline.durationMs != nil,
            seekToLiveEdge: seekToLiveEdge,
            playbackRate: uiState.playbackRate,
            playbackState: uiState.playbackState,
            shouldResumePlayback: uiState.playbackState == .playing,
            audioSelection: trackSelection.audio,
            subtitleSelection: confirmedSubtitleSelection,
            abrPolicy: trackSelection.abrPolicy
        )
    }
}

func derivePlaybackState(
    currentState: PlaybackStateUi,
    player: AVPlayer,
    durationMs: Int64?,
    positionMs: Int64,
) -> PlaybackStateUi {
    if currentState == .finished {
        return .finished
    }

    if player.rate > 0 || player.timeControlStatus == .playing {
        return .playing
    }

    if let durationMs, durationMs > 0, positionMs >= durationMs {
        return .finished
    }

    if positionMs > 0 {
        return .paused
    }

    return .ready
}

func normalizedSeekableRange(durationMs: Int64?) -> SeekableRangeUi {
    SeekableRangeUi(startMs: 0, endMs: max(durationMs ?? 0, 0))
}

func iosHostLog(_ message: String) {
    print("[VesperPlayerIOSHost] \(message)")
}

func clampToInt(_ value: Int64) -> Int {
    guard value > 0 else {
        return 0
    }
    return Int(min(value, Int64(Int.max)))
}
