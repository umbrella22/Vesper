@preconcurrency import AVFoundation
import Foundation

struct VesperSourceReadinessWaitPolicy: Equatable, Sendable {
    let timeout: Duration
    let pollInterval: Duration

    static let production = VesperSourceReadinessWaitPolicy(
        timeout: .seconds(30),
        pollInterval: .milliseconds(50)
    )
}

struct VesperSeekCommandWaitPolicy: Equatable, Sendable {
    let timeout: Duration

    static let production = VesperSeekCommandWaitPolicy(timeout: .seconds(15))
}

enum VesperSourceTimelineReadiness: Equatable {
    case waiting
    case ready(TimelineKindUi)
}

func sourceTimelineReadiness(
    durationMs: Int64?,
    hasIndefiniteDuration: Bool,
    seekableRange: SeekableRangeUi?,
    isConfirmedLive: Bool
) -> VesperSourceTimelineReadiness {
    let hasSeekableWindow = seekableRange.map { $0.endMs > $0.startMs } ?? false
    if isConfirmedLive {
        return .ready(hasSeekableWindow ? .liveDvr : .live)
    }
    if let durationMs, durationMs > 0 {
        return .ready(.vod)
    }
    // Indefinite is only a representation of an unresolved duration until the
    // platform has independent live evidence.
    _ = hasIndefiniteDuration
    return .waiting
}

@MainActor
final class VesperSourceCommandHandle {
    let commandId: UInt64
    let source: VesperPlayerSource
    let deadline: ContinuousClock.Instant
    var cancellationReason: String?
    var retryAttemptCount = 0
    var task: Task<Void, Error>?

    init(
        commandId: UInt64,
        source: VesperPlayerSource,
        deadline: ContinuousClock.Instant
    ) {
        self.commandId = commandId
        self.source = source
        self.deadline = deadline
    }
}

@MainActor
final class VesperSeekCommandHandle {
    let commandId: UInt64
    let sourceEpoch: UInt64
    let source: VesperPlayerSource
    let playbackEpoch: UInt64
    let targetMs: Int64
    weak var player: AVPlayer?
    weak var nativeFrameSession: VesperNativeFramePipelineSession?
    var continuation: CheckedContinuation<Void, Error>?
    var timeoutTask: Task<Void, Never>?
    private(set) var isSettled = false

    init(
        commandId: UInt64,
        sourceEpoch: UInt64,
        source: VesperPlayerSource,
        playbackEpoch: UInt64,
        targetMs: Int64,
        player: AVPlayer?,
        nativeFrameSession: VesperNativeFramePipelineSession?
    ) {
        self.commandId = commandId
        self.sourceEpoch = sourceEpoch
        self.source = source
        self.playbackEpoch = playbackEpoch
        self.targetMs = targetMs
        self.player = player
        self.nativeFrameSession = nativeFrameSession
    }

    func succeed() {
        guard !isSettled else { return }
        isSettled = true
        timeoutTask?.cancel()
        timeoutTask = nil
        continuation?.resume()
        continuation = nil
    }

    func fail(_ error: Error) {
        guard !isSettled else { return }
        isSettled = true
        timeoutTask?.cancel()
        timeoutTask = nil
        continuation?.resume(throwing: error)
        continuation = nil
    }
}

typealias VesperSystemPlayerSeekSubmitter = @MainActor (
    _ player: AVPlayer,
    _ target: CMTime,
    _ toleranceBefore: CMTime,
    _ toleranceAfter: CMTime,
    _ completion: @escaping @Sendable (Bool) -> Void
) -> Void

typealias VesperSourceLoadAttemptOverride = @MainActor (
    _ bridge: VesperNativePlayerBridge,
    _ source: VesperPlayerSource,
    _ sourceLoadEpoch: UInt64,
    _ deadline: ContinuousClock.Instant
) async throws -> Void

