@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func resetTrackState() {
        let preserveConfirmedSelection = pendingResilienceRestore != nil
        let preservedConfirmedSelection = publishedConfirmedSubtitleSelection
        cancelPendingSubtitleSelection()
        audioGroup = nil
        subtitleGroup = nil
        videoVariantPinsByTrackId = [:]
        desiredVideoVariantPin = nil
        dashStartupAbrLimitPin = nil
        audioOptionsByTrackId = [:]
        subtitleOptionsByTrackId = [:]
        failedSubtitleTrackIds.removeAll()
        subtitleOverlayRenderer.reset()
        hasAppliedDefaultTrackPreferences = false
        fixedTrackConvergenceState = nil
        publishedTrackCatalog = .empty
        publishedTrackSelection = VesperTrackSelectionSnapshot(
            subtitle: .disabled(),
            confirmedSubtitle: preserveConfirmedSelection ? preservedConfirmedSelection : .disabled(),
            effectiveSubtitleTrackId: nil
        )
        publishedEffectiveVideoTrackId = nil
        publishedEffectiveSubtitleTrackId = nil
        confirmedSubtitleSelection = preserveConfirmedSelection
            ? preservedConfirmedSelection
            : .disabled()
        publishedRequestedSubtitleSelection = .disabled()
        publishedConfirmedSubtitleSelection = preserveConfirmedSelection
            ? preservedConfirmedSelection
            : .disabled()
        publishedVideoVariantObservation = nil
        publishedFixedTrackStatus = nil
        // Reset the subtitle lifecycle state so a new source epoch does not
        // inherit a stale failure from the previous source.
        publishedSubtitleState = preserveConfirmedSelection
            ? .loading(advertisedTrackCount: currentSource?.externalSubtitles.count ?? 0)
            : (currentSource.map {
                .loading(advertisedTrackCount: $0.externalSubtitles.count)
            } ?? .empty)
        pendingSubtitleOverlayFailure = nil
    }

    func updateTrackSelection(
        _ transform: (VesperTrackSelectionSnapshot) -> VesperTrackSelectionSnapshot
    ) {
        publishedTrackSelection = transform(publishedTrackSelection)
        refreshEffectiveVideoTrackObservation(for: player?.currentItem)
    }

    func resolvedConstrainedVideoVariantPin(
        for policy: VesperAbrPolicy
    ) -> LoadedVideoVariantPin? {
        let resolvedResolution = resolveConstrainedMaximumVideoResolution(
            maxWidth: policy.maxWidth,
            maxHeight: policy.maxHeight,
            tracks: publishedTrackCatalog.videoTracks
        )
        if (policy.maxWidth != nil || policy.maxHeight != nil) && resolvedResolution == nil {
            return nil
        }

        return LoadedVideoVariantPin(
            peakBitRate: policy.maxBitRate.map(Double.init),
            maxWidth: resolvedResolution?.width,
            maxHeight: resolvedResolution?.height
        )
    }

    func resolvedFixedVideoVariantTrack(
        for requestedTrackId: String
    ) -> (track: VesperMediaTrack, pin: LoadedVideoVariantPin)? {
        let videoTracks = publishedTrackCatalog.videoTracks
        guard !videoTracks.isEmpty else {
            return nil
        }

        if
            let exactTrack = videoTracks.first(where: { $0.id == requestedTrackId }),
            let exactPin = videoVariantPinsByTrackId[requestedTrackId]
        {
            return (track: exactTrack, pin: exactPin)
        }

        guard
            let resolvedTrackId = resolveRequestedVideoVariantTrackId(
                requestedTrackId,
                tracks: videoTracks
            ),
            let resolvedTrack = videoTracks.first(where: { $0.id == resolvedTrackId }),
            let resolvedPin = videoVariantPinsByTrackId[resolvedTrackId]
        else {
            return nil
        }

        iosHostLog(
            "remapped fixedTrack request trackId=\(requestedTrackId) resolvedTrackId=\(resolvedTrackId)"
        )
        return (track: resolvedTrack, pin: resolvedPin)
    }

    func applyTrackSelection(
        _ selection: VesperTrackSelection,
        kind: VesperMediaTrackKind,
        group: AVMediaSelectionGroup,
        optionsByTrackId: [String: AVMediaSelectionOption],
        item: AVPlayerItem
    ) throws {
        let optionToSelect: AVMediaSelectionOption?
        switch selection.mode {
        case .auto:
            if kind == .subtitle {
                guard let candidate = automaticSubtitleOption(
                    in: group,
                    optionsByTrackId: optionsByTrackId
                ) else {
                    reportSubtitleFailure(
                        code: "subtitle_auto_candidate_unavailable",
                        phase: .selection,
                        trackId: nil,
                        message: "subtitle auto selection has no policy-matching candidate"
                    )
                    throw VesperSubtitleSelectionError.autoCandidateUnavailable
                }
                optionToSelect = candidate
            } else {
                optionToSelect = group.defaultOption
                    ?? item.currentMediaSelection.selectedMediaOption(in: group)
            }
        case .disabled:
            optionToSelect = nil
        case .track:
            guard let trackId = selection.trackId, let option = optionsByTrackId[trackId] else {
                let trackIdText = selection.trackId ?? "nil"
                iosHostLog(
                    "set\(kind.rawValue.capitalized)TrackSelection ignored: requested track is not present in the current catalog"
                )
                if kind == .subtitle {
                    reportSubtitleFailure(
                        code: "subtitle_track_not_found",
                        phase: .selection,
                        trackId: trackIdText,
                        message: "subtitle track id \(trackIdText) is not present in the current catalog"
                    )
                    throw VesperSubtitleSelectionError.trackNotFound(trackId: trackIdText)
                }
                return
            }
            optionToSelect = option
        }

        item.select(optionToSelect, in: group)

        // The snapshot must reflect what AVPlayer actually selected, not the
        // request intent. For subtitles in
        // `.track` mode, read back `currentMediaSelection.selectedMediaOption(in:)`
        // and confirm it matches the target before publishing the requested
        // selection. `.disabled` must converge on `nil`; a non-nil
        // selection after a disable request is a `subtitle_selection_failed`.
        // `.auto` is intentionally excluded because AVPlayer may
        // legitimately pick a different option based on system language
        // preferences, and that is an acceptable automatic choice. For
        // audio/video the legacy behavior is preserved because they do not
        // have a subtitle failure channel.
        if kind == .subtitle {
            let confirmed = item.currentMediaSelection.selectedMediaOption(in: group)
            switch selection.mode {
            case .track:
                if let expected = optionToSelect, confirmed !== expected {
                    reportSubtitleFailure(
                        code: "subtitle_selection_failed",
                        phase: .selection,
                        trackId: selection.trackId,
                        message: "AVPlayer did not converge on the requested subtitle option"
                    )
                    throw VesperSubtitleSelectionError.selectionDidNotConverge(
                        trackId: selection.trackId
                    )
                }
            case .disabled:
                if confirmed != nil {
                    reportSubtitleFailure(
                        code: "subtitle_selection_failed",
                        phase: .selection,
                        trackId: nil,
                        message: "AVPlayer did not disable the subtitle selection"
                    )
                    throw VesperSubtitleSelectionError.selectionDidNotConverge(trackId: nil)
                }
            case .auto:
                if let expected = optionToSelect, confirmed !== expected {
                    reportSubtitleFailure(
                        code: "subtitle_selection_failed",
                        phase: .selection,
                        trackId: nil,
                        message: "AVPlayer did not converge on the automatic subtitle candidate"
                    )
                    throw VesperSubtitleSelectionError.selectionDidNotConverge(trackId: nil)
                }
            }
        }

        updateTrackSelection { current in
            switch kind {
            case .video:
                VesperTrackSelectionSnapshot(
                    video: selection,
                    audio: current.audio,
                    subtitle: current.subtitle,
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: current.abrPolicy
                )
            case .audio:
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: selection,
                    subtitle: current.subtitle,
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: current.abrPolicy
                )
            case .subtitle:
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: selection,
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: current.abrPolicy
                )
            }
        }
        if kind == .subtitle {
            confirmedSubtitleSelection = selection
            publishedEffectiveSubtitleTrackId = optionToSelect.flatMap { option in
                subtitleOptionsByTrackId.first { _, candidate in candidate === option }?.key
            }
        }
    }
}
