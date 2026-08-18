@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    var activeTrackCatalogPlaybackPath: String? {
        guard currentSource != nil else { return nil }
        return nativeFramePipelineCoordinator.activeSession == nil
            ? "systemPlayer"
            : "sdkManagedNativeFrame"
    }

    /// Publishes all track catalogs through one identity/revision boundary.
    /// The revision is session-local and changes only when source identity,
    /// playback path, DRM context, track identity/support, or adaptivity
    /// changes.
    func publishTrackCatalog(
        _ catalog: VesperTrackCatalog,
        playbackPath: String? = nil
    ) {
        let resolvedPlaybackPath = playbackPath ?? catalog.playbackPath ?? activeTrackCatalogPlaybackPath
        let normalizedTracks = resolvedPlaybackPath.map { path in
            catalog.tracks.map { $0.catalogEntry(forPlaybackPath: path) }
        } ?? catalog.tracks
        let fingerprint = TrackCatalogFingerprint(
            sourceEpoch: subtitleSourceEpoch,
            playbackPath: resolvedPlaybackPath,
            drmKeySystem: currentSource?.drmConfiguration?.keySystem,
            tracks: normalizedTracks.sorted {
                if $0.kind.rawValue != $1.kind.rawValue {
                    return $0.kind.rawValue < $1.kind.rawValue
                }
                return $0.id < $1.id
            },
            adaptiveVideo: catalog.adaptiveVideo,
            adaptiveAudio: catalog.adaptiveAudio
        )
        if trackCatalogFingerprintState != fingerprint {
            if trackCatalogRevisionState < Int64.max {
                trackCatalogRevisionState += 1
            }
            trackCatalogFingerprintState = fingerprint
        }
        publishedTrackCatalog = VesperTrackCatalog(
            tracks: normalizedTracks,
            adaptiveVideo: catalog.adaptiveVideo,
            adaptiveAudio: catalog.adaptiveAudio,
            catalogRevision: trackCatalogRevisionState,
            playbackPath: resolvedPlaybackPath
        )
    }

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
        publishTrackCatalog(.empty)
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

    func validateFixedVideoVariantTrack(
        requestedTrackId: String,
        expectedCatalogRevision: Int64?
    ) throws -> (track: VesperMediaTrack, pin: LoadedVideoVariantPin) {
        let actualCatalogRevision = publishedTrackCatalog.catalogRevision
        if let expectedCatalogRevision,
           expectedCatalogRevision != actualCatalogRevision {
            throw VesperFixedTrackSelectionError(
                code: "staleCatalog",
                trackId: requestedTrackId,
                expectedCatalogRevision: expectedCatalogRevision,
                actualCatalogRevision: actualCatalogRevision,
                message: "the track catalog changed before the fixed-track command was applied",
                details: [
                    "playbackPath": publishedTrackCatalog.playbackPath ?? "systemPlayer"
                ]
            )
        }

        let videoTracks = publishedTrackCatalog.videoTracks
        let resolvedTrackId: String?
        if videoTracks.contains(where: { $0.id == requestedTrackId }) {
            resolvedTrackId = requestedTrackId
        } else {
            resolvedTrackId = resolveRequestedVideoVariantTrackId(
                requestedTrackId,
                tracks: videoTracks
            )
        }
        guard let resolvedTrackId,
              let track = videoTracks.first(where: { $0.id == resolvedTrackId })
        else {
            throw VesperFixedTrackSelectionError(
                code: "trackUnavailable",
                trackId: requestedTrackId,
                expectedCatalogRevision: expectedCatalogRevision,
                actualCatalogRevision: actualCatalogRevision,
                message: "setAbrPolicy fixedTrack requires a video variant from the current iOS track catalog (trackId=\(requestedTrackId))"
            )
        }

        let support = track.support
        let rejectionCode: String?
        switch support.status {
        case .exceedsCapabilities:
            rejectionCode = "trackExceedsCapabilities"
        case .unsupported:
            rejectionCode = "trackUnsupported"
        case .supported, .unknown:
            rejectionCode = nil
        }
        if let rejectionCode {
            var details = fixedTrackSupportDetails(support)
            details["resolvedTrackId"] = resolvedTrackId
            let message = rejectionCode == "trackExceedsCapabilities"
                ? "the requested video track exceeds current playback capabilities"
                : "the requested video track is unsupported by the active playback path"
            throw VesperFixedTrackSelectionError(
                code: rejectionCode,
                trackId: requestedTrackId,
                expectedCatalogRevision: expectedCatalogRevision,
                actualCatalogRevision: actualCatalogRevision,
                message: message,
                details: details
            )
        }

        guard let pin = videoVariantPinsByTrackId[resolvedTrackId], pin.hasAnyLimit else {
            throw VesperFixedTrackSelectionError(
                code: "trackUnsupported",
                trackId: requestedTrackId,
                expectedCatalogRevision: expectedCatalogRevision,
                actualCatalogRevision: actualCatalogRevision,
                message: "setAbrPolicy fixedTrack could not derive bitrate or resolution limits for trackId=\(resolvedTrackId) on iOS",
                details: [
                    "reason": VesperTrackSupportReason.presentationUnavailable.rawValue,
                    "resolvedTrackId": resolvedTrackId,
                    "playbackPath": publishedTrackCatalog.playbackPath ?? "systemPlayer"
                ]
            )
        }
        return (track: track, pin: pin)
    }

    private func fixedTrackSupportDetails(
        _ support: VesperTrackSupport
    ) -> [String: String] {
        var details: [String: String] = [
            "reason": support.reasonRawValue ?? support.reason.rawValue,
            "playbackPath": support.playbackPath
                ?? publishedTrackCatalog.playbackPath
                ?? "systemPlayer"
        ]
        if let raw = support.statusRawValue {
            details["statusRawValue"] = raw
        }
        if let raw = support.formatSupportRawValue {
            details["formatSupportRawValue"] = raw
        }
        return details
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