func vesperCommandError(
    message: String,
    code: VesperPlayerErrorCode,
    category: VesperPlayerErrorCategory,
    retriable: Bool,
    reason: String,
    commandId: UInt64,
    sourceEpoch: UInt64,
    obsolete: Bool = false,
    details: [String: String] = [:]
) -> VesperPlayerError {
    var commandDetails = details
    commandDetails["commandReason"] = reason
    if commandDetails["reason"] == nil {
        commandDetails["reason"] = reason
    }
    commandDetails["commandId"] = String(commandId)
    commandDetails["sourceEpoch"] = String(sourceEpoch)
    if obsolete {
        commandDetails["obsolete"] = "true"
    }
    return VesperPlayerError(
        message: message,
        code: code,
        category: category,
        retriable: retriable,
        details: commandDetails
    )
}

func obsoleteVesperCommandError(
    message: String,
    category: VesperPlayerErrorCategory,
    reason: String,
    commandId: UInt64,
    sourceEpoch: UInt64
) -> VesperPlayerError {
    vesperCommandError(
        message: message,
        code: .cancelled,
        category: category,
        retriable: true,
        reason: reason,
        commandId: commandId,
        sourceEpoch: sourceEpoch,
        obsolete: true
    )
}

extension VesperNativePlayerBridge {
    func cancelPendingSeekCommand(reason: String) {
        guard let command = activeSeekCommand else { return }
        activeSeekCommand = nil
        command.player?.currentItem?.cancelPendingSeeks()
        command.fail(
            obsoleteVesperCommandError(
                message: "iOS seek command was superseded before it completed.",
                category: .playback,
                reason: reason,
                commandId: command.commandId,
                sourceEpoch: command.sourceEpoch
            )
        )
    }

