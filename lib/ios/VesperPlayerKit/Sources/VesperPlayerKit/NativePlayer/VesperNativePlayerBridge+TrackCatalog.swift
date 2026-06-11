@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func resetTrackState() {
        audioGroup = nil
        subtitleGroup = nil
        videoVariantPinsByTrackId = [:]
        desiredVideoVariantPin = nil
        dashStartupAbrLimitPin = nil
        audioOptionsByTrackId = [:]
        subtitleOptionsByTrackId = [:]
        hasAppliedDefaultTrackPreferences = false
        fixedTrackConvergenceState = nil
        publishedTrackCatalog = .empty
        publishedTrackSelection = VesperTrackSelectionSnapshot()
        publishedEffectiveVideoTrackId = nil
        publishedVideoVariantObservation = nil
        publishedFixedTrackStatus = nil
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
    ) {
        let optionToSelect: AVMediaSelectionOption?
        switch selection.mode {
        case .auto:
            optionToSelect = group.defaultOption ?? item.currentMediaSelection.selectedMediaOption(in: group)
        case .disabled:
            optionToSelect = nil
        case .track:
            guard let trackId = selection.trackId, let option = optionsByTrackId[trackId] else {
                let trackIdText = selection.trackId ?? "nil"
                iosHostLog(
                    "set\(kind.rawValue.capitalized)TrackSelection ignored: trackId=\(trackIdText) is not present in the current catalog"
                )
                return
            }
            optionToSelect = option
        }

        item.select(optionToSelect, in: group)
        updateTrackSelection { current in
            switch kind {
            case .video:
                VesperTrackSelectionSnapshot(
                    video: selection,
                    audio: current.audio,
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            case .audio:
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: selection,
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            case .subtitle:
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: selection,
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }
}
