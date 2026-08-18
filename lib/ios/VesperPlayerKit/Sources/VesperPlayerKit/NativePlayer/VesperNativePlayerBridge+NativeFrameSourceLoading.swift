@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func configureNativeFramePlayback(
        source: VesperPlayerSource,
        session: VesperNativeFramePipelineSession
    ) {
        iosHostLog(
            "configured iOS native-frame pipeline source=\(diagnosticURLDescription(source.uri))"
        )
        recordBenchmark("native_frame_pipeline_configured")
        releaseDashStartupAbrLimitIfNeeded(reason: "nativeFrameSourceReload", item: player?.currentItem)
        removeObservers()
        player?.pause()
        player = nil
        surfaceHost?.attachNativeFramePresenter()
        pendingPlaybackStart = false
        hasAppliedDefaultTrackPreferences = false
        resetTrackState()
        _ = advancePlaybackEpoch()
        currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: "Native Frame Pipeline (SDK video and native audio)",
                sourceLabel: source.label,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: false,
                timeline: nativeFrameTimelineState(positionMs: 0, durationMs: session.durationMs)
            )
        }
        session.onFramePresented = { [weak self] timeline in
            guard let self else { return }
            self.updateNativeFrameTimeline(timeline)
        }
        session.onPlaybackEnded = { [weak self] in
            guard let self else { return }
            self.handlePlaybackEnded()
        }
        session.onPlaybackFailed = { [weak self] issue in
            guard let self else { return }
            self.handlePlaybackFailure(
                error: nil,
                fallbackMessage: issue.message
            )
        }
        let shouldPlayAfterPendingSeek = pendingAutoPlay
        if applyPendingNativeFrameSeekIfNeeded(session: session, playAfterSeek: shouldPlayAfterPendingSeek) {
            pendingAutoPlay = false
            return
        }
        if pendingAutoPlay {
            pendingAutoPlay = false
            startNativeFrameSessionPlayback(session)
        }
    }

    func startNativeFrameSessionPlayback(_ session: VesperNativeFramePipelineSession) {
        guard session.play(rate: desiredPlaybackRate) else { return }
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

    func updateNativeFramePendingSeekTimeline(positionMs: Int64) {
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: $0.isBuffering,
                isInterrupted: $0.isInterrupted,
                timeline: nativeFrameTimelineState(
                    positionMs: positionMs,
                    durationMs: $0.timeline.durationMs
                )
            )
        }
    }

    func nativeFramePendingRelativeSeekTarget(deltaMs: Int64) -> Int64 {
        let timeline = publishedUiState.timeline
        let proposed = timeline.positionMs + deltaMs
        let hasResolvedWindow =
            timeline.seekableRange.map { $0.endMs > $0.startMs } == true ||
            (timeline.durationMs ?? 0) > 0
        if hasResolvedWindow {
            return timeline.clampedPosition(proposed)
        }
        return max(proposed, 0)
    }

    func applyPendingNativeFrameSeekIfNeeded(
        session: VesperNativeFramePipelineSession,
        playAfterSeek: Bool
    ) -> Bool {
        guard let pendingSeek = pendingNativeFrameSeek else { return false }
        pendingNativeFrameSeek = nil
        let timeline = nativeFrameTimelineState(
            positionMs: publishedUiState.timeline.positionMs,
            durationMs: session.durationMs ?? publishedUiState.timeline.durationMs
        )
        let target = pendingSeek.resolve(using: timeline)
        iosHostLog("applying pending native-frame seek targetMs=\(target)")
        _ = session.seek(toMs: target) { [weak self, weak session] _ in
            guard playAfterSeek, let self, let session, !session.isClosed else { return }
            self.startNativeFrameSessionPlayback(session)
        }
        return true
    }

    func updateNativeFrameTimeline(_ timeline: VesperNativeFramePipelineTimeline) {
        let durationMs = timeline.durationMs ?? publishedUiState.timeline.durationMs
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: nativeFrameTimelineState(
                    positionMs: timeline.positionMs,
                    durationMs: durationMs
                )
            )
        }
        if let durationMs,
           durationMs > 0,
           timeline.positionMs >= durationMs {
            handlePlaybackEnded()
        }
    }

    func nativeFrameTimelineState(positionMs: Int64, durationMs: Int64?) -> TimelineUiState {
        let clampedDuration = durationMs.flatMap { $0 > 0 ? $0 : nil }
        let clampedPosition = if let clampedDuration {
            min(max(positionMs, 0), clampedDuration)
        } else {
            max(positionMs, 0)
        }
        return TimelineUiState(
            kind: .vod,
            isSeekable: clampedDuration != nil,
            seekableRange: clampedDuration.map { SeekableRangeUi(startMs: 0, endMs: $0) },
            liveEdgeMs: nil,
            positionMs: clampedPosition,
            durationMs: clampedDuration
        )
    }
}
