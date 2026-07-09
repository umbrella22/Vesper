@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

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
            iosHostLog("initialize ignored: native-frame pipeline already configured source=\(currentSource.uri)")
            recordBenchmark("initialize_native_frame_already_active")
            if pendingAutoPlay {
                pendingAutoPlay = false
                nativeSession.play(rate: desiredPlaybackRate)
                updateState {
                    PlayerHostUiState(
                        title: $0.title,
                        subtitle: $0.subtitle,
                        sourceLabel: $0.sourceLabel,
                        playbackState: .playing,
                        playbackRate: $0.playbackRate,
                        isBuffering: false,
                        isInterrupted: $0.isInterrupted,
                        timeline: $0.timeline
                    )
                }
            }
            return
        }
        let shouldAutoPlay = pendingAutoPlay || player == nil
        iosHostLog(
            "initialize source=\(currentSource.uri) label=\(currentSource.label) kind=\(currentSource.kind.rawValue) protocol=\(currentSource.protocol.rawValue) autoPlay=\(shouldAutoPlay)"
        )
        configureAudioSessionIfNeeded()
        pendingAutoPlay = shouldAutoPlay
        startSourceLoadTask(source: currentSource, shouldAutoPlay: shouldAutoPlay)
    }

    func initializeAsync() async {
        initialize()
        await sourceLoadTask?.value
    }

    func dispose() {
        clearLastError()
        recordBenchmark("dispose_command")
        iosHostLog("dispose")
        cancelPendingRetry(resetAttempts: true)
        cancelStopSeekTimeout()
        cancelSourceLoadTask()
        pendingResilienceRestore = nil
        currentSource = nil
        currentHdrFailureEvidence = nil
        hasAppliedDefaultTrackPreferences = false
        pendingAutoPlay = false
        pendingNativeFrameSurfaceLoad = false
        pendingNativeFrameSeek = nil
        tearDownActivePlayback()
        deactivateAudioSessionIfNeeded()
        benchmarkRecorder.dispose()
    }

    func refresh() {
        refreshPlaybackState()
    }

    func selectSource(_ source: VesperPlayerSource) {
        clearLastError()
        recordBenchmark(
            "select_source_start",
            attributes: ["targetProtocol": source.protocol.rawValue]
        )
        iosHostLog(
            "selectSource source=\(source.uri) label=\(source.label) kind=\(source.kind.rawValue) protocol=\(source.protocol.rawValue)"
        )
        currentSource = source
        currentHdrFailureEvidence = nil
        cancelPendingRetry(resetAttempts: true)
        pendingResilienceRestore = nil
        pendingAutoPlay = true
        pendingNativeFrameSeek = nil
        cancelSourceLoadTask()
        tearDownActivePlayback()
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: sourceSubtitle(for: source),
                sourceLabel: source.label,
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
        initialize()
    }

    func selectSourceAsync(_ source: VesperPlayerSource) async {
        selectSource(source)
        await sourceLoadTask?.value
    }

    func startSourceLoadTask(source: VesperPlayerSource, shouldAutoPlay: Bool) {
        cancelSourceLoadTask()
        let epoch = nextSourceLoadEpoch()
        sourceLoadTask = Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let pluginDiagnostics = await self.probeMobilePluginsAsync(for: source)
                guard !Task.isCancelled, self.isCurrentSourceLoad(epoch, source: source) else { return }
                self.currentPluginDiagnostics = self.pluginDiagnosticsWithNativeFramePipeline(pluginDiagnostics)
                try await self.loadCurrentSource(source, sourceLoadEpoch: epoch)
                guard !Task.isCancelled, self.isCurrentSourceLoad(epoch, source: source) else { return }
                self.sourceLoadTask = nil
                self.pendingNativeFrameSurfaceLoad = false
                let shouldStartAfterLoad = shouldAutoPlay && self.pendingAutoPlay
                self.pendingAutoPlay = false
                if shouldStartAfterLoad {
                    iosHostLog("auto-playing source=\(source.uri)")
                    self.startPlayback()
                }
                self.refreshPlaybackState()
                self.recordBenchmark("initialize_completed")
            } catch {
                guard !Task.isCancelled, self.isCurrentSourceLoad(epoch, source: source) else { return }
                self.sourceLoadTask = nil
                self.finishSourceLoadFailure(error)
            }
        }
    }

    func cancelSourceLoadTask() {
        sourceLoadTask?.cancel()
        sourceLoadTask = nil
        sourceLoadEpoch &+= 1
        fairPlayDrmCoordinator?.cancelPendingRequests()
    }

    func finishSourceLoadFailure(_ error: Error) {
        if !pendingNativeFrameSurfaceLoad {
            pendingAutoPlay = false
        }
        if pendingNativeFrameSurfaceLoad {
            iosHostLog("initialize deferred: \(error.localizedDescription)")
            recordBenchmark(
                "initialize_deferred",
                attributes: ["reason": error.localizedDescription]
            )
            resumePendingNativeFrameSurfaceLoadIfNeeded()
            return
        }
        iosHostLog("initialize failed: \(error.localizedDescription)")
        closeCurrentSourceNormalizerResource()
        recordBenchmark(
            "initialize_failed",
            attributes: ["error": error.localizedDescription]
        )
        handlePlaybackFailure(error: error, fallbackMessage: error.localizedDescription)
    }

    func probeMobilePluginsAsync(for source: VesperPlayerSource) async -> [[String: Any]] {
        let sourceNormalizer = sourceNormalizerConfiguration
        let frameProcessor = frameProcessorConfiguration
        return await VesperBoundedUtilityQueue.shared.run(fallback: { [] }) {
            VesperMobilePluginDiagnosticsProbe.run(
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
        pendingNativeFrameSurfaceLoad = false
        initialize()
    }

    func tearDownActivePlayback() {
        cancelSourceLoadTask()
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
}
