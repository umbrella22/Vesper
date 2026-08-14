@preconcurrency import AVFoundation
import Foundation

private enum VesperSystemPlayerCommandReadiness {
    case waiting
    case ready(TimelineKindUi, isConfirmedLive: Bool)
    case failed(Error)
}

extension VesperNativePlayerBridge {
    func awaitNativeFrameSurfaceHost(
        source: VesperPlayerSource,
        sourceLoadEpoch: UInt64,
        deadline: ContinuousClock.Instant
    ) async throws {
        let clock = ContinuousClock()

        while surfaceHost == nil {
            try Task.checkCancellation()
            guard isCurrentSourceLoad(sourceLoadEpoch, source: source),
                  let command = activeSourceCommand
            else {
                let commandId = activeSourceCommand?.commandId ?? sourceCommandGeneration
                throw obsoleteVesperCommandError(
                    message: "iOS source command was superseded while waiting for a video surface.",
                    category: .source,
                    reason: "sourceCommandSuperseded",
                    commandId: commandId,
                    sourceEpoch: commandId
                )
            }
            try ensureCurrentSourceCommand(command)
            guard clock.now < deadline else {
                throw sourceCommandTimeoutError(command)
            }
            let remaining = clock.now.duration(to: deadline)
            try await clock.sleep(for: min(sourceReadinessWaitPolicy.pollInterval, remaining))
        }

        pendingNativeFrameSurfaceLoad = false
    }

    func awaitSystemPlayerCommandReadiness(
        source: VesperPlayerSource,
        sourceLoadEpoch: UInt64,
        player: AVPlayer,
        item: AVPlayerItem,
        playbackEpoch: UInt64,
        deadline: ContinuousClock.Instant
    ) async throws {
        let clock = ContinuousClock()

        while true {
            try Task.checkCancellation()
            try ensureCurrentSourceLoad(
                sourceLoadEpoch,
                source: source,
                player: player,
                item: item,
                playbackEpoch: playbackEpoch
            )
            if let observedFailure = pendingSourceCommandFailure {
                pendingSourceCommandFailure = nil
                throw observedFailure
            }

            switch await systemPlayerCommandReadiness(source: source, item: item) {
            case .ready(let kind, let isConfirmedLive):
                currentSourceIsConfirmedLive = isConfirmedLive
                let timeline = currentTimelineState(kindOverride: kind)
                updateState {
                    PlayerHostUiState(
                        title: $0.title,
                        subtitle: $0.subtitle,
                        sourceLabel: $0.sourceLabel,
                        playbackState: $0.playbackState,
                        playbackRate: $0.playbackRate,
                        isBuffering: false,
                        isInterrupted: $0.isInterrupted,
                        timeline: timeline
                    )
                }
                recordBenchmark(
                    "source_command_ready",
                    attributes: [
                        "itemStatus": itemStatusName(item.status),
                        "playbackEpoch": "\(playbackEpoch)",
                        "timelineKind": kind.rawValue,
                    ]
                )
                return
            case .failed(let error):
                throw error
            case .waiting:
                break
            }

            guard clock.now < deadline else {
                throw vesperCommandError(
                    message: "iOS player item did not become ready for source commands before the deadline.",
                    code: .timeout,
                    category: .source,
                    retriable: source.kind == .remote,
                    reason: "sourceCommandReadinessTimeout",
                    commandId: activeSourceCommand?.commandId ?? sourceCommandGeneration,
                    sourceEpoch: activeSourceCommand?.commandId ?? sourceCommandGeneration,
                    details: [
                        "protocol": source.protocol.rawValue,
                        "itemStatus": itemStatusName(item.status),
                    ]
                )
            }
            let remaining = clock.now.duration(to: deadline)
            try await clock.sleep(for: min(sourceReadinessWaitPolicy.pollInterval, remaining))
        }
    }

    private func ensureCurrentSourceLoad(
        _ sourceLoadEpoch: UInt64,
        source: VesperPlayerSource,
        player: AVPlayer,
        item: AVPlayerItem,
        playbackEpoch: UInt64
    ) throws {
        guard isCurrentSourceLoad(sourceLoadEpoch, source: source),
              isPlaybackEpochCurrent(playbackEpoch),
              self.player === player,
              player.currentItem === item
        else {
            let commandId = activeSourceCommand?.commandId ?? sourceCommandGeneration
            throw obsoleteVesperCommandError(
                message: "iOS source command was superseded while waiting for readiness.",
                category: .source,
                reason: "sourceCommandSuperseded",
                commandId: commandId,
                sourceEpoch: commandId
            )
        }
    }

    private func systemPlayerCommandReadiness(
        source: VesperPlayerSource,
        item: AVPlayerItem
    ) async -> VesperSystemPlayerCommandReadiness {
        switch item.status {
        case .failed:
            return .failed(
                item.error ?? VesperPlayerError(
                    message: "iOS player item failed before source commands became available.",
                    code: .invalidSource,
                    category: .source,
                    retriable: source.kind == .remote,
                    details: [
                        "reason": "sourceCommandReadinessFailed",
                        "itemStatus": itemStatusName(item.status),
                    ]
                )
            )
        case .unknown:
            return .waiting
        case .readyToPlay:
            break
        @unknown default:
            return .waiting
        }

        let durationMs = item.duration.finiteMilliseconds
        let seekableRange = currentSeekableRange(item: item, durationMs: durationMs)
        let isConfirmedLive = await confirmedLiveEvidence(for: source, item: item)
        switch sourceTimelineReadiness(
            durationMs: durationMs,
            hasIndefiniteDuration: item.duration.isIndefinite,
            seekableRange: seekableRange,
            isConfirmedLive: isConfirmedLive
        ) {
        case .waiting:
            return .waiting
        case .ready(let kind):
            return .ready(kind, isConfirmedLive: isConfirmedLive)
        }
    }

    private func confirmedLiveEvidence(
        for source: VesperPlayerSource,
        item: AVPlayerItem
    ) async -> Bool {
        if source.protocol == .dash {
            return await currentDashSession?.manifestTypeSnapshot() == .dynamic
        }
        guard source.protocol == .hls else {
            return false
        }

        switch item.asset.status(of: .duration) {
        case .loaded(let duration):
            return duration.isIndefinite
        case .notYetLoaded, .loading, .failed:
            return false
        }
    }
}
