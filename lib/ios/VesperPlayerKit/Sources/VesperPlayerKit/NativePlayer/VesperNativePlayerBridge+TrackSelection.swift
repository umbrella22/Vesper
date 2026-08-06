@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func setVideoTrackSelection(_ selection: VesperTrackSelection) {
        let trackIdText = selection.trackId ?? "nil"
        reportCommandError(
            code: .unsupported,
            category: .capability,
            message:
                "setVideoTrackSelection is not implemented on iOS AVPlayer (mode=\(selection.mode.rawValue), trackId=\(trackIdText))"
        )
    }

    func setAudioTrackSelection(_ selection: VesperTrackSelection) {
        clearLastError()
        iosHostLog("setAudioTrackSelection mode=\(selection.mode.rawValue)")
        guard let item = player?.currentItem else {
            iosHostLog("setAudioTrackSelection ignored: no current item")
            return
        }

        guard let group = audioGroup else {
            iosHostLog("setAudioTrackSelection ignored: no audible media selection group")
            return
        }

        try? applyTrackSelection(
            selection,
            kind: .audio,
            group: group,
            optionsByTrackId: audioOptionsByTrackId,
            item: item
        )
    }

    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) async throws {
        try await coordinateSubtitleSelection(selection, origin: .explicit)
    }

    func coordinateSubtitleSelection(
        _ selection: VesperTrackSelection,
        origin: SubtitleSelectionOrigin
    ) async throws {
        clearLastError()
        iosHostLog("setSubtitleTrackSelection mode=\(selection.mode.rawValue)")
        if origin == .explicit {
            explicitSubtitleIntentSourceEpoch = subtitleSourceEpoch
        } else if origin == .defaultPolicy,
                  explicitSubtitleIntentSourceEpoch == subtitleSourceEpoch {
            return
        } else if origin == .visibilityRestore,
                  selection != publishedConfirmedSubtitleSelection {
            return
        }
        if let pendingSubtitleSelection,
           !origin.canSupersede(pendingSubtitleSelection.origin) {
            return
        }
        subtitleSelectionTask?.cancel()
        pendingSubtitleSelection = nil
        nextSubtitleCommandId &+= 1
        let commandId = nextSubtitleCommandId
        let sourceEpoch = subtitleSourceEpoch
        clearSubtitleFailure()
        publishedRequestedSubtitleSelection = selection
        updateTrackSelection { current in
            VesperTrackSelectionSnapshot(
                video: current.video,
                audio: current.audio,
                subtitle: selection,
                confirmedSubtitle: current.confirmedSubtitle,
                effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                abrPolicy: current.abrPolicy
            )
        }
        publishedSubtitleState = VesperSubtitleState(
            catalogState: publishedSubtitleState.catalogState,
            selectionState: .applying,
            advertisedTrackCount: publishedSubtitleState.advertisedTrackCount,
            selectableTrackCount: publishedSubtitleState.selectableTrackCount,
            catalogError: publishedSubtitleState.catalogError,
            selectionError: nil
        )

        guard let item = player?.currentItem else {
            let failure = VesperSubtitleSelectionError.platformTrackUnavailable(
                trackId: selection.trackId
            )
            let commandError = VesperSubtitleSelectionCommandError(
                failure: failure,
                commandId: commandId,
                sourceEpoch: sourceEpoch
            )
            reportSubtitleFailure(
                code: commandError.code,
                phase: .selection,
                trackId: commandError.trackId,
                retriable: commandError.retriable,
                message: commandError.localizedDescription,
                commandId: commandId,
                sourceEpoch: sourceEpoch
            )
            throw commandError
        }

        // A single pending transaction keeps ownership and cancellation
        // bounded. The previous task is cancelled before the new command is
        // registered, so an old AVPlayer callback cannot publish a new state.
        let playbackEpoch = currentPlaybackEpoch()
        let pending = PendingSubtitleSelection(
            commandId: commandId,
            sourceEpoch: sourceEpoch,
            playbackEpoch: playbackEpoch,
            item: item,
            selection: selection,
            origin: origin
        )
        pendingSubtitleSelection = pending

        let task = Task { @MainActor [weak self] in
            guard let self else { throw CancellationError() }
            try await self.performSubtitleSelectionTransaction(pending)
        }
        subtitleSelectionTask = task
        defer {
            if pendingSubtitleSelection?.commandId == commandId {
                pendingSubtitleSelection = nil
                subtitleSelectionTask = nil
            }
        }

        do {
            try await task.value
        } catch {
            if let failure = subtitleSelectionInvalidationFailure(for: pending) {
                throw VesperSubtitleSelectionCommandError(
                    failure: failure,
                    commandId: commandId,
                    sourceEpoch: sourceEpoch
                )
            }
            if error is CancellationError {
                let failure = VesperSubtitleSelectionError.selectionCancelled(
                    trackId: selection.trackId
                )
                throw VesperSubtitleSelectionCommandError(
                    failure: failure,
                    commandId: commandId,
                    sourceEpoch: sourceEpoch
                )
            }
            guard let failure = error as? VesperSubtitleSelectionError else {
                restoreConfirmedSubtitleBackendIfPossible(on: item)
                throw error
            }
            restoreConfirmedSubtitleBackendIfPossible(on: item)
            let commandError = VesperSubtitleSelectionCommandError(
                failure: failure,
                commandId: commandId,
                sourceEpoch: sourceEpoch
            )
            reportSubtitleFailure(
                code: commandError.code,
                phase: .selection,
                trackId: commandError.trackId,
                retriable: commandError.retriable,
                message: commandError.localizedDescription,
                commandId: commandId,
                sourceEpoch: sourceEpoch
            )
            throw commandError
        }

    }

    private func performSubtitleSelectionTransaction(
        _ pending: PendingSubtitleSelection
    ) async throws {
        guard isCurrentSubtitleSelection(pending) else {
            throw CancellationError()
        }
        if pending.selection.mode != .disabled {
            try await waitForSubtitleCatalogReadiness(pending: pending)
        }
        if let failure = subtitleCatalogSelectionFailure(for: pending.selection) {
            throw failure
        }

        let item = pending.item
        let target: SubtitleSelectionTarget
        switch pending.selection.mode {
        case .disabled:
            target = .disabled
        case .auto:
            let availableTracks = publishedTrackCatalog.subtitleTracks.filter { track in
                subtitleOverlayRenderer.containsTrack(track.id)
                    || subtitleOptionsByTrackId[track.id] != nil
            }
            guard let trackId = resolveAutomaticSubtitleTrackId(
                tracks: availableTracks,
                preferredLanguage: resolvedTrackPreferencePolicy.preferredSubtitleLanguage,
                selectUndeterminedLanguage:
                    resolvedTrackPreferencePolicy.selectUndeterminedSubtitleLanguage,
                allowDefaultCandidate: automaticSubtitleSelectionAllowsDefaultCandidate(
                    origin: pending.origin,
                    startupPolicySelectsSubtitlesByDefault:
                        resolvedTrackPreferencePolicy.selectSubtitlesByDefault
                )
            ) else {
                throw VesperSubtitleSelectionError.autoCandidateUnavailable
            }
            if subtitleOverlayRenderer.containsTrack(trackId) {
                target = .overlay(trackId)
            } else if let option = subtitleOptionsByTrackId[trackId] {
                guard subtitleGroup != nil else {
                    throw VesperSubtitleSelectionError.platformTrackUnavailable(trackId: trackId)
                }
                target = .native(option: option, trackId: trackId)
            } else {
                throw VesperSubtitleSelectionError.platformTrackUnavailable(trackId: trackId)
            }
        case .track:
            guard let trackId = pending.selection.trackId else {
                throw VesperSubtitleSelectionError.trackNotFound(trackId: "nil")
            }
            if subtitleOverlayRenderer.containsTrack(trackId) {
                target = .overlay(trackId)
            } else if let option = subtitleOptionsByTrackId[trackId] {
                target = .native(option: option, trackId: trackId)
            } else {
                throw VesperSubtitleSelectionError.trackNotFound(trackId: trackId)
            }
        }

        switch target {
        case .disabled:
            var group = subtitleGroup
            if group == nil,
               publishedSubtitleState.catalogState == .loading {
                group = try await waitForSubtitleGroup(pending: pending)
            }
            if let group {
                item.select(nil, in: group)
                try await waitForSubtitleOption(
                    nil,
                    in: group,
                    pending: pending
                )
            }
            guard subtitleOverlayRenderer.select(trackId: nil) else {
                throw VesperSubtitleSelectionError.selectionDidNotConverge(trackId: nil)
            }
            commitSubtitleSelection(pending, effectiveTrackId: nil)
        case let .overlay(trackId):
            if let group = subtitleGroup {
                item.select(nil, in: group)
                try await waitForSubtitleOption(
                    nil,
                    in: group,
                    pending: pending
                )
            }
            guard subtitleOverlayRenderer.select(trackId: trackId) else {
                throw VesperSubtitleSelectionError.trackNotFound(
                    trackId: pending.selection.trackId ?? ""
                )
            }
            commitSubtitleSelection(pending, effectiveTrackId: trackId)
        case let .native(option, trackId):
            guard let group = subtitleGroup else {
                throw VesperSubtitleSelectionError.platformTrackUnavailable(
                    trackId: pending.selection.trackId
                )
            }
            item.select(option, in: group)
            try await waitForSubtitleOption(option, in: group, pending: pending)
            guard subtitleOverlayRenderer.select(trackId: nil) else {
                throw VesperSubtitleSelectionError.selectionDidNotConverge(
                    trackId: pending.selection.trackId
                )
            }
            commitSubtitleSelection(pending, effectiveTrackId: trackId)
        }
        enforceSubtitleVisibility(for: item)
    }

    private enum SubtitleSelectionTarget {
        case disabled
        case overlay(String?)
        case native(option: AVMediaSelectionOption, trackId: String?)
    }

    func subtitleCatalogSelectionFailure(
        for selection: VesperTrackSelection
    ) -> VesperSubtitleSelectionError? {
        guard selection.mode != .disabled else { return nil }
        if publishedSubtitleState.catalogState == .failed {
            let error = publishedSubtitleState.catalogError
            return .catalogUnavailable(
                code: error?.code ?? "subtitle_platform_track_unavailable",
                trackId: selection.trackId ?? error?.trackId,
                phase: error?.phase ?? .discovery,
                phaseRawValue: error?.phaseRawValue,
                message: error?.message ?? "The subtitle catalog is unavailable.",
                retriable: error?.retriable ?? false
            )
        }
        if selection.mode == .track,
           let trackId = selection.trackId,
           !publishedTrackCatalog.subtitleTracks.contains(where: { $0.id == trackId }) {
            return .trackNotFound(trackId: trackId)
        }
        return nil
    }

    private func restoreConfirmedSubtitleBackendIfPossible(on item: AVPlayerItem) {
        let effectiveTrackId = publishedEffectiveSubtitleTrackId
        if let effectiveTrackId,
           subtitleOverlayRenderer.containsTrack(effectiveTrackId) {
            if let subtitleGroup {
                item.select(nil, in: subtitleGroup)
            }
            _ = subtitleOverlayRenderer.select(trackId: effectiveTrackId)
            return
        }
        if let effectiveTrackId,
           let option = subtitleOptionsByTrackId[effectiveTrackId],
           let subtitleGroup {
            item.select(option, in: subtitleGroup)
            _ = subtitleOverlayRenderer.select(trackId: nil)
            return
        }
        if publishedConfirmedSubtitleSelection.mode == .disabled {
            if let subtitleGroup {
                item.select(nil, in: subtitleGroup)
            }
            _ = subtitleOverlayRenderer.select(trackId: nil)
        }
    }

    private func waitForSubtitleCatalogReadiness(
        pending: PendingSubtitleSelection
    ) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: subtitleSelectionWaitPolicy.timeout)
        while publishedSubtitleState.catalogState == .loading {
            guard isCurrentSubtitleSelection(pending) else {
                throw CancellationError()
            }
            guard clock.now < deadline else {
                throw VesperSubtitleSelectionError.selectionTimedOut(
                    trackId: pending.selection.trackId
                )
            }
            try await clock.sleep(for: subtitleSelectionWaitPolicy.pollInterval)
        }
    }

    private func waitForSubtitleOption(
        _ expected: AVMediaSelectionOption?,
        in group: AVMediaSelectionGroup,
        pending: PendingSubtitleSelection
    ) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: subtitleSelectionWaitPolicy.timeout)
        while true {
            guard isCurrentSubtitleSelection(pending) else {
                throw CancellationError()
            }
            let actual = pending.item.currentMediaSelection.selectedMediaOption(in: group)
            if (expected == nil && actual == nil) || (expected != nil && actual === expected) {
                return
            }
            if pending.item.status == .failed {
                throw VesperSubtitleSelectionError.selectionDidNotConverge(
                    trackId: pending.selection.trackId
                )
            }
            guard clock.now < deadline else {
                throw VesperSubtitleSelectionError.selectionTimedOut(
                    trackId: pending.selection.trackId
                )
            }
            try await clock.sleep(for: subtitleSelectionWaitPolicy.pollInterval)
        }
    }

    private func waitForSubtitleGroup(
        pending: PendingSubtitleSelection
    ) async throws -> AVMediaSelectionGroup? {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: subtitleSelectionWaitPolicy.timeout)
        while subtitleGroup == nil {
            guard isCurrentSubtitleSelection(pending) else {
                throw CancellationError()
            }
            if publishedSubtitleState.catalogState != .loading {
                return nil
            }
            guard clock.now < deadline else {
                throw VesperSubtitleSelectionError.selectionTimedOut(
                    trackId: pending.selection.trackId
                )
            }
            try await clock.sleep(for: subtitleSelectionWaitPolicy.pollInterval)
        }
        return subtitleGroup
    }

    private func isCurrentSubtitleSelection(_ pending: PendingSubtitleSelection) -> Bool {
        subtitleSelectionInvalidationFailure(for: pending) == nil
    }

    private func subtitleSelectionInvalidationFailure(
        for pending: PendingSubtitleSelection
    ) -> VesperSubtitleSelectionError? {
        if currentSource == nil {
            return .selectionCancelled(trackId: pending.selection.trackId)
        }
        if subtitleSourceEpoch != pending.sourceEpoch
            || currentPlaybackEpoch() != pending.playbackEpoch
            || player?.currentItem !== pending.item {
            return .sourceChanged(trackId: pending.selection.trackId)
        }
        guard let current = pendingSubtitleSelection else {
            return .selectionSuperseded(trackId: pending.selection.trackId)
        }
        if current.commandId != pending.commandId
            || current.sourceEpoch != pending.sourceEpoch
            || current.playbackEpoch != pending.playbackEpoch
            || current.item !== pending.item {
            return .selectionSuperseded(trackId: pending.selection.trackId)
        }
        return nil
    }

    func subtitleTrackId(for option: AVMediaSelectionOption) -> String? {
        subtitleOptionsByTrackId.first { _, candidate in candidate === option }?.key
    }

    private func commitSubtitleSelection(
        _ pending: PendingSubtitleSelection,
        effectiveTrackId: String?
    ) {
        guard isCurrentSubtitleSelection(pending) else { return }
        confirmedSubtitleSelection = pending.selection
        publishedConfirmedSubtitleSelection = pending.selection
        publishedEffectiveSubtitleTrackId = effectiveTrackId
        publishedTrackSelection = VesperTrackSelectionSnapshot(
            video: publishedTrackSelection.video,
            audio: publishedTrackSelection.audio,
            subtitle: publishedTrackSelection.subtitle,
            confirmedSubtitle: pending.selection,
            effectiveSubtitleTrackId: effectiveTrackId,
            abrPolicy: publishedTrackSelection.abrPolicy
        )
        if pending.origin == .explicit {
            latestConfirmedExplicitSubtitleSelection = (
                sourceEpoch: pending.sourceEpoch,
                selection: pending.selection
            )
        }
        publishedSubtitleState = VesperSubtitleState(
            catalogState: publishedSubtitleState.catalogState,
            selectionState: .confirmed,
            advertisedTrackCount: publishedSubtitleState.advertisedTrackCount,
            selectableTrackCount: publishedSubtitleState.selectableTrackCount,
            catalogError: publishedSubtitleState.catalogError,
            selectionError: nil
        )
    }

    func setAbrPolicy(_ policy: VesperAbrPolicy) {
        applyAbrPolicy(
            policy,
            origin: .manual,
            clearLastReportedError: true
        )
    }

    func applyAbrPolicy(
        _ policy: VesperAbrPolicy,
        origin: AbrPolicyOrigin,
        clearLastReportedError: Bool
    ) {
        if clearLastReportedError {
            clearLastError()
        }
        let trackIdText = policy.trackId ?? "nil"
        let maxBitRateText = policy.maxBitRate.map(String.init) ?? "nil"
        let maxWidthText = policy.maxWidth.map(String.init) ?? "nil"
        let maxHeightText = policy.maxHeight.map(String.init) ?? "nil"
        iosHostLog(
            "setAbrPolicy mode=\(policy.mode.rawValue) trackId=\(trackIdText) maxBitRate=\(maxBitRateText) maxWidth=\(maxWidthText) maxHeight=\(maxHeightText)"
        )
        let hasResolutionLimit = policy.maxWidth != nil || policy.maxHeight != nil
        let resolvedVideoVariantPin: LoadedVideoVariantPin?
        var resolvedFixedTrackId: String?
        switch policy.mode {
        case .constrained:
            guard policy.maxBitRate != nil || hasResolutionLimit else {
                reportCommandError(
                    code: .unsupported,
                    category: .capability,
                    message:
                        "setAbrPolicy constrained mode requires maxBitRate or maxWidth/maxHeight on iOS"
                )
                return
            }
            if
                hasResolutionLimit,
                let resolvedPin = resolvedConstrainedVideoVariantPin(for: policy)
            {
                resolvedVideoVariantPin = resolvedPin
            } else if hasResolutionLimit {
                reportCommandError(
                    code: .unsupported,
                    category: .capability,
                    message:
                        "setAbrPolicy constrained mode requires a loaded iOS video variant catalog to infer a single-axis maxWidth/maxHeight limit"
                )
                return
            } else {
                resolvedVideoVariantPin = LoadedVideoVariantPin(
                    peakBitRate: policy.maxBitRate.map(Double.init),
                    maxWidth: nil,
                    maxHeight: nil
                )
            }
        case .fixedTrack:
            guard let trackId = policy.trackId, !trackId.isEmpty else {
                reportCommandError(
                    code: .invalidArgument,
                    category: .input,
                    message: "setAbrPolicy fixedTrack requires a non-empty trackId on iOS"
                )
                return
            }
            guard let resolvedFixedTrack = resolvedFixedVideoVariantTrack(for: trackId) else {
                reportCommandError(
                    code: .unsupported,
                    category: .capability,
                    message:
                        "setAbrPolicy fixedTrack requires a video variant from the current iOS track catalog (trackId=\(trackId))"
                )
                return
            }
            guard resolvedFixedTrack.pin.hasAnyLimit else {
                reportCommandError(
                    code: .unsupported,
                    category: .capability,
                    message:
                        "setAbrPolicy fixedTrack could not derive bitrate or resolution limits for trackId=\(resolvedFixedTrack.track.id) on iOS"
                )
                return
            }
            resolvedFixedTrackId = resolvedFixedTrack.track.id
            resolvedVideoVariantPin = resolvedFixedTrack.pin
        case .auto:
            resolvedVideoVariantPin = nil
            break
        }

        guard let item = player?.currentItem else {
            iosHostLog("setAbrPolicy ignored: no current item")
            return
        }

        switch policy.mode {
        case .auto:
            fixedTrackConvergenceState = nil
            applyVideoVariantPin(nil, to: item)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: .auto(),
                    audio: current.audio,
                    subtitle: current.subtitle,
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: .auto()
                )
            }
        case .constrained:
            fixedTrackConvergenceState = nil
            applyVideoVariantPin(resolvedVideoVariantPin, to: item)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: .auto(),
                    audio: current.audio,
                    subtitle: current.subtitle,
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: .constrained(
                        maxBitRate: policy.maxBitRate,
                        maxWidth: policy.maxWidth,
                        maxHeight: policy.maxHeight
                    )
                )
            }
        case .fixedTrack:
            guard let resolvedFixedTrackId, let resolvedVideoVariantPin else {
                return
            }
            fixedTrackConvergenceState = FixedTrackConvergenceState(
                requestedTrackId: resolvedFixedTrackId,
                origin: origin
            )
            applyVideoVariantPin(resolvedVideoVariantPin, to: item)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    // iOS fixedTrack is a best-effort variant constraint, not exact video-track selection.
                    video: .auto(),
                    audio: current.audio,
                    subtitle: current.subtitle,
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: .fixedTrack(resolvedFixedTrackId)
                )
            }
        }
    }

    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        clearLastError()
        if currentResiliencePolicy == policy {
            return
        }

        currentResiliencePolicy = policy
        publishedResiliencePolicy = policy
        guard let currentSource else {
            return
        }

        iosHostLog(
            "apply resilience policy buffering=\(policy.buffering.preset.rawValue) retry=\(policy.retry.backoff.rawValue) cache=\(policy.cache.preset.rawValue)"
        )
        cancelPendingRetry(resetAttempts: true)

        guard player != nil else {
            return
        }

        pendingResilienceRestore = PendingResilienceRestore(
            sourceUri: currentSource.uri,
            state: PreservedPlaybackState.capture(
                uiState: publishedUiState,
                trackSelection: publishedTrackSelection,
                confirmedSubtitleSelection: publishedConfirmedSubtitleSelection
            )
        )

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: true,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
        initialize()
    }

    func setAudioSessionInterrupted(_ interrupted: Bool) {
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: $0.isBuffering,
                isInterrupted: interrupted,
                timeline: $0.timeline
            )
        }
    }

    func drainBenchmarkEvents() -> [VesperBenchmarkEvent] {
        benchmarkRecorder.drainEvents()
    }

    func benchmarkSummary() -> VesperBenchmarkSummary {
        benchmarkRecorder.summary()
    }

    func awaitBenchmarkSinkShutdown(timeout: TimeInterval) async -> Bool {
        await benchmarkRecorder.awaitSinkShutdown(timeout: timeout)
    }
}
