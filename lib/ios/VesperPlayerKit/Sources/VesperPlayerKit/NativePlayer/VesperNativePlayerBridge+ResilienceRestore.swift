@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func updateState(_ transform: (PlayerHostUiState) -> PlayerHostUiState) {
        publishedUiState = transform(publishedUiState)
    }

    func applyPendingResilienceRestore(
        ifNeededFor item: AVPlayerItem,
        phase: PendingResilienceRestorePhase
    ) {
        guard
            var pendingResilienceRestore,
            currentSource?.uri == pendingResilienceRestore.sourceUri,
            player?.currentItem === item
        else {
            return
        }

        switch phase {
        case .coreState:
            if pendingResilienceRestore.needsCoreStateRestore {
                restoreCorePlaybackState(pendingResilienceRestore.state)
                pendingResilienceRestore.needsCoreStateRestore = false
            }
        case .trackSelection:
            if pendingResilienceRestore.needsTrackSelectionRestore {
                pendingResilienceRestore.needsTrackSelectionRestore =
                    restoreTrackSelectionsIfNeeded(pendingResilienceRestore.state, item: item)
            }
        }

        if
            !pendingResilienceRestore.needsCoreStateRestore &&
                !pendingResilienceRestore.needsTrackSelectionRestore
        {
            self.pendingResilienceRestore = nil
            return
        }

        self.pendingResilienceRestore = pendingResilienceRestore
    }

    func restoreCorePlaybackState(_ state: PreservedPlaybackState) {
        if state.seekToLiveEdge, publishedUiState.timeline.kind == .liveDvr {
            seekToLiveEdge()
        } else if state.restorePosition {
            seekToPosition(max(state.positionMs, 0))
        }

        if abs(state.playbackRate - 1.0) > 0.001 {
            setPlaybackRate(state.playbackRate)
        }

        if !abrPolicyRequiresLoadedVideoVariantCatalog(state.abrPolicy) {
            applyAbrPolicy(
                state.abrPolicy,
                origin: .resilienceRestore,
                clearLastReportedError: false
            )
        }

        if state.shouldResumePlayback {
            play()
        } else if state.playbackState == .paused {
            pause()
        }
    }

    func restoreTrackSelectionsIfNeeded(
        _ state: PreservedPlaybackState,
        item: AVPlayerItem
    ) -> Bool {
        if state.audioSelection.mode != .auto {
            if let group = audioGroup {
                try? applyTrackSelection(
                    state.audioSelection,
                    kind: .audio,
                    group: group,
                    optionsByTrackId: audioOptionsByTrackId,
                    item: item
                )
            }
        }

        if state.subtitleSelection.mode != .auto {
            // A resilience restore must not silently drop a preserved subtitle
            // selection when the legible group
            // or option lookup fails. Surface a structured
            // `subtitle_platform_track_unavailable` /
            // `subtitle_track_not_found` failure so the host can show the
            // user that subtitles could not be restored.
            if let group = subtitleGroup {
                if state.subtitleSelection.mode == .track,
                   let trackId = state.subtitleSelection.trackId,
                   subtitleOptionsByTrackId[trackId] == nil
                {
                    reportSubtitleFailure(
                        code: "subtitle_track_not_found",
                        phase: .selection,
                        trackId: trackId,
                        message: "resilience restore could not locate preserved subtitle trackId=\(trackId)"
                    )
                } else {
                    try? applyTrackSelection(
                        state.subtitleSelection,
                        kind: .subtitle,
                        group: group,
                        optionsByTrackId: subtitleOptionsByTrackId,
                        item: item
                    )
                }
            } else if state.subtitleSelection.mode == .track {
                reportSubtitleFailure(
                    code: "subtitle_platform_track_unavailable",
                    phase: .selection,
                    trackId: state.subtitleSelection.trackId,
                    message: "resilience restore rejected: no legible media selection group for preserved subtitle selection"
                )
            }
        }

        if abrPolicyRequiresLoadedVideoVariantCatalog(state.abrPolicy) {
            applyAbrPolicy(
                state.abrPolicy,
                origin: .resilienceRestore,
                clearLastReportedError: false
            )
        }

        return false
    }

    func canStartPlayback(_ player: AVPlayer) -> Bool {
        playbackStartDeferralReason(player) == nil
    }

    func playbackStartDeferralReason(_ player: AVPlayer) -> String? {
        guard let item = player.currentItem else {
            return "player item is attached"
        }
        switch item.status {
        case .readyToPlay:
            break
        case .failed:
            return "current item recovers from failure"
        case .unknown:
            if currentSource?.protocol != .dash {
                return "current item becomes ready"
            }
        @unknown default:
            return "current item becomes ready"
        }
        if currentSource?.kind == .local, let surfaceHost, !surfaceHost.isReadyForDisplay {
            return "first video frame is ready for display"
        }
        return nil
    }

    func attemptPendingPlaybackStart(reason: String) {
        guard pendingPlaybackStart else {
            return
        }
        guard let player, canStartPlayback(player) else {
            return
        }
        iosHostLog("resuming deferred playback reason=\(reason)")
        startPlayback()
    }

    func handleFixedTrackConvergenceUpdate(
        status: VesperFixedTrackStatus?,
        effectiveVideoTrackId: String?,
        observation: VesperVideoVariantObservation?,
        now: Date
    ) {
        let abrPolicy = publishedTrackSelection.abrPolicy
        guard
            abrPolicy.mode == .fixedTrack,
            let requestedTrackId = abrPolicy.trackId,
            !requestedTrackId.isEmpty
        else {
            fixedTrackConvergenceState = nil
            if fixedTrackIssueActive {
                clearLastError()
            }
            return
        }

        var convergenceState = fixedTrackConvergenceState
        if convergenceState?.requestedTrackId != requestedTrackId {
            convergenceState = FixedTrackConvergenceState(
                requestedTrackId: requestedTrackId,
                origin: convergenceState?.origin ?? .manual
            )
        }

        switch status {
        case .locked:
            if var convergenceState {
                convergenceState.resetMismatch()
                if convergenceState.lockedStartedAt == nil {
                    convergenceState.lockedStartedAt = now
                }
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
            if fixedTrackIssueActive {
                clearLastError()
            }
        case .pending:
            if var convergenceState {
                convergenceState.resetLocked()
                convergenceState.resetMismatch()
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
        case .fallback:
            guard var convergenceState else {
                return
            }
            convergenceState.resetLocked()
            let mismatchSignature = FixedTrackMismatchSignature(
                effectiveVideoTrackId: effectiveVideoTrackId,
                observation: observation
            )
            if convergenceState.mismatchSignature != mismatchSignature {
                convergenceState.mismatchSignature = mismatchSignature
                convergenceState.mismatchStartedAt = now
                convergenceState.hasHandledPersistentMismatch = false
                fixedTrackConvergenceState = convergenceState
                return
            }
            guard let mismatchStartedAt = convergenceState.mismatchStartedAt else {
                convergenceState.mismatchStartedAt = now
                fixedTrackConvergenceState = convergenceState
                return
            }
            let mismatchDuration = now.timeIntervalSince(mismatchStartedAt)
            guard
                !convergenceState.hasHandledPersistentMismatch,
                shouldEscalatePersistentFixedTrackFallback(
                    status: status,
                    observation: observation,
                    playbackState: publishedUiState.playbackState,
                    isBuffering: publishedUiState.isBuffering,
                    elapsed: mismatchDuration
                )
            else {
                fixedTrackConvergenceState = convergenceState
                return
            }

            convergenceState.hasHandledPersistentMismatch = true
            fixedTrackConvergenceState = convergenceState
            reportPersistentFixedTrackMismatch(
                requestedTrackId: requestedTrackId,
                effectiveVideoTrackId: effectiveVideoTrackId,
                observation: observation,
                origin: convergenceState.origin
            )
        case nil:
            if var convergenceState {
                convergenceState.resetLocked()
                convergenceState.resetMismatch()
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
        }
    }

    func reportPersistentFixedTrackMismatch(
        requestedTrackId: String,
        effectiveVideoTrackId: String?,
        observation: VesperVideoVariantObservation?,
        origin: AbrPolicyOrigin
    ) {
        let requestedTrack = publishedTrackCatalog.videoTracks.first { track in
            track.id == requestedTrackId
        }
        let observedTrack = effectiveVideoTrackId.flatMap { effectiveVideoTrackId in
            publishedTrackCatalog.videoTracks.first { track in
                track.id == effectiveVideoTrackId
            }
        }
        let observedDescription = observedVariantDescription(
            observedTrack: observedTrack,
            observation: observation
        )
        let requestedDescription = requestedTrackDescription(
            requestedTrack: requestedTrack,
            fallbackTrackId: requestedTrackId
        )

        let message: String
        switch origin {
        case .resilienceRestore:
            let recoveryPolicy = resolveFixedTrackRecoveryPolicy(
                requestedTrackId: requestedTrackId,
                tracks: publishedTrackCatalog.videoTracks
            )
            applyAbrPolicy(
                recoveryPolicy,
                origin: .recoveredFallback,
                clearLastReportedError: false
            )
            switch recoveryPolicy.mode {
            case .constrained:
                message = VesperPlayerI18n.fixedTrackRestoreFallbackConstrained(
                    requested: requestedDescription,
                    fallback: abrPolicyDescription(recoveryPolicy),
                    observed: observedDescription
                )
            case .auto, .fixedTrack:
                message = VesperPlayerI18n.fixedTrackRestoreFallbackAuto(
                    requested: requestedDescription,
                    observed: observedDescription
                )
            }
        case .manual, .defaultPolicy, .recoveredFallback:
            message = VesperPlayerI18n.fixedTrackMismatch(
                requested: requestedDescription,
                observed: observedDescription
            )
        }

        iosHostLog(
            "fixedTrackMismatch requested=\(requestedTrackId) effective=\(effectiveVideoTrackId ?? "nil") origin=\(origin.rawValue) message=\(message)"
        )
        fixedTrackIssueActive = true
        publishedLastError = VesperPlayerError(
            message: message,
            code: .invalidState,
            category: .playback,
            retriable: false
        )
    }

    func requestedTrackDescription(
        requestedTrack: VesperMediaTrack?,
        fallbackTrackId: String
    ) -> String {
        if let label = requestedTrack?.label, !label.isEmpty {
            return label
        }
        if let requestedTrack {
            return trackObservationDescription(requestedTrack)
        }
        return fallbackTrackId
    }

    func observedVariantDescription(
        observedTrack: VesperMediaTrack?,
        observation: VesperVideoVariantObservation?
    ) -> String {
        if let observedTrack {
            if let observationDescription = observationDescription(observation) {
                return "\(trackObservationDescription(observedTrack)) (\(observationDescription))"
            }
            return trackObservationDescription(observedTrack)
        }
        return observationDescription(observation) ?? "an unknown adaptive variant"
    }

    func trackObservationDescription(_ track: VesperMediaTrack) -> String {
        if let label = track.label, !label.isEmpty {
            return label
        }

        var components: [String] = []
        if let width = track.width, let height = track.height {
            components.append("\(width)x\(height)")
        }
        if let bitRate = track.bitRate {
            components.append(formattedBitRate(bitRate))
        }
        if !components.isEmpty {
            return components.joined(separator: " · ")
        }
        return track.id
    }

    func observationDescription(_ observation: VesperVideoVariantObservation?) -> String? {
        guard let observation else {
            return nil
        }

        var components: [String] = []
        if let width = observation.width, let height = observation.height {
            components.append("\(width)x\(height)")
        }
        if let bitRate = observation.bitRate {
            components.append(formattedBitRate(bitRate))
        }
        return components.isEmpty ? nil : components.joined(separator: " · ")
    }

    func formattedBitRate(_ bitRate: Int64) -> String {
        let bitRateDouble = Double(bitRate)
        if bitRateDouble >= 1_000_000 {
            let value = (bitRateDouble / 100_000).rounded() / 10
            return String(format: "%.1f Mbps", locale: Locale.current, value)
        }
        if bitRateDouble >= 1_000 {
            let value = (bitRateDouble / 100).rounded() / 10
            return String(format: "%.1f Kbps", locale: Locale.current, value)
        }
        return "\(bitRate) bps"
    }

    func abrPolicyDescription(_ policy: VesperAbrPolicy) -> String {
        switch policy.mode {
        case .constrained:
            var components: [String] = []
            if let maxHeight = policy.maxHeight {
                components.append("\(maxHeight)p")
            } else if let maxWidth = policy.maxWidth {
                components.append("\(maxWidth)w")
            }
            if let maxBitRate = policy.maxBitRate {
                components.append(formattedBitRate(maxBitRate))
            }
            return components.isEmpty ? "automatic ABR" : components.joined(separator: " · ")
        case .auto:
            return "automatic ABR"
        case .fixedTrack:
            return policy.trackId ?? "fixed track"
        }
    }
}
