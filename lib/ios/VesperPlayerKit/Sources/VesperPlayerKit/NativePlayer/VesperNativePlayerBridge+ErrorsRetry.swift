@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

func staleRetryDiagnosticMessage(
    expectedUri: String,
    currentUri: String?,
    attempt: Int
) -> String {
    let expectedDescription = diagnosticURLDescription(expectedUri)
    let currentDescription = diagnosticURLDescription(currentUri)
    return "ignored stale retry task sourceUri=\(expectedDescription) "
        + "currentSource=\(currentDescription) attempt=\(attempt)"
}

extension VesperNativePlayerBridge {
    func sourceCommandTimeoutError(
        _ command: VesperSourceCommandHandle
    ) -> VesperPlayerError {
        vesperCommandError(
            message: "iOS source selection did not become command-ready before the deadline.",
            code: .timeout,
            category: .source,
            retriable: command.source.kind == .remote,
            reason: "sourceCommandTimeout",
            commandId: command.commandId,
            sourceEpoch: command.commandId,
            details: [
                "retryAttempts": "\(command.retryAttemptCount)",
                "sourceProtocol": command.source.protocol.rawValue,
            ]
        )
    }

    func ensureCurrentSourceCommand(
        _ command: VesperSourceCommandHandle
    ) throws {
        guard activeSourceCommand === command, currentSource == command.source else {
            throw obsoleteSourceCommandError(
                command,
                reason: command.cancellationReason ?? "sourceCommandSuperseded"
            )
        }
    }

    func obsoleteSourceCommandError(
        _ command: VesperSourceCommandHandle,
        reason: String
    ) -> VesperPlayerError {
        obsoleteVesperCommandError(
            message: "iOS source selection was superseded before it completed.",
            category: .source,
            reason: reason,
            commandId: command.commandId,
            sourceEpoch: command.commandId
        )
    }

    func resolvedPlaybackFailure(
        error: Error?,
        fallbackMessage: String,
        itemStatusDetails: [String: String] = [:],
        itemErrorLogDetails: [String: String] = [:]
    ) -> ResolvedBridgeError {
        let classifiedError = reclassifyHTTPSourceError(
            classifyPlaybackFailure(error, fallbackMessage: fallbackMessage),
            nativeError: error,
            itemStatusDetails: itemStatusDetails,
            itemErrorLogDetails: itemErrorLogDetails
        )
        return classifiedError
            .enrichedWithDetails(
                sourceFailureDiagnosticDetails(
                    for: classifiedError,
                    nativeError: error
                )
            )
            .enrichedWithDetails(itemStatusDetails)
            .enrichedWithDetails(itemErrorLogDetails)
            .enrichedWithHdrFailureEvidence(currentHdrFailureEvidence)
    }

    func sourceCommandRetryDelay(
        _ error: ResolvedBridgeError,
        command: VesperSourceCommandHandle
    ) -> UInt64? {
        guard error.retriable, command.source.kind == .remote else {
            return nil
        }
        let retryPolicy = currentResiliencePolicy.resolvedForRuntimeSource(command.source).retry
        let nextAttempt = command.retryAttemptCount + 1
        if let maxAttempts = retryPolicy.maxAttempts, nextAttempt > maxAttempts {
            return nil
        }

        let remainingMs = positiveMilliseconds(
            ContinuousClock().now.duration(to: command.deadline)
        )
        guard remainingMs > 0 else {
            return nil
        }
        return min(
            retryDelayMs(forAttempt: nextAttempt, retryPolicy: retryPolicy),
            remainingMs
        )
    }

    func publishSourceCommandRetry(
        _ error: ResolvedBridgeError,
        command: VesperSourceCommandHandle,
        delayMs: UInt64
    ) {
        guard activeSourceCommand === command else { return }
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: VesperPlayerI18n.retryScheduled(
                    delay: formattedRetryDelay(delayMs),
                    message: error.message
                ),
                sourceLabel: $0.sourceLabel,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: true,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
        recordBenchmark(
            "source_command_retry_scheduled",
            attributes: [
                "attempt": "\(command.retryAttemptCount)",
                "category": error.category.rawValue,
                "delayMs": "\(delayMs)",
            ]
        )
    }

