@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func loadCurrentSource(
        _ source: VesperPlayerSource,
        sourceLoadEpoch: UInt64
    ) async throws {
        guard currentSource == source else {
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: "Selected source changed before loading started."]
            )
        }

        recordBenchmark("source_load_start")
        if let drmFailure = vesperDrmPhase0Failure(
            for: source,
            sourceNormalizerConfiguration: sourceNormalizerConfiguration,
            nativeFramePipelineConfiguration: nativeFramePipelineConfiguration
        ) {
            throw drmFailure
        }
        switch evaluateNativeFramePipelineRoute(for: source) {
        case .systemPlayer, .fallback:
            break
        case .waitForSurface(let issue):
            pendingNativeFrameSurfaceLoad = true
            pendingAutoPlay = pendingAutoPlay || player == nil
            currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -5,
                userInfo: [NSLocalizedDescriptionKey: issue.message]
            )
        case .nativeFrame:
            let startupSession = nativeFramePipelineCoordinator.activeSession
            switch await nativeFramePipelineCoordinator.startActiveSession() {
            case .success(let session):
                guard !Task.isCancelled else {
                    nativeFramePipelineCoordinator.closeSession(session)
                    throw CancellationError()
                }
                guard isCurrentSourceLoad(sourceLoadEpoch, source: source) else {
                    nativeFramePipelineCoordinator.closeSession(session)
                    throw NSError(
                        domain: "io.github.umbrella22.vesper.host.ios",
                        code: -1,
                        userInfo: [NSLocalizedDescriptionKey: "Selected source changed before native-frame startup completed."]
                    )
                }
                configureNativeFramePlayback(source: source, session: session)
                return
            case .failure(let error):
                guard !Task.isCancelled else {
                    nativeFramePipelineCoordinator.closeActiveSession(ifSameAs: startupSession)
                    throw CancellationError()
                }
                guard isCurrentSourceLoad(sourceLoadEpoch, source: source) else {
                    nativeFramePipelineCoordinator.closeActiveSession(ifSameAs: startupSession)
                    throw NSError(
                        domain: "io.github.umbrella22.vesper.host.ios",
                        code: -1,
                        userInfo: [NSLocalizedDescriptionKey: "Selected source changed before native-frame startup completed."]
                    )
                }
                if nativeFramePipelineConfiguration.mode == .preferNativeFrame {
                    nativeFramePipelineFallbackIssue = error.issue
                    nativeFramePipelineCoordinator.closeActiveSession(ifSameAs: startupSession)
                    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
                    iosHostLog("native-frame pipeline fallback: \(error.message)")
                    break
                }
                currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
                nativeFramePipelineCoordinator.closeActiveSession(ifSameAs: startupSession)
                throw NSError(
                    domain: "io.github.umbrella22.vesper.host.ios",
                    code: -3,
                    userInfo: [NSLocalizedDescriptionKey: error.localizedDescription]
                )
            }
        case .fail(let issue):
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -4,
                userInfo: [NSLocalizedDescriptionKey: issue.message]
            )
        }
        let normalizedResource = await openSourceNormalizerResourceIfNeeded(
            for: source,
            sourceLoadEpoch: sourceLoadEpoch
        )
        if normalizedResource == nil && sourceNormalizerConfiguration.mode == .requireNormalized {
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -2,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "SourceNormalizer requireNormalized failed to open a normalized resource."
                ]
            )
        }
        let normalizedSession = makeSourceNormalizerResourceSession(for: normalizedResource)
        let playbackSource = normalizedPlaybackSource(
            original: source,
            resource: normalizedResource
        )
        let url: URL
        if let normalizedURL = normalizedSession?.playbackURL {
            url = normalizedURL
        } else if normalizedResource != nil && sourceNormalizerConfiguration.mode == .requireNormalized {
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -2,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "SourceNormalizer requireNormalized failed to create a playback resource loader session."
                ]
            )
        } else {
            url = try resolvedUrl(for: source)
        }
        let urlDescription = diagnosticURLDescription(url.absoluteString)
        iosHostLog(
            "loadCurrentSource url=\(urlDescription) sourceNormalizerRoute=\(normalizedResource?.outputRoute ?? "native")"
        )
        try Task.checkCancellation()
        guard currentSource == source else {
            closeCurrentSourceNormalizerResource()
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: "Selected source changed before player item configuration."]
            )
        }
        let resolvedResiliencePolicy = currentResiliencePolicy.resolvedForRuntimeSource(source)
        resolvedTrackPreferencePolicy = trackPreferencePolicy.resolvedForRuntime()
        let cachePolicy = resolvedCachePolicy(resolvedResiliencePolicy.cache)
        VesperSharedUrlCacheCoordinator.shared.apply(
            policy: cachePolicy,
            token: cachePolicyToken
        )
        preloadCoordinator.configure(cachePolicy: cachePolicy)
        preloadCoordinator.warmCurrentSource(source: source, url: url)
        releaseDashStartupAbrLimitIfNeeded(reason: "sourceReload", item: player?.currentItem)
        try Task.checkCancellation()
        guard isCurrentSourceLoad(sourceLoadEpoch, source: source) else {
            throw CancellationError()
        }
        let item = try makePlayerItem(
            for: playbackSource,
            url: url,
            sourceEpoch: subtitleSourceEpoch
        )
        applySubtitleStyle(currentSubtitleStyle, to: item)
        refreshCurrentHdrFailureEvidence(for: playbackSource, item: item)
        let bufferingPolicy = resolvedBufferingPolicy(resolvedResiliencePolicy.buffering)
        item.preferredForwardBufferDuration = bufferingPolicy.preferredForwardBufferDuration
        let player = AVPlayer(playerItem: item)
        player.allowsExternalPlayback = true
        player.automaticallyWaitsToMinimizeStalling =
            bufferingPolicy.automaticallyWaitsToMinimizeStalling
        applyDefaultPlaybackRate(desiredPlaybackRate, to: player)

        let playbackEpoch = advancePlaybackEpoch()
        removeObservers()
        pendingPlaybackStart = false
        hasAppliedDefaultTrackPreferences = false
        currentHdrFailureEvidence = nil
        resetTrackState()
        applyDashStartupAbrLimitIfNeeded(for: playbackSource, to: item)
        self.player = player
        surfaceHost?.attach(player: player)
        subtitleOverlayRenderer.attach(surfaceHost: surfaceHost)
        installObservers(for: player, item: item, playbackEpoch: playbackEpoch)
        startSubtitleOverlayLoadTask(
            configurations: playbackSource.externalSubtitles,
            source: source,
            sourceLoadEpoch: sourceLoadEpoch,
            item: item,
            playbackEpoch: playbackEpoch
        )
        recordBenchmark("source_load_configured")

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: normalizedResource.map { "SourceNormalizer \($0.outputRoute)" }
                    ?? sourceSubtitle(for: source),
                sourceLabel: source.label,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: false,
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
    }

    func startSubtitleOverlayLoadTask(
        configurations: [VesperExternalSubtitleSource],
        source: VesperPlayerSource,
        sourceLoadEpoch: UInt64,
        item: AVPlayerItem,
        playbackEpoch: UInt64
    ) {
        subtitleOverlayLoadTask?.cancel()
        subtitleOverlayLoadTask = nil
        guard !configurations.isEmpty else { return }

        subtitleOverlayLoadTask = Task { @MainActor [weak self, weak item] in
            guard let self, let item else { return }
            do {
                let prepared = try await self.subtitleOverlayRenderer.prepare(configurations)
                try Task.checkCancellation()
                guard self.isCurrentSourceLoad(sourceLoadEpoch, source: source),
                      self.currentPlaybackEpoch() == playbackEpoch,
                      self.player?.currentItem === item
                else {
                    return
                }
                self.pendingSubtitleOverlayFailure = prepared.failures.first
                self.subtitleOverlayRenderer.install(prepared)
                self.hasAppliedDefaultTrackPreferences = false
                self.subtitleOverlayLoadTask = nil
                self.refreshTrackCatalogAndSelection(for: item)
            } catch is CancellationError {
                return
            } catch {
                guard self.isCurrentSourceLoad(sourceLoadEpoch, source: source),
                      self.currentPlaybackEpoch() == playbackEpoch,
                      self.player?.currentItem === item
                else {
                    return
                }
                self.subtitleOverlayLoadTask = nil
                self.pendingSubtitleOverlayFailure = .init(
                    trackId: "",
                    error: VesperSubtitleError(
                        code: "subtitle_resource_failed",
                        phase: .resource,
                        trackId: nil,
                        retriable: true,
                        message: "External subtitle preparation failed."
                    )
                )
                self.refreshTrackCatalogAndSelection(for: item)
            }
        }
    }
}
