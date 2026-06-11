@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    var isNativeFrameLoadPending: Bool {
        guard player == nil else { return false }
        if pendingNativeFrameSurfaceLoad {
            return true
        }
        if let activeSession = nativeFramePipelineCoordinator.activeSession,
           !activeSession.didStart {
            return true
        }
        guard sourceLoadTask != nil else {
            return false
        }
        switch nativeFramePipelineConfiguration.mode {
        case .preferNativeFrame, .requireNativeFrame:
            return true
        case .disabled, .diagnosticsOnly:
            return false
        }
    }

    func attachSurfaceHost(_ host: UIView) {
        guard let host = host as? PlayerSurfaceView else {
            return
        }
        let activeNativeSession = nativeFramePipelineCoordinator.activeSession
        if surfaceHost === host {
            host.onReadyForDisplay = { [weak self] in
                Task { @MainActor in
                    self?.handleSurfaceReadyForDisplay()
                }
            }
            if activeNativeSession?.didStart == true, player == nil {
                host.attachNativeFramePresenter()
            } else {
                host.attach(player: player)
            }
            resumePendingNativeFrameSurfaceLoadIfNeeded()
            attemptPendingPlaybackStart(reason: "attachSurfaceHost")
            return
        }

        recordBenchmark("attach_surface_host")
        iosHostLog("attachSurfaceHost")
        surfaceHost?.onReadyForDisplay = nil
        let shouldRebindNativeSession =
            activeNativeSession != nil &&
            activeNativeSession?.surfaceHost !== host
        if shouldRebindNativeSession {
            iosHostLog("native-frame pipeline rebinding after surface host change")
            activeNativeSession?.rebindSurfaceHost(host)
        }
        surfaceHost = host
        host.onReadyForDisplay = { [weak self] in
            Task { @MainActor in
                self?.handleSurfaceReadyForDisplay()
            }
        }
        if activeNativeSession?.didStart == true, player == nil {
            host.attachNativeFramePresenter()
        } else {
            host.attach(player: player)
        }
        resumePendingNativeFrameSurfaceLoadIfNeeded()
        attemptPendingPlaybackStart(reason: "attachSurfaceHost")
    }

    func detachSurfaceHost() {
        detachSurfaceHostIfCurrent(nil)
    }

    func detachSurfaceHost(_ host: UIView) {
        guard let host = host as? PlayerSurfaceView else {
            return
        }
        detachSurfaceHostIfCurrent(host)
    }

    func detachSurfaceHostIfCurrent(_ expectedHost: PlayerSurfaceView?) {
        if let expectedHost, surfaceHost !== expectedHost {
            expectedHost.clearReadyCallback()
            expectedHost.detachBridgeIfNeeded()
            return
        }
        iosHostLog("detachSurfaceHost")
        recordBenchmark("detach_surface_host")
        if let nativeSession = nativeFramePipelineCoordinator.activeSession {
            iosHostLog("native-frame pipeline suspending until surface host reattaches")
            pendingAutoPlay = pendingAutoPlay || nativeSession.isPlaying || publishedUiState.playbackState == .playing
            pendingNativeFrameSurfaceLoad = currentSource != nil
            cancelSourceLoadTask()
            nativeFramePipelineCoordinator.closeActiveSession()
        }
        surfaceHost?.onReadyForDisplay = nil
        surfaceHost?.attach(player: nil)
        surfaceHost = nil
    }

    func play() {
        clearLastError()
        recordBenchmark("play_command")
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
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
            return
        }
        if isNativeFrameLoadPending {
            iosHostLog("play deferred until native-frame load completes")
            pendingAutoPlay = true
            return
        }
        if player == nil {
            pendingAutoPlay = true
            initialize()
            return
        }

        if isSeekingToStartAfterStop {
            iosHostLog("play deferred until stop seek completes")
            pendingPlayAfterStopSeek = true
            return
        }

        iosHostLog("play")
        startPlayback()
        refreshPlaybackState()
    }

    func startPlayback() {
        guard let player else { return }
        recordBenchmark("start_playback_attempt")
        if publishedUiState.playbackState == .finished {
            player.seek(to: .zero)
        }

        if let deferralReason = playbackStartDeferralReason(player) {
            pendingPlaybackStart = true
            recordBenchmark(
                "start_playback_deferred",
                attributes: ["reason": deferralReason]
            )
            iosHostLog("deferring playback until \(deferralReason)")
            return
        }

        pendingPlaybackStart = false
        let rate = desiredPlaybackRate
        applyDefaultPlaybackRate(rate, to: player)
        iosHostLog("startPlayback rate=\(rate)")
        recordBenchmark("start_playback_applied", attributes: ["rate": "\(rate)"])
        player.playImmediately(atRate: rate)
    }

    func pause() {
        clearLastError()
        recordBenchmark("pause_command")
        pendingAutoPlay = false
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
            iosHostLog("pause native-frame")
            nativeSession.pause()
            updateState {
                PlayerHostUiState(
                    title: $0.title,
                    subtitle: $0.subtitle,
                    sourceLabel: $0.sourceLabel,
                    playbackState: .paused,
                    playbackRate: $0.playbackRate,
                    isBuffering: false,
                    isInterrupted: $0.isInterrupted,
                    timeline: $0.timeline
                )
            }
            return
        }
        iosHostLog("pause")
        player?.pause()
        refreshPlaybackState()
    }

    func togglePause() {
        switch publishedUiState.playbackState {
        case .playing:
            pause()
        case .ready, .paused, .finished:
            play()
        }
    }

    func stop() {
        clearLastError()
        recordBenchmark("stop_command")
        pendingAutoPlay = false
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
            iosHostLog("stop native-frame")
            nativeSession.stop()
            let durationMs = nativeSession.durationMs ?? publishedUiState.timeline.durationMs
            updateState {
                PlayerHostUiState(
                    title: $0.title,
                    subtitle: $0.subtitle,
                    sourceLabel: $0.sourceLabel,
                    playbackState: .ready,
                    playbackRate: $0.playbackRate,
                    isBuffering: false,
                    isInterrupted: $0.isInterrupted,
                    timeline: nativeFrameTimelineState(positionMs: 0, durationMs: durationMs)
                )
            }
            return
        }
        if isNativeFrameLoadPending {
            iosHostLog("stop deferred until native-frame load completes")
            pendingNativeFrameSeek = .position(0)
            updateNativeFramePendingSeekTimeline(positionMs: 0)
            updateState {
                PlayerHostUiState(
                    title: $0.title,
                    subtitle: $0.subtitle,
                    sourceLabel: $0.sourceLabel,
                    playbackState: .ready,
                    playbackRate: $0.playbackRate,
                    isBuffering: false,
                    isInterrupted: $0.isInterrupted,
                    timeline: $0.timeline
                )
            }
            return
        }
        iosHostLog("stop")
        releaseDashStartupAbrLimitIfNeeded(reason: "stop", item: player?.currentItem)
        pendingPlayAfterStopSeek = false
        isSeekingToStartAfterStop = true
        let playbackEpoch = currentPlaybackEpoch()
        scheduleStopSeekTimeout(playbackEpoch: playbackEpoch)
        player?.pause()
        player?.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.handleStopSeekCompletion(playbackEpoch: playbackEpoch)
            }
        }
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: 0)
            )
        }
    }

    func seek(by deltaMs: Int64) {
        clearLastError()
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
            let timeline = publishedUiState.timeline
            let target = timeline.clampedPosition(timeline.positionMs + deltaMs)
            iosHostLog("seek(by:) native-frame deltaMs=\(deltaMs) targetMs=\(target)")
            _ = nativeSession.seek(toMs: target)
            return
        }
        if isNativeFrameLoadPending {
            let target = nativeFramePendingRelativeSeekTarget(deltaMs: deltaMs)
            pendingNativeFrameSeek = .position(target)
            updateNativeFramePendingSeekTimeline(positionMs: target)
            iosHostLog("seek(by:) deferred until native-frame surface attaches deltaMs=\(deltaMs) targetMs=\(target)")
            return
        }
        iosHostLog("seek(by:) deltaMs=\(deltaMs)")
        let timeline = publishedUiState.timeline
        let target = timeline.clampedPosition(timeline.positionMs + deltaMs)
        seekToPosition(target)
    }

    func seek(toRatio ratio: Double) {
        clearLastError()
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
            let timeline = publishedUiState.timeline
            let target = timeline.position(forRatio: ratio)
            iosHostLog("seek(toRatio:) native-frame ratio=\(ratio) targetMs=\(target)")
            _ = nativeSession.seek(toMs: target)
            return
        }
        if isNativeFrameLoadPending {
            pendingNativeFrameSeek = .ratio(ratio)
            let target = publishedUiState.timeline.position(forRatio: ratio)
            updateNativeFramePendingSeekTimeline(positionMs: target)
            iosHostLog("seek(toRatio:) deferred until native-frame surface attaches ratio=\(ratio) targetMs=\(target)")
            return
        }
        iosHostLog("seek(toRatio:) ratio=\(ratio)")
        let timeline = publishedUiState.timeline
        let target = timeline.position(forRatio: ratio)
        seekToPosition(target)
    }

    func seekToLiveEdge() {
        clearLastError()
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
            reportCommandError(
                code: .unsupported,
                category: .capability,
                message: "seekToLiveEdge is not implemented for iOS native-frame pipeline yet"
            )
            return
        }
        if isNativeFrameLoadPending {
            iosHostLog("seekToLiveEdge ignored while native-frame pipeline waits for surface")
            return
        }
        let timeline = publishedUiState.timeline
        guard let target = timeline.goLivePositionMs else {
            return
        }
        iosHostLog("seekToLiveEdge targetMs=\(target)")
        seekToPosition(target)
    }

    func setPlaybackRate(_ rate: Float) {
        clearLastError()
        let clampedRate = min(max(rate, 0.5), 3.0)
        iosHostLog("setPlaybackRate rate=\(clampedRate)")
        desiredPlaybackRate = clampedRate
        if let nativeSession = nativeFramePipelineCoordinator.activeSession,
           nativeSession.didStart {
            nativeSession.setPlaybackRate(clampedRate)
            updateState {
                PlayerHostUiState(
                    title: $0.title,
                    subtitle: $0.subtitle,
                    sourceLabel: $0.sourceLabel,
                    playbackState: $0.playbackState,
                    playbackRate: clampedRate,
                    isBuffering: $0.isBuffering,
                    isInterrupted: $0.isInterrupted,
                    timeline: $0.timeline
                )
            }
            return
        }
        if let player {
            applyDefaultPlaybackRate(clampedRate, to: player)
        }
        if publishedUiState.playbackState == .playing {
            player?.playImmediately(atRate: clampedRate)
        }
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: clampedRate,
                isBuffering: $0.isBuffering,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
        refreshPlaybackState()
    }
}