    func executeSeekCommand(to positionMs: Int64) async throws {
        try Task.checkCancellation()
        cancelPendingSeekCommand(reason: "seekCommandSuperseded")
        seekCommandGeneration &+= 1
        if seekCommandGeneration == 0 {
            seekCommandGeneration = 1
        }

        let commandId = seekCommandGeneration
        let sourceEpoch = sourceCommandGeneration
        guard let source = currentSource,
              activeSourceCommand == nil
        else {
            let error = seekCommandError(
                message: "iOS playback is not ready for seek.",
                code: .invalidState,
                reason: "seekCommandNotReady",
                commandId: commandId,
                sourceEpoch: sourceEpoch
            )
            publishSeekCommandFailure(error)
            throw error
        }

        let systemPlayer = player
        let nativeFrameSession = nativeFramePipelineCoordinator.activeSession
        guard systemPlayer != nil || nativeFrameSession?.didStart == true else {
            let error = seekCommandError(
                message: "iOS playback route is unavailable for seek.",
                code: .invalidState,
                reason: "seekRouteUnavailable",
                commandId: commandId,
                sourceEpoch: sourceEpoch
            )
            publishSeekCommandFailure(error)
            throw error
        }

        let command = VesperSeekCommandHandle(
            commandId: commandId,
            sourceEpoch: sourceEpoch,
            source: source,
            playbackEpoch: currentPlaybackEpoch(),
            targetMs: max(positionMs, 0),
            player: systemPlayer,
            nativeFrameSession: nativeFrameSession
        )

        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                command.continuation = continuation
                activeSeekCommand = command
                scheduleSeekCommandTimeout(command)

                if Task.isCancelled {
                    cancelSeekCommandIfCurrent(
                        command,
                        reason: "seekCommandCancelled"
                    )
                    return
                }

                recordBenchmark(
                    "seek_start",
                    attributes: ["positionMs": "\(command.targetMs)"]
                )
                if let systemPlayer {
                    systemPlayerSeekSubmitter(
                        systemPlayer,
                        CMTime(milliseconds: command.targetMs),
                        .zero,
                        .zero
                    ) { [weak self, weak command] finished in
                        Task { @MainActor in
                            guard let self, let command else { return }
                            self.completeSeekCommand(command, finished: finished)
                        }
                    }
                    return
                }

                guard let nativeFrameSession else {
                    completeSeekCommand(command, finished: false)
                    return
                }
                let submitted = nativeFrameSession.seek(toMs: command.targetMs) {
                    [weak self, weak command] finished in
                    guard let self, let command else { return }
                    self.completeSeekCommand(command, finished: finished)
                }
                if !submitted {
                    completeSeekCommand(command, finished: false)
                }
            }
        } onCancel: { [weak self, weak command] in
            Task { @MainActor in
                guard let self, let command else { return }
                self.cancelSeekCommandIfCurrent(
                    command,
                    reason: "seekCommandCancelled"
                )
            }
        }
    }

    private func scheduleSeekCommandTimeout(_ command: VesperSeekCommandHandle) {
        command.timeoutTask = Task { @MainActor [weak self, weak command] in
            guard let self, let command else { return }
            do {
                try await Task.sleep(for: self.seekCommandWaitPolicy.timeout)
            } catch {
                return
            }
            guard self.activeSeekCommand === command else { return }
            let error = self.seekCommandError(
                message: "iOS seek did not complete before the deadline.",
                code: .timeout,
                reason: "seekCommandTimeout",
                commandId: command.commandId,
                sourceEpoch: command.sourceEpoch,
                retriable: true
            )
            self.failSeekCommand(command, error: error, publish: true)
        }
    }

    private func completeSeekCommand(
        _ command: VesperSeekCommandHandle,
        finished: Bool
    ) {
        guard activeSeekCommand === command else { return }
        guard seekCommandGeneration == command.commandId else {
            cancelSeekCommandIfCurrent(command, reason: "seekCommandSuperseded")
            return
        }
        guard sourceCommandGeneration == command.sourceEpoch,
              currentSource == command.source
        else {
            cancelSeekCommandIfCurrent(command, reason: "seekSourceChanged")
            return
        }
        guard isPlaybackEpochCurrent(command.playbackEpoch) else {
            cancelSeekCommandIfCurrent(command, reason: "seekPlaybackChanged")
            return
        }
        if let commandPlayer = command.player {
            guard player === commandPlayer else {
                cancelSeekCommandIfCurrent(command, reason: "seekSourceChanged")
                return
            }
        } else if let commandSession = command.nativeFrameSession {
            guard nativeFramePipelineCoordinator.activeSession === commandSession else {
                cancelSeekCommandIfCurrent(command, reason: "seekSourceChanged")
                return
            }
        }

        guard finished else {
            let error = seekCommandError(
                message: "iOS playback route did not finish the seek.",
                code: .seekFailure,
                reason: "seekCommandInterrupted",
                commandId: command.commandId,
                sourceEpoch: command.sourceEpoch
            )
            failSeekCommand(command, error: error, publish: true)
            return
        }

        activeSeekCommand = nil
        handleSeekCompletion(
            positionMs: command.targetMs,
            playbackEpoch: command.playbackEpoch
        )
        command.succeed()
    }

    private func cancelSeekCommandIfCurrent(
        _ command: VesperSeekCommandHandle,
        reason: String
    ) {
        guard activeSeekCommand === command else { return }
        activeSeekCommand = nil
        command.player?.currentItem?.cancelPendingSeeks()
        command.fail(
            obsoleteVesperCommandError(
                message: "iOS seek command is no longer current.",
                category: .playback,
                reason: reason,
                commandId: command.commandId,
                sourceEpoch: command.sourceEpoch
            )
        )
    }

    private func failSeekCommand(
        _ command: VesperSeekCommandHandle,
        error: VesperPlayerError,
        publish: Bool
    ) {
        guard activeSeekCommand === command else { return }
        activeSeekCommand = nil
        command.player?.currentItem?.cancelPendingSeeks()
        if publish {
            publishSeekCommandFailure(error)
        }
        command.fail(error)
    }

    func seekCommandError(
        message: String,
        code: VesperPlayerErrorCode,
        reason: String,
        commandId: UInt64,
        sourceEpoch: UInt64,
        retriable: Bool = false
    ) -> VesperPlayerError {
        vesperCommandError(
            message: message,
            code: code,
            category: .playback,
            retriable: retriable,
            reason: reason,
            commandId: commandId,
            sourceEpoch: sourceEpoch
        )
    }

    private func publishSeekCommandFailure(_ error: VesperPlayerError) {
        guard error.details["obsolete"] != "true" else { return }
        publishedLastError = error
        recordBenchmark(
            "seek_failed",
            attributes: [
                "code": error.code.rawValue,
                "reason": error.details["reason"] ?? "unknown",
            ]
        )
    }
}