    func sourceCommandTerminalError(
        _ error: ResolvedBridgeError,
        command: VesperSourceCommandHandle
    ) -> VesperPlayerError {
        guard ContinuousClock().now < command.deadline else {
            return sourceCommandTimeoutError(command)
        }

        var details = error.details
        details["retryAttempts"] = "\(command.retryAttemptCount)"
        if error.retriable {
            details["attemptsExhausted"] = "true"
            if let maxAttempts = currentResiliencePolicy
                .resolvedForRuntimeSource(command.source)
                .retry.maxAttempts
            {
                details["maxAttempts"] = "\(maxAttempts)"
            }
        }
        return vesperCommandError(
            message: error.message,
            code: error.code,
            category: error.category,
            retriable: error.retriable,
            reason: error.retriable
                ? "sourceCommandRetryExhausted"
                : "sourceCommandFailed",
            commandId: command.commandId,
            sourceEpoch: command.commandId,
            details: details
        )
    }

    func publishSourceCommandFailure(
        _ error: VesperPlayerError,
        command: VesperSourceCommandHandle
    ) {
        guard activeSourceCommand === command else { return }
        pendingAutoPlay = false
        pendingPlaybackStart = false
        player?.pause()
        publishedLastError = error
        updateErrorState(message: error.message)
        recordBenchmark(
            "source_command_failed",
            attributes: [
                "category": error.category.rawValue,
                "code": error.code.rawValue,
                "retriable": "\(error.retriable)",
            ]
        )
    }

    private func positiveMilliseconds(_ duration: Duration) -> UInt64 {
        let components = duration.components
        guard components.seconds >= 0 else { return 0 }
        let millisecondsFromSeconds = UInt64(components.seconds) * 1_000
        let positiveAttoseconds = max(components.attoseconds, 0)
        return millisecondsFromSeconds
            + UInt64(positiveAttoseconds) / 1_000_000_000_000_000
    }

    func sourceSubtitle(for source: VesperPlayerSource) -> String {
        switch source.kind {
        case .local:
            return VesperPlayerI18n.nativeLocalSourceSubtitle()
        case .remote:
            return VesperPlayerI18n.nativeRemoteSourceSubtitle(source.protocol.rawValue)
        }
    }

    func cancelPendingRetry(resetAttempts: Bool) {
        retryTask?.cancel()
        retryTask = nil
        if resetAttempts {
            retryAttemptCount = 0
        }
    }

    func cancelStopSeekTimeout() {
        stopSeekTimeoutTask?.cancel()
        stopSeekTimeoutTask = nil
    }

