@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func initialize() {
        clearLastError()
        recordBenchmark("initialize_start")
        logLinkedPluginAbiSummaryIfNeeded()
        guard let currentSource else {
            recordBenchmark("initialize_without_source")
            updateState {
                PlayerHostUiState(
                    title: $0.title,
                    subtitle: VesperPlayerI18n.selectSourcePrompt,
                    sourceLabel: VesperPlayerI18n.noSourceSelected,
                    playbackState: .ready,
                    playbackRate: $0.playbackRate,
                    isBuffering: false,
                    isInterrupted: $0.isInterrupted,
                    timeline: TimelineUiState(
                        kind: .vod,
                        isSeekable: true,
                        seekableRange: SeekableRangeUi(startMs: 0, endMs: 0),
                        liveEdgeMs: nil,
                        positionMs: 0,
                        durationMs: nil
                    )
                )
            }
            return
        }
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart,
           nativeSession.source == currentSource {
            let sourceDescription = diagnosticURLDescription(currentSource.uri)
            iosHostLog(
                "initialize ignored: native-frame pipeline already configured source=\(sourceDescription)"
            )
            recordBenchmark("initialize_native_frame_already_active")
            if pendingAutoPlay {
                pendingAutoPlay = false
                startNativeFrameSessionPlayback(nativeSession)
            }
            return
        }
        let shouldAutoPlay = pendingAutoPlay || player == nil
        let sourceDescription = diagnosticURLDescription(currentSource.uri)
        iosHostLog(
            "initialize source=\(sourceDescription) kind=\(currentSource.kind.rawValue) protocol=\(currentSource.protocol.rawValue) autoPlay=\(shouldAutoPlay)"
        )
        configureAudioSessionIfNeeded()
        pendingAutoPlay = shouldAutoPlay
        startSourceLoadTask(source: currentSource, shouldAutoPlay: shouldAutoPlay)
    }

    func initializeAsync() async throws {
        initialize()
        guard let task = sourceLoadTask else { return }
        try await task.value
    }

    func dispose() {
        clearLastError()
        recordBenchmark("dispose_command")
        iosHostLog("dispose")
        cancelPendingRetry(resetAttempts: true)
        cancelStopSeekTimeout()
        cancelSourceLoadTask(reason: "sourceCommandDisposed")
        advanceSubtitleSourceEpoch()
        pendingResilienceRestore = nil
        currentSource = nil
        currentHdrFailureEvidence = nil
        hasAppliedDefaultTrackPreferences = false
        pendingAutoPlay = false
        pendingNativeFrameSurfaceLoad = false
        pendingNativeFrameSeek = nil
        tearDownActivePlayback(cancelSourceCommand: false)
        deactivateAudioSessionIfNeeded()
        benchmarkRecorder.dispose(player: player)
        _ = pipelineEventHookSession?.flush()
        let reportBatch = pipelineEventHookSession?.drainReports() ?? VesperPipelineEventHookReportBatch()
        if !reportBatch.isEmpty {
            finalizedPipelineEventHookReports = reportBatch
        }
        if !reportBatch.isEmpty {
            let dispatcherError = reportBatch.dispatcherError ?? "none"
            iosHostLog(
                "playback EventHook reports drained count=\(reportBatch.reports.count) " +
                    "droppedEvents=\(reportBatch.droppedEvents) " +
                    "droppedReports=\(reportBatch.droppedReports) " +
                    "error=\(dispatcherError)"
            )
        }
        pipelineEventHookSession?.dispose()
    }

    func refresh() {
        refreshPlaybackState()
    }

    func selectSource(_ source: VesperPlayerSource) {
        _ = startSourceSelection(source)
    }

    func startSourceSelection(_ source: VesperPlayerSource) -> Task<Void, Error> {
        clearLastError()
        recordBenchmark(
            "select_source_start",
            attributes: ["targetProtocol": source.protocol.rawValue]
        )
        let sourceDescription = diagnosticURLDescription(source.uri)
        iosHostLog(
            "selectSource source=\(sourceDescription) kind=\(source.kind.rawValue) protocol=\(source.protocol.rawValue)"
        )
        cancelPendingRetry(resetAttempts: true)
        cancelSourceLoadTask(reason: "sourceCommandSuperseded")
        cancelPendingSeekCommand(reason: "seekSourceChanged")
        tearDownActivePlayback(cancelSourceCommand: false)
        advanceSubtitleSourceEpoch()
        currentSource = source
        currentSourceIsConfirmedLive = nil
        currentHdrFailureEvidence = nil
        pendingResilienceRestore = nil
        pendingAutoPlay = true
        pendingNativeFrameSeek = nil
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: sourceSubtitle(for: source),
                sourceLabel: source.label,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: true,
                isInterrupted: $0.isInterrupted,
                timeline: TimelineUiState(
                    kind: .vod,
                    isSeekable: true,
                    seekableRange: SeekableRangeUi(startMs: 0, endMs: 0),
                    liveEdgeMs: nil,
                    positionMs: 0,
                    durationMs: nil
                )
            )
        }
        configureAudioSessionIfNeeded()
        return startSourceLoadTask(source: source, shouldAutoPlay: true)
    }

    func selectSourceAsync(_ source: VesperPlayerSource) async throws {
        let task = startSourceSelection(source)
        try await task.value
    }

    @discardableResult
    func startSourceLoadTask(
        source: VesperPlayerSource,
        shouldAutoPlay: Bool
    ) -> Task<Void, Error> {
        cancelSourceLoadTask(reason: "sourceCommandSuperseded")
        sourceCommandGeneration &+= 1
        if sourceCommandGeneration == 0 {
            sourceCommandGeneration = 1
        }
        let command = VesperSourceCommandHandle(
            commandId: sourceCommandGeneration,
            source: source,
            deadline: ContinuousClock().now.advanced(by: sourceReadinessWaitPolicy.timeout)
        )
        activeSourceCommand = command
        pendingSourceCommandFailure = nil
        retryAttemptCount = 0
        switch nativeFramePipelineConfiguration.mode {
        case .preferNativeFrame, .requireNativeFrame:
            pendingNativeFrameSurfaceLoad = surfaceHost == nil
        case .disabled, .diagnosticsOnly:
            pendingNativeFrameSurfaceLoad = false
        }
        let task = Task { @MainActor [weak self, weak command] in
            guard let self, let command else {
                throw CancellationError()
            }
            do {
                try await self.executeSourceCommand(
                    command,
                    shouldAutoPlay: shouldAutoPlay
                )
            } catch is CancellationError {
                throw self.obsoleteSourceCommandError(
                    command,
                    reason: command.cancellationReason ?? "sourceCommandCancelled"
                )
            } catch {
                throw error
            }
        }
        command.task = task
        sourceLoadTask = task
        return task
    }

    func cancelSourceLoadTask(reason: String = "sourceCommandCancelled") {
        activeSourceCommand?.cancellationReason = reason
        activeSourceCommand?.task?.cancel()
        sourceLoadTask?.cancel()
        activeSourceCommand = nil
        sourceLoadTask = nil
        pendingSourceCommandFailure = nil
        subtitleOverlayLoadTask?.cancel()
        subtitleOverlayLoadTask = nil
        sourceLoadEpoch &+= 1
        fairPlayDrmCoordinator?.cancelPendingRequests()
    }

    func executeSourceCommand(
        _ command: VesperSourceCommandHandle,
        shouldAutoPlay: Bool
    ) async throws {
        let clock = ContinuousClock()
        defer {
            if activeSourceCommand === command {
                activeSourceCommand = nil
                sourceLoadTask = nil
                pendingSourceCommandFailure = nil
            }
        }

        while true {
            try Task.checkCancellation()
            try ensureCurrentSourceCommand(command)
            guard clock.now < command.deadline else {
                let error = sourceCommandTimeoutError(command)
                tearDownActivePlayback(cancelSourceCommand: false)
                publishSourceCommandFailure(error, command: command)
                throw error
            }
            let epoch = nextSourceLoadEpoch()
            do {
                let pluginDiagnostics = await probeMobilePluginsAsync(for: command.source)
                try Task.checkCancellation()
                try ensureCurrentSourceCommand(command)
                guard isCurrentSourceLoad(epoch, source: command.source) else {
                    throw obsoleteSourceCommandError(command, reason: "sourceCommandSuperseded")
                }
                currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(pluginDiagnostics)
                if let sourceLoadAttemptOverride {
                    try await sourceLoadAttemptOverride(
                        self,
                        command.source,
                        epoch,
                        command.deadline
                    )
                } else {
                    try await loadCurrentSource(
                        command.source,
                        sourceLoadEpoch: epoch,
                        deadline: command.deadline
                    )
                }
                try Task.checkCancellation()
                try ensureCurrentSourceCommand(command)
                guard isCurrentSourceLoad(epoch, source: command.source) else {
                    throw obsoleteSourceCommandError(command, reason: "sourceCommandSuperseded")
                }

                pendingNativeFrameSurfaceLoad = false
                retryAttemptCount = 0
                let shouldStartAfterLoad = shouldAutoPlay && pendingAutoPlay
                pendingAutoPlay = false
                if shouldStartAfterLoad {
                    iosHostLog(
                        "auto-playing source=\(diagnosticURLDescription(command.source.uri))"
                    )
                    startPlayback()
                }
                refreshPlaybackState()
                recordBenchmark("initialize_completed")
                return
            } catch is CancellationError {
                throw obsoleteSourceCommandError(
                    command,
                    reason: command.cancellationReason ?? "sourceCommandCancelled"
                )
            } catch let error as VesperPlayerError where error.details["obsolete"] == "true" {
                throw error
            } catch {
                try ensureCurrentSourceCommand(command)
                let resolved = resolvedPlaybackFailure(
                    error: error,
                    fallbackMessage: error.localizedDescription
                )
                if let delay = sourceCommandRetryDelay(resolved, command: command) {
                    command.retryAttemptCount += 1
                    retryAttemptCount = command.retryAttemptCount
                    tearDownActivePlayback(cancelSourceCommand: false)
                    publishSourceCommandRetry(
                        resolved,
                        command: command,
                        delayMs: delay
                    )
                    try await clock.sleep(for: .milliseconds(Int64(delay)))
                    continue
                }

                let terminal = sourceCommandTerminalError(
                    resolved,
                    command: command
                )
                tearDownActivePlayback(cancelSourceCommand: false)
                publishSourceCommandFailure(terminal, command: command)
                throw terminal
            }
        }
    }

    func probeMobilePluginsAsync(for source: VesperPlayerSource) async -> [[String: Any]] {
        let sourceNormalizer = sourceNormalizerConfiguration
        let frameProcessor = frameProcessorConfiguration
        return await VesperBoundedUtilityQueue.shared.run(fallback: { [] }) {
            return VesperMobilePluginDiagnosticsProbe.run(
                source: source,
                sourceNormalizer: sourceNormalizer,
                frameProcessor: frameProcessor
            )
        }
    }

    func pluginDiagnosticsWithNativeFramePipeline(_ diagnostics: [[String: Any]]) -> [[String: Any]] {
        diagnostics.filter { diagnostic in
            (diagnostic["pluginKind"] as? String) != "native_frame_pipeline"
        } + nativeFramePipelineDiagnostics(fallbackIssue: nativeFramePipelineFallbackIssue)
    }

    func nextSourceLoadEpoch() -> UInt64 {
        sourceLoadEpoch &+= 1
        return sourceLoadEpoch
    }

    /// Returns `true` only when both the epoch and the source identity still match
    /// the load that captured them.
    ///
    /// The `&& currentSource == source` check is load-bearing: `sourceLoadEpoch`
    /// is a wrapping `UInt64` counter (`&+= 1`), so on a theoretical 2^64-wrap it
    /// could revisit an old value. The source-identity clause makes the predicate
    /// behave as a never-reuse token even in that case, because a new load always
    /// reassigns `currentSource` before bumping the epoch. Do not simplify this to
    /// an epoch-only comparison.
    func isCurrentSourceLoad(_ epoch: UInt64, source: VesperPlayerSource) -> Bool {
        sourceLoadEpoch == epoch && currentSource == source
    }

    func logLinkedPluginAbiSummaryIfNeeded() {
        guard !didLogLinkedPluginAbiSummary else {
            return
        }
        didLogLinkedPluginAbiSummary = true

        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_plugin_abi_summary_json(&outputPointer, &errorPointer)
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }

        if ok, let outputPointer {
            iosHostLog("linked Rust plugin ABI summary: \(String(cString: outputPointer))")
        } else if let errorPointer {
            iosHostLog("linked Rust plugin ABI summary failed: \(String(cString: errorPointer))")
        } else {
            iosHostLog("linked Rust plugin ABI summary failed")
        }
    }

    func nativeFramePipelineDiagnostics(
        fallbackIssue: VesperNativeFramePipelineIssue? = nil
    ) -> [[String: Any]] {
        nativeFramePipelineCoordinator.makeDiagnostics(
            configuration: nativeFramePipelineConfiguration,
            fallbackIssue: fallbackIssue
        )
    }

    func evaluateNativeFramePipelineRoute(for source: VesperPlayerSource) -> VesperNativeFramePipelineRouteDecision {
        let decision = nativeFramePipelineCoordinator.evaluateRoute(
            for: source,
            configuration: nativeFramePipelineConfiguration,
            sourceNormalizer: sourceNormalizerConfiguration,
            surfaceHost: surfaceHost
        )
        switch decision {
        case .systemPlayer, .nativeFrame:
            nativeFramePipelineFallbackIssue = nil
        case .fallback(let issue):
            nativeFramePipelineFallbackIssue = issue
        case .fail(let issue):
            nativeFramePipelineFallbackIssue = nil
            if case .fail = decision {
                reportCommandError(
                    code: .unsupported,
                    category: .capability,
                    message: issue.message
                )
            }
        case .waitForSurface(let issue):
            nativeFramePipelineFallbackIssue = nil
            iosHostLog("native-frame pipeline waiting: \(issue.message)")
        }
        currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
        if case .fallback(let issue) = decision {
            iosHostLog("native-frame pipeline fallback: \(issue.message)")
        }
        return decision
    }

    func resumePendingNativeFrameSurfaceLoadIfNeeded() {
        guard pendingNativeFrameSurfaceLoad,
              surfaceHost != nil,
              player == nil,
              currentSource != nil
        else {
            return
        }
        guard nativeFramePipelineCoordinator.activeSession == nil else {
            return
        }
        if activeSourceCommand != nil {
            return
        }
        pendingNativeFrameSurfaceLoad = false
        initialize()
    }

    func tearDownActivePlayback(cancelSourceCommand: Bool = true) {
        if cancelSourceCommand {
            cancelSourceLoadTask()
        }
        cancelPendingSeekCommand(reason: "seekPlaybackTornDown")
        releaseDashStartupAbrLimitIfNeeded(reason: "tearDown", item: player?.currentItem)
        _ = advancePlaybackEpoch()
        cancelStopSeekTimeout()
        preloadCoordinator.cancelAll()
        VesperSharedUrlCacheCoordinator.shared.remove(token: cachePolicyToken)
        pendingPlaybackStart = false
        pendingNativeFrameSurfaceLoad = false
        pendingNativeFrameSeek = nil
        pendingPlayAfterStopSeek = false
        isSeekingToStartAfterStop = false
        removeObservers()
        cancelPendingSubtitleSelection()
        player?.pause()
        surfaceHost?.attach(player: nil)
        nativeFramePipelineCoordinator.closeActiveSession()
        player = nil
        currentDashSession = nil
        dashResourceLoaderDelegate = nil
        fairPlayDrmCoordinator?.close()
        fairPlayDrmCoordinator = nil
        fairPlayDrmCoordinatorId = nil
        closeCurrentSourceNormalizerResource()
        resetTrackState()
    }

    @discardableResult
    func advanceSubtitleSourceEpoch() -> UInt64 {
        subtitleSourceEpoch &+= 1
        cancelPendingSubtitleSelection()
        explicitSubtitleIntentSourceEpoch = nil
        latestConfirmedExplicitSubtitleSelection = nil
        return subtitleSourceEpoch
    }

    func cancelPendingSubtitleSelection() {
        subtitleSelectionTask?.cancel()
        subtitleSelectionTask = nil
        pendingSubtitleSelection = nil
    }
}
