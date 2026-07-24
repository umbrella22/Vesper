@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func seekToPosition(_ positionMs: Int64) {
        let playbackEpoch = currentPlaybackEpoch()
        let time = CMTime(milliseconds: positionMs)
        recordBenchmark("seek_start", attributes: ["positionMs": "\(positionMs)"])
        player?.seek(to: time) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.handleSeekCompletion(positionMs: positionMs, playbackEpoch: playbackEpoch)
            }
        }
    }

    func installObservers(for player: AVPlayer, item: AVPlayerItem, playbackEpoch: UInt64) {
        timeObserverToken = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.25, preferredTimescale: 600),
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    iosHostLog("ignored stale time observer playbackEpoch=\(playbackEpoch)")
                    return
                }
                self.refreshPlaybackState(timelineOnly: true)
            }
        }

        timeControlObservation = player.observe(\.timeControlStatus, options: [.initial, .new]) { [weak self] player, _ in
            let reason = player.reasonForWaitingToPlay?.rawValue ?? "nil"
            iosHostLog(
                "timeControlStatus=\(timeControlStatusName(player.timeControlStatus)) reason=\(reason) rate=\(player.rate)"
            )
            Task { @MainActor in
                guard let self, self.isPlaybackEpochCurrent(playbackEpoch) else {
                    return
                }
                self.recordBenchmark(
                    "time_control_status_changed",
                    attributes: [
                        "status": timeControlStatusName(player.timeControlStatus),
                        "reason": reason,
                    ]
                )
            }
        }

        itemStatusObservation = item.observe(\.status, options: [.initial, .new]) { [weak self] item, _ in
            let errorMessage = item.error?.localizedDescription ?? "nil"
            iosHostLog("itemStatus=\(itemStatusName(item.status)) error=\(errorMessage)")
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    iosHostLog("ignored stale item status playbackEpoch=\(playbackEpoch)")
                    return
                }
                self.recordBenchmark(
                    "player_item_status_changed",
                    attributes: [
                        "status": itemStatusName(item.status),
                        "error": errorMessage,
                    ]
                )
                switch item.status {
                case .readyToPlay:
                    self.recordBenchmark("player_item_ready")
                    self.cancelPendingRetry(resetAttempts: true)
                    self.refreshTrackCatalogAndSelection(for: item)
                    Task { @MainActor [weak self, weak item] in
                        guard let self, let item else { return }
                        await self.applyPendingResilienceRestore(
                            ifNeededFor: item,
                            phase: .coreState
                        )
                    }
                    self.attemptPendingPlaybackStart(reason: "itemReadyToPlay")
                    self.refreshPlaybackState()
                case .failed:
                    self.pendingPlaybackStart = false
                    let itemStatusDetails = playerItemStatusDetails(item.status)
                    let itemErrorLogDetails = playerItemErrorLogDetails(item)
                    self.handlePlaybackFailure(
                        error: item.error,
                        fallbackMessage: errorMessage,
                        itemStatusDetails: itemStatusDetails,
                        itemErrorLogDetails: itemErrorLogDetails
                    )
                case .unknown:
                    break
                @unknown default:
                    break
                }
            }
        }

        itemBufferEmptyObservation = item.observe(\.isPlaybackBufferEmpty, options: [.initial, .new]) { [weak self] item, _ in
            iosHostLog("itemBufferEmpty=\(item.isPlaybackBufferEmpty)")
            Task { @MainActor in
                guard let self, self.isPlaybackEpochCurrent(playbackEpoch) else {
                    return
                }
                self.recordBenchmark(
                    "buffer_empty_changed",
                    attributes: ["empty": "\(item.isPlaybackBufferEmpty)"]
                )
            }
        }

        itemLikelyToKeepUpObservation = item.observe(\.isPlaybackLikelyToKeepUp, options: [.initial, .new]) {
            [weak self] item, _
            in
            iosHostLog("itemLikelyToKeepUp=\(item.isPlaybackLikelyToKeepUp)")
            guard let self else { return }
            if item.isPlaybackLikelyToKeepUp {
                Task { @MainActor in
                    guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                        iosHostLog("ignored stale likelyToKeepUp playbackEpoch=\(playbackEpoch)")
                        return
                    }
                    self.recordBenchmark(
                        "likely_to_keep_up_changed",
                        attributes: ["likely": "\(item.isPlaybackLikelyToKeepUp)"]
                    )
                    self.attemptPendingPlaybackStart(reason: "itemLikelyToKeepUp")
                }
            } else {
                Task { @MainActor in
                    guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                        return
                    }
                    self.recordBenchmark(
                        "likely_to_keep_up_changed",
                        attributes: ["likely": "\(item.isPlaybackLikelyToKeepUp)"]
                    )
                }
            }
        }

        endObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime,
            object: item,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    iosHostLog("ignored stale ended observer playbackEpoch=\(playbackEpoch)")
                    return
                }
                self.handlePlaybackEnded()
            }
        }

        playbackStalledObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemPlaybackStalled,
            object: item,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    return
                }
                self.recordBenchmark("playback_stalled")
            }
        }

        refreshTrackCatalogAndSelection(for: item)
    }

    func removeObservers() {
        if let token = timeObserverToken, let player {
            player.removeTimeObserver(token)
        }
        timeObserverToken = nil
        timeControlObservation = nil
        itemStatusObservation = nil
        itemBufferEmptyObservation = nil
        itemLikelyToKeepUpObservation = nil

        if let endObserver {
            NotificationCenter.default.removeObserver(endObserver)
        }
        endObserver = nil
        if let playbackStalledObserver {
            NotificationCenter.default.removeObserver(playbackStalledObserver)
        }
        playbackStalledObserver = nil
    }

    func advancePlaybackEpoch() -> UInt64 {
        playbackEpoch &+= 1
        readyForDisplayCountByEpoch = [playbackEpoch: readyForDisplayCountByEpoch[playbackEpoch] ?? 0]
        return playbackEpoch
    }

    func currentPlaybackEpoch() -> UInt64 {
        playbackEpoch
    }

    func playbackEpochSnapshot() -> UInt64 {
        playbackEpoch
    }

    func readyForDisplayEpochCountSnapshot() -> Int {
        readyForDisplayCountByEpoch.count
    }

    func stopSeekStateSnapshot() -> StopSeekStateSnapshot {
        StopSeekStateSnapshot(
            isSeekingToStartAfterStop: isSeekingToStartAfterStop,
            pendingPlayAfterStopSeek: pendingPlayAfterStopSeek
        )
    }

    func isPlaybackEpochCurrent(_ capturedPlaybackEpoch: UInt64) -> Bool {
        capturedPlaybackEpoch == playbackEpoch
    }

    func handleSeekCompletion(positionMs: Int64, playbackEpoch: UInt64) {
        guard isPlaybackEpochCurrent(playbackEpoch) else {
            iosHostLog(
                "ignored stale seek completion playbackEpoch=\(playbackEpoch) current=\(self.playbackEpoch) positionMs=\(positionMs)"
            )
            return
        }
        recordBenchmark("seek_completed", attributes: ["positionMs": "\(positionMs)"])
        updateTimelinePosition(positionMs)
        refreshPlaybackState()
    }

    func handleStopSeekCompletion(playbackEpoch: UInt64) {
        guard isPlaybackEpochCurrent(playbackEpoch) else {
            iosHostLog(
                "ignored stale stop seek completion playbackEpoch=\(playbackEpoch) current=\(self.playbackEpoch)"
            )
            return
        }
        iosHostLog("stop seek completed")
        recordBenchmark("stop_seek_completed")
        cancelStopSeekTimeout()
        isSeekingToStartAfterStop = false
        updateTimelinePosition(0)
        if pendingPlayAfterStopSeek {
            pendingPlayAfterStopSeek = false
            iosHostLog("resuming deferred play after stop seek")
            startPlayback()
        }
        refreshPlaybackState()
    }

    func handleSurfaceReadyForDisplay() {
        let playbackEpoch = currentPlaybackEpoch()
        let readyCount = (readyForDisplayCountByEpoch[playbackEpoch] ?? 0) + 1
        readyForDisplayCountByEpoch[playbackEpoch] = readyCount
        let isFirstForEpoch = firstFrameRenderedPlaybackEpoch != playbackEpoch

        iosHostLog("surfaceReadyForDisplay epoch=\(playbackEpoch) firstForEpoch=\(isFirstForEpoch)")
        recordBenchmark(
            "ready_for_display",
            attributes: [
                "playbackEpoch": "\(playbackEpoch)",
                "readyCount": "\(readyCount)",
                "isFirstForEpoch": "\(isFirstForEpoch)",
            ]
        )

        if isFirstForEpoch {
            firstFrameRenderedPlaybackEpoch = playbackEpoch
            recordBenchmark(
                "first_frame_rendered",
                attributes: ["playbackEpoch": "\(playbackEpoch)"]
            )
            releaseDashStartupAbrLimitIfNeeded(reason: "firstFrameRendered", item: nil)
        }

        attemptPendingPlaybackStart(reason: "surfaceReadyForDisplay")
    }

    func handlePlaybackEnded() {
        recordBenchmark("playback_ended")
        let durationMs = currentDurationMs() ?? publishedUiState.timeline.durationMs ?? 0
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: .finished,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: durationMs)
            )
        }
    }

    func refreshPlaybackState(timelineOnly: Bool = false) {
        VesperPlaybackTrace.interval("VesperRefresh#refreshPlaybackState") {
            refreshPlaybackStateBody(timelineOnly: timelineOnly)
        }
    }

    private func refreshPlaybackStateBody(timelineOnly: Bool) {
        guard let player else {
            return
        }

        let previousPlaybackState = publishedUiState.playbackState
        let previousBuffering = publishedUiState.isBuffering
        let durationMs = currentDurationMs()
        let positionMs = player.currentTime().milliseconds
        subtitleOverlayRenderer.render(positionMs: positionMs)
        let buffering = player.timeControlStatus == .waitingToPlayAtSpecifiedRate
        let playbackState = derivePlaybackState(
            currentState: publishedUiState.playbackState,
            player: player,
            durationMs: durationMs,
            positionMs: positionMs
        )

        // A periodic time observer may also discover a playback transition.
        // Only suppress Flutter's full snapshot when the presentation state
        // is unchanged; playing/paused/buffering transitions must remain
        // visible to the host.
        timelineOnlyUpdatePending = timelineOnly &&
            previousPlaybackState == playbackState &&
            previousBuffering == buffering
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: buffering,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: positionMs)
            )
        }
        // Effective ABR observation is intentionally kept on the native time
        // observer. It only publishes when the selected variant changes and
        // remains the convergence clock for fixed-track diagnostics. A change
        // emits its own full controller update after the timeline marker is
        // consumed.
        VesperPlaybackTrace.interval("VesperRefresh#effectiveVideoObservation") {
            refreshEffectiveVideoTrackObservation(for: player.currentItem)
        }
    }

    func updateTimelinePosition(_ positionMs: Int64) {
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: $0.isBuffering,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: positionMs)
            )
        }
    }

    func currentTimelineState(positionMs explicitPositionMs: Int64? = nil) -> TimelineUiState {
        let durationMs = currentDurationMs()
        let item = player?.currentItem
        let seekableRange = currentSeekableRange(item: item, durationMs: durationMs)
        let kind = currentTimelineKind(durationMs: durationMs, seekableRange: seekableRange)
        let seekableRangeStartMs = seekableRange?.startMs ?? 0
        let seekableRangeEndMs = seekableRange?.endMs ?? 0
        let hasSeekableWindow = seekableRangeEndMs > seekableRangeStartMs

        let liveEdgeMs: Int64?
        switch kind {
        case .vod:
            liveEdgeMs = nil
        case .live, .liveDvr:
            liveEdgeMs = seekableRange?.endMs
        }

        let isSeekable: Bool
        switch kind {
        case .vod, .liveDvr:
            isSeekable = hasSeekableWindow
        case .live:
            isSeekable = false
        }

        let currentPositionMs = player?.currentTime().milliseconds
        let rawPositionMs = explicitPositionMs ?? currentPositionMs ?? publishedUiState.timeline.positionMs
        let clampedPositionMs: Int64
        if let seekableRange, seekableRange.endMs >= seekableRange.startMs {
            clampedPositionMs = min(max(rawPositionMs, seekableRange.startMs), seekableRange.endMs)
        } else {
            clampedPositionMs = max(rawPositionMs, 0)
        }

        let uiDurationMs: Int64?
        switch kind {
        case .vod:
            uiDurationMs = durationMs
        case .live:
            uiDurationMs = nil
        case .liveDvr:
            uiDurationMs = seekableRange.map { max($0.endMs - $0.startMs, 0) }
        }

        return TimelineUiState(
            kind: kind,
            isSeekable: isSeekable,
            seekableRange: isSeekable ? seekableRange : nil,
            liveEdgeMs: liveEdgeMs,
            positionMs: clampedPositionMs,
            durationMs: uiDurationMs
        )
    }

    func currentTimelineKind(
        durationMs: Int64?,
        seekableRange: SeekableRangeUi?
    ) -> TimelineKindUi {
        if let durationMs, durationMs > 0 {
            return .vod
        }

        guard currentSource?.kind == .remote, currentSource?.protocol == .hls else {
            return .vod
        }

        if let seekableRange, seekableRange.endMs > seekableRange.startMs {
            return .liveDvr
        }

        return .live
    }

    func currentSeekableRange(
        item: AVPlayerItem?,
        durationMs: Int64?
    ) -> SeekableRangeUi? {
        if let item {
            let ranges = item.seekableTimeRanges.compactMap { value -> SeekableRangeUi? in
                let timeRange = value.timeRangeValue
                guard
                    let startMs = timeRange.start.finiteMilliseconds,
                    let endMs = CMTimeAdd(timeRange.start, timeRange.duration).finiteMilliseconds,
                    endMs >= startMs
                else {
                    return nil
                }
                return SeekableRangeUi(startMs: startMs, endMs: endMs)
            }
            if let widestRange = ranges.max(by: { ($0.endMs - $0.startMs) < ($1.endMs - $1.startMs) }) {
                return widestRange
            }
        }

        return normalizedSeekableRange(durationMs: durationMs)
    }

    func currentDurationMs() -> Int64? {
        player?.currentItem?.duration.finiteMilliseconds
    }
}
