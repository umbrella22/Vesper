@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

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

    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) throws {
        clearLastError()
        clearSubtitleFailure()
        iosHostLog("setSubtitleTrackSelection mode=\(selection.mode.rawValue)")
        guard let item = player?.currentItem else {
            throw VesperSubtitleSelectionError.platformTrackUnavailable(trackId: nil)
        }

        if subtitleOverlayRenderer.hasTracks {
            let selectedSideLoadId: String?
            switch selection.mode {
            case .disabled:
                selectedSideLoadId = nil
            case .auto:
                selectedSideLoadId = subtitleOverlayRenderer.firstTrackId()
            case .track:
                selectedSideLoadId = selection.trackId.flatMap { trackId in
                    subtitleOverlayRenderer.containsTrack(trackId) ? trackId : nil
                }
            }
            if selection.mode != .track || selectedSideLoadId != nil {
                if let group = subtitleGroup {
                    item.select(nil, in: group)
                }
                guard subtitleOverlayRenderer.select(trackId: selectedSideLoadId) else {
                    throw VesperSubtitleSelectionError.trackNotFound(
                        trackId: selection.trackId ?? ""
                    )
                }
                updateTrackSelection { current in
                    VesperTrackSelectionSnapshot(
                        video: current.video,
                        audio: current.audio,
                        subtitle: selection,
                        abrPolicy: current.abrPolicy
                    )
                }
                enforceSubtitleVisibility(for: item)
                return
            }
            _ = subtitleOverlayRenderer.select(trackId: nil)
            if selection.mode == .track, subtitleGroup == nil {
                throw VesperSubtitleSelectionError.trackNotFound(
                    trackId: selection.trackId ?? ""
                )
            }
        }

        // Immediate validation failures must throw so the iOS Flutter plugin
        // surfaces them through `FlutterError` and
        // the Dart `Future<void>` actually fails. Race failures after
        // `item.select` still surface through `reportSubtitleFailure`
        // because they cannot be observed synchronously.
        guard let group = subtitleGroup else {
            throw VesperSubtitleSelectionError.platformTrackUnavailable(trackId: selection.trackId)
        }

        // For .track mode, validate the id against the current catalog
        // before applying so an unknown id throws
        // `subtitle_track_not_found` instead of a silent no-op in
        // `applyTrackSelection`.
        if selection.mode == .track,
           let trackId = selection.trackId,
           subtitleOptionsByTrackId[trackId] == nil
        {
            throw VesperSubtitleSelectionError.trackNotFound(trackId: trackId)
        }

        try applyTrackSelection(
            selection,
            kind: .subtitle,
            group: group,
            optionsByTrackId: subtitleOptionsByTrackId,
            item: item
        )
        enforceSubtitleVisibility(for: item)
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
                trackSelection: publishedTrackSelection
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
}