    func scheduleStopSeekTimeout(playbackEpoch: UInt64) {
        cancelStopSeekTimeout()
        stopSeekTimeoutTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                guard let self, self.isPlaybackEpochCurrent(playbackEpoch), self.isSeekingToStartAfterStop else {
                    return
                }
                iosHostLog("stop seek timed out")
                self.recordBenchmark("stop_seek_timeout")
                self.isSeekingToStartAfterStop = false
                let shouldPlay = self.pendingPlayAfterStopSeek
                self.pendingPlayAfterStopSeek = false
                self.updateTimelinePosition(0)
                if shouldPlay {
                    self.startPlayback()
                }
                self.refreshPlaybackState()
            }
        }
    }

    func clearLastError() {
        publishedLastError = nil
        fixedTrackIssueActive = false
    }

    func reportCommandError(
        code: VesperPlayerErrorCode,
        category: VesperPlayerErrorCategory,
        message: String,
        details: [String: String] = [:]
    ) {
        iosHostLog("commandError category=\(category.rawValue) message=\(message)")
        fixedTrackIssueActive = false
        publishedLastError = VesperPlayerError(
            message: message,
            code: code,
            category: category,
            retriable: false,
            details: details
        )
    }

    func handlePlaybackFailure(
        error: Error?,
        fallbackMessage: String,
        itemStatusDetails: [String: String] = [:],
        itemErrorLogDetails: [String: String] = [:]
    ) {
        let resolvedError = resolvedPlaybackFailure(
            error: error,
            fallbackMessage: fallbackMessage,
            itemStatusDetails: itemStatusDetails,
            itemErrorLogDetails: itemErrorLogDetails
        )
        iosHostLog(
            "playbackFailure category=\(resolvedError.category.rawValue) retriable=\(resolvedError.retriable) message=\(resolvedError.message)"
        )
        let enrichedError = resolvedError.enrichedWithHdrFailureEvidence(currentHdrFailureEvidence)
        if activeSourceCommand != nil {
            pendingSourceCommandFailure = enrichedError.toPlayerError()
            recordBenchmark(
                "source_command_attempt_failed",
                attributes: [
                    "category": enrichedError.category.rawValue,
                    "retriable": "\(enrichedError.retriable)",
                ]
            )
            return
        }
        releaseDashStartupAbrLimitIfNeeded(reason: "playbackFailure", item: player?.currentItem)
        recordBenchmark(
            "playback_error",
            attributes: [
                "category": enrichedError.category.rawValue,
                "retriable": "\(enrichedError.retriable)",
            ]
        )
        fixedTrackIssueActive = false

        if scheduleRetryIfPossible(for: enrichedError) {
            return
        }

        let terminalError = enrichedError.enrichedWithDetails(
            retryExhaustionDetails(for: enrichedError)
        )
        pendingAutoPlay = false
        pendingPlaybackStart = false
        player?.pause()
        publishedLastError = terminalError.toPlayerError()
        updateErrorState(message: terminalError.message)
    }

    func updateErrorState(message: String) {
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: VesperPlayerI18n.nativeBridgeError(message),
                sourceLabel: $0.sourceLabel,
                playbackState: .paused,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: false,
                timeline: $0.timeline
            )
        }
    }

    func retryExhaustionDetails(for error: ResolvedBridgeError) -> [String: String] {
        guard error.retriable,
              let currentSource,
              currentSource.kind == .remote
        else {
            return [:]
        }
        let retryPolicy = currentResiliencePolicy.resolvedForRuntimeSource(currentSource).retry
        guard let maxAttempts = retryPolicy.maxAttempts,
              retryAttemptCount >= maxAttempts
        else {
            return [:]
        }
        return [
            "attemptsExhausted": "true",
            "maxAttempts": "\(maxAttempts)",
            "retryAttempts": "\(retryAttemptCount)",
        ]
    }

    func scheduleRetryIfPossible(for error: ResolvedBridgeError) -> Bool {
        guard error.retriable, let currentSource, currentSource.kind == .remote else {
            return false
        }

        let retryPolicy = currentResiliencePolicy.resolvedForRuntimeSource(currentSource).retry
        let nextAttempt = retryAttemptCount + 1
        if let maxAttempts = retryPolicy.maxAttempts, nextAttempt > maxAttempts {
            return false
        }

        let delayMs = retryDelayMs(forAttempt: nextAttempt, retryPolicy: retryPolicy)
        retryAttemptCount = nextAttempt
        pendingAutoPlay = true
        pendingPlaybackStart = false
        retryTask?.cancel()

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: VesperPlayerI18n.retryScheduled(delay: formattedRetryDelay(delayMs), message: error.message),
                sourceLabel: $0.sourceLabel,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }

        let expectedUri = currentSource.uri
        let expectedPlaybackEpoch = currentPlaybackEpoch()
        retryTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: delayMs * 1_000_000)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                self?.handleScheduledRetryFire(
                    expectedUri: expectedUri,
                    playbackEpoch: expectedPlaybackEpoch,
                    attempt: nextAttempt,
                    delayMs: delayMs
                )
            }
        }
        return true
    }

    func handleScheduledRetryFire(
        expectedUri: String,
        playbackEpoch: UInt64,
        attempt: Int,
        delayMs: UInt64
    ) {
        guard currentSource?.uri == expectedUri else {
            iosHostLog(
                staleRetryDiagnosticMessage(
                    expectedUri: expectedUri,
                    currentUri: currentSource?.uri,
                    attempt: attempt
                )
            )
            return
        }
        guard isPlaybackEpochCurrent(playbackEpoch) else {
            iosHostLog(
                "ignored stale retry task playbackEpoch=\(playbackEpoch) current=\(self.playbackEpoch) attempt=\(attempt)"
            )
            return
        }
        iosHostLog("retrying playback attempt=\(attempt) delayMs=\(delayMs)")
        initialize()
    }

    func handlePlaybackFailureForTesting(
        error: Error?,
        fallbackMessage: String,
        itemStatusDetails: [String: String] = [:],
        itemErrorLogDetails: [String: String] = [:]
    ) {
        handlePlaybackFailure(
            error: error,
            fallbackMessage: fallbackMessage,
            itemStatusDetails: itemStatusDetails,
            itemErrorLogDetails: itemErrorLogDetails
        )
    }

    func retryDelayMs(forAttempt attempt: Int, retryPolicy: VesperRetryPolicy) -> UInt64 {
        let policy = retryPolicy
        let multiplier: Double
        switch policy.backoff {
        case .fixed:
            multiplier = 1
        case .linear:
            multiplier = Double(attempt)
        case .exponential:
            multiplier = pow(2, Double(max(attempt - 1, 0)))
        }

        let computedDelay = Double(policy.baseDelayMs) * multiplier
        return min(UInt64(computedDelay.rounded()), policy.maxDelayMs)
    }

    func classifyPlaybackFailure(
        _ error: Error?,
        fallbackMessage: String
    ) -> ResolvedBridgeError {
        guard let error else {
            return ResolvedBridgeError(
                category: .platform,
                retriable: false,
                message: fallbackMessage
            )
        }

        if let playerError = error as? VesperPlayerError {
            return ResolvedBridgeError(
                code: playerError.code,
                category: playerError.category,
                retriable: playerError.retriable,
                message: playerError.message,
                details: playerError.details
            )
        }

        if let drmError = error as? VesperPlayerDrmUnsupportedError {
            return ResolvedBridgeError(
                code: .unsupported,
                category: .capability,
                retriable: false,
                message: drmError.localizedDescription,
                details: drmError.details
            )
        }
        if let drmError = error as? VesperPlayerDrmRuntimeError {
            if drmError.retriable {
                return ResolvedBridgeError(
                    category: .network,
                    retriable: true,
                    message: drmError.localizedDescription,
                    details: drmError.details
                )
            }
            return ResolvedBridgeError(
                code: .unsupported,
                category: .capability,
                retriable: false,
                message: drmError.localizedDescription,
                details: drmError.details
            )
        }

        let nsError = error as NSError
        if nsError.domain == "io.github.umbrella22.vesper.host.ios",
           nsError.code == -3 || nsError.code == -4 {
            return ResolvedBridgeError(
                code: .unsupported,
                category: .capability,
                retriable: false,
                message: nsError.localizedDescription,
                capabilityFailureCause: .hostNativeFrameUnsupported
            )
        }
        if nsError.domain == NSURLErrorDomain {
            switch nsError.code {
            case NSURLErrorTimedOut,
                NSURLErrorCannotFindHost,
                NSURLErrorCannotConnectToHost,
                NSURLErrorNetworkConnectionLost,
                NSURLErrorDNSLookupFailed,
                NSURLErrorNotConnectedToInternet:
                return ResolvedBridgeError(
                    category: .network,
                    retriable: true,
                    message: nsError.localizedDescription
                )
            case NSURLErrorBadServerResponse:
                return ResolvedBridgeError(
                    category: .network,
                    retriable: true,
                    message: nsError.localizedDescription
                )
            case NSURLErrorFileDoesNotExist,
                NSURLErrorBadURL,
                NSURLErrorUnsupportedURL:
                return ResolvedBridgeError(
                    category: .source,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            case NSURLErrorNoPermissionsToReadFile:
                return ResolvedBridgeError(
                    category: .capability,
                    retriable: false,
                    message: nsError.localizedDescription,
                    capabilityFailureCause: .filePermissionDenied
                )
            default:
                break
            }
        }

        if nsError.domain == AVFoundationErrorDomain || nsError.domain == AVError.errorDomain {
            switch AVError.Code(rawValue: nsError.code) {
            case .decoderNotFound:
                return ResolvedBridgeError(
                    category: .decode,
                    retriable: false,
                    message: nsError.localizedDescription,
                    capabilityFailureCause: .decoderNotFound
                )
            case .decoderTemporarilyUnavailable:
                return ResolvedBridgeError(
                    category: .decode,
                    retriable: false,
                    message: nsError.localizedDescription,
                    capabilityFailureCause: .decoderTemporarilyUnavailable
                )
            case .fileFormatNotRecognized:
                return ResolvedBridgeError(
                    category: .capability,
                    retriable: false,
                    message: nsError.localizedDescription,
                    capabilityFailureCause: .fileFormatNotRecognized
                )
            case .contentIsUnavailable, .mediaServicesWereReset:
                return ResolvedBridgeError(
                    category: .platform,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            default:
                break
            }
        }

        return ResolvedBridgeError(
            category: .platform,
            retriable: false,
            message: nsError.localizedDescription
        )
    }

    private func httpStatusCode(
        itemStatusDetails: [String: String],
        itemErrorLogDetails: [String: String]
    ) -> Int? {
        let values = itemErrorLogDetails.merging(itemStatusDetails) { current, _ in current }
        for key in [
            "avPlayerItemErrorStatusCode",
            "avPlayerItemStatusCode",
            "httpStatusCode",
        ] {
            if let status = values[key].flatMap(Int.init), (400...599).contains(status) {
                return status
            }
        }
        return nil
    }

    private func reclassifyHTTPSourceError(
        _ error: ResolvedBridgeError,
        nativeError: Error?,
        itemStatusDetails: [String: String],
        itemErrorLogDetails: [String: String]
    ) -> ResolvedBridgeError {
        guard error.category == .platform,
            isRemoteHTTPSource,
            !isMediaServicesReset(nativeError),
            let statusCode = httpStatusCode(
                itemStatusDetails: itemStatusDetails,
                itemErrorLogDetails: itemErrorLogDetails
            )
        else {
            return error
        }

        var details = error.details
        details["httpStatusCode"] = String(statusCode)
        return ResolvedBridgeError(
            code: .backendFailure,
            category: .network,
            retriable: true,
            message: error.message,
            details: details,
            capabilityFailureCause: error.capabilityFailureCause
        )
    }

    private func isMediaServicesReset(_ error: Error?) -> Bool {
        guard let nsError = error as NSError?,
              nsError.domain == AVFoundationErrorDomain || nsError.domain == AVError.errorDomain
        else {
            return false
        }
        return AVError.Code(rawValue: nsError.code) == .mediaServicesWereReset
    }

    private var isRemoteHTTPSource: Bool {
        guard let source = currentSource,
              source.kind == .remote,
              let scheme = URL(string: source.uri)?.scheme?.lowercased()
        else {
            return false
        }
        return scheme == "http" || scheme == "https"
    }

    private func sourceFailureDiagnosticDetails(
        for error: ResolvedBridgeError,
        nativeError: Error?
    ) -> [String: String] {
        guard error.category == .network || error.category == .source else {
            return [:]
        }

        var details: [String: String] = [:]
        if let nativeError {
            let nsError = nativeError as NSError
            details["nativeErrorDomain"] = nsError.domain
            details["nativeErrorCode"] = String(nsError.code)
        }
        if let source = currentSource {
            details["sourceProtocol"] = source.protocol.rawValue
            if let redactedSourceUri = redactedURLForDiagnostics(source.uri) {
                details["sourceUri"] = redactedSourceUri
            }
        }
        return details
    }

    func resolvedBufferingPolicy(_ resolvedPolicy: VesperBufferingPolicy) -> ResolvedBufferingPolicy {
        let effectiveMs =
            resolvedPolicy.maxBufferMs
            ?? resolvedPolicy.minBufferMs
            ?? resolvedPolicy.bufferForPlaybackAfterRebufferMs
            ?? resolvedPolicy.bufferForPlaybackMs
            ?? 0

        let automaticallyWaits = switch resolvedPolicy.preset {
        case .lowLatency:
            false
        default:
            true
        }

        return ResolvedBufferingPolicy(
            preferredForwardBufferDuration: TimeInterval(effectiveMs) / 1000.0,
            automaticallyWaitsToMinimizeStalling: automaticallyWaits
        )
    }

    func resolvedCachePolicy(_ resolvedPolicy: VesperCachePolicy) -> ResolvedCachePolicy {
        let maxMemoryBytes = resolvedPolicy.maxMemoryBytes ?? 0
        let maxDiskBytes = resolvedPolicy.maxDiskBytes ?? 0

        return ResolvedCachePolicy(
            enabled: max(maxMemoryBytes, maxDiskBytes) > 0,
            memoryCapacity: clampToInt(maxMemoryBytes),
            diskCapacity: clampToInt(maxDiskBytes)
        )
    }

    func formattedRetryDelay(_ delayMs: UInt64) -> String {
        let seconds = Double(delayMs) / 1000.0
        if seconds >= 10 || seconds.rounded() == seconds {
            return VesperPlayerI18n.retryDelaySecondsInt(Int(seconds.rounded()))
        }
        return VesperPlayerI18n.retryDelaySecondsDecimal(seconds)
    }

    func configureAudioSessionIfNeeded() {
        guard !audioSessionLease.isActive else { return }
        audioSessionLease.activate()
        iosHostLog("audio session activation requested")
    }

    func deactivateAudioSessionIfNeeded() {
        audioSessionLease.deactivate()
    }
}
