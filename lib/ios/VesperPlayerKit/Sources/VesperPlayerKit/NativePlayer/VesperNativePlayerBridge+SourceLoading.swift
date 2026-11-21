@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func loadCurrentSource(
        _ source: VesperPlayerSource,
        sourceLoadEpoch: UInt64,
        deadline: ContinuousClock.Instant
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
        if source.protocol == .rtsp, let retiringRtspHandle {
            try await VesperRtspSession.waitForRetirement(retiringRtspHandle, deadline: deadline)
            try Task.checkCancellation()
            guard isCurrentSourceLoad(sourceLoadEpoch, source: source) else { throw CancellationError() }
            self.retiringRtspHandle = nil
        }
        let rtsp = try source.protocol == .rtsp ? VesperRtspSession(source: source) : nil
        if let rtsp { currentRtspSession = rtsp }
        // A superseded activation may finish after its replacement owns the bridge.
        defer {
            if Task.isCancelled, let rtsp, currentRtspSession === rtsp {
                tearDownActivePlayback(cancelSourceCommand: false)
            }
        }
        nativeFrameRoute: while rtsp == nil {
            switch evaluateNativeFramePipelineRoute(for: source) {
            case .systemPlayer, .fallback:
                break nativeFrameRoute
            case .waitForSurface:
                pendingNativeFrameSurfaceLoad = true
                pendingAutoPlay = pendingAutoPlay || player == nil
                currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
                try await awaitNativeFrameSurfaceHost(
                    source: source,
                    sourceLoadEpoch: sourceLoadEpoch,
                    deadline: deadline
                )
                continue nativeFrameRoute
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
                        break nativeFrameRoute
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
        }
        let normalizedResource = rtsp == nil ? await openSourceNormalizerResourceIfNeeded(
            for: source,
            sourceLoadEpoch: sourceLoadEpoch
        ) : nil
        if rtsp == nil && normalizedResource == nil && sourceNormalizerConfiguration.mode == .requireNormalized {
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
        if let rtsp {
            url = try await rtsp.playlist(deadline: deadline)
        } else if let normalizedURL = normalizedSession?.playbackURL {
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
        let urlDescription = rtsp == nil ? diagnosticURLDescription(url.absoluteString) : "rtsp-loopback"
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
        if rtsp == nil { preloadCoordinator.warmCurrentSource(source: source, url: url) }
        releaseDashStartupAbrLimitIfNeeded(reason: "sourceReload", item: player?.currentItem)
        try Task.checkCancellation()
        guard isCurrentSourceLoad(sourceLoadEpoch, source: source) else {
            throw CancellationError()
        }
        let item = rtsp != nil ? AVPlayerItem(url: url) : try makePlayerItem(
            for: playbackSource,
            url: url,
            sourceEpoch: subtitleSourceEpoch
        )
        applySubtitleStyle(currentSubtitleStyle, to: item)
        refreshCurrentHdrFailureEvidence(for: playbackSource, item: item)
        let bufferingPolicy = resolvedBufferingPolicy(resolvedResiliencePolicy.buffering)
        item.preferredForwardBufferDuration = bufferingPolicy.preferredForwardBufferDuration
        let player = AVPlayer(playerItem: item)
        player.volume = systemPlayerVolume
        player.isMuted = systemPlayerIsMuted
        player.allowsExternalPlayback = rtsp == nil
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
        if let rtsp {
            rtspHealthTask = Task { @MainActor [weak self, weak rtsp] in
                while !Task.isCancelled {
                    do {
                        try await Task.sleep(for: .milliseconds(500))
                        guard let self, let rtsp, self.currentRtspSession === rtsp else { return }
                        try rtsp.checkHealth()
                    } catch is CancellationError {
                        return
                    } catch {
                        self?.handlePlaybackFailure(error: error, fallbackMessage: "The RTSP bridge stopped.")
                        return
                    }
                }
            }
        }
        activePlayerObservationToken = diagnosticsTracker.capture()
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

        let sourceLoadPlaybackState: PlaybackStateUi =
            !pendingAutoPlay && publishedUiState.playbackState == .paused
                ? .paused
                : .ready
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: normalizedResource.map { "SourceNormalizer \($0.outputRoute)" }
                    ?? sourceSubtitle(for: source),
                sourceLabel: source.label,
                playbackState: sourceLoadPlaybackState,
                playbackRate: $0.playbackRate,
                isBuffering: true,
                isInterrupted: false,
                timeline: TimelineUiState(
                    kind: rtsp == nil ? .vod : .live,
                    isSeekable: rtsp == nil,
                    seekableRange: rtsp == nil ? SeekableRangeUi(startMs: 0, endMs: 0) : nil,
                    liveEdgeMs: nil,
                    positionMs: 0,
                    durationMs: nil
                )
            )
        }
        try await awaitSystemPlayerCommandReadiness(
            source: source,
            sourceLoadEpoch: sourceLoadEpoch,
            player: player,
            item: item,
            playbackEpoch: playbackEpoch,
            deadline: deadline
        )
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
