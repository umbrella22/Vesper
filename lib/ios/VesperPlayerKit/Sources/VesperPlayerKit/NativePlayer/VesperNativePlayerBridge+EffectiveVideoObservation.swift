@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func refreshEffectiveVideoTrackObservation(for item: AVPlayerItem?) {
        let now = Date()
        let videoVariantObservation = resolvedVideoVariantObservation(for: item)
        if publishedVideoVariantObservation != videoVariantObservation {
            publishedVideoVariantObservation = videoVariantObservation
        }
        let resolvedTrackId = resolvedEffectiveVideoTrackId(
            for: item,
            observation: videoVariantObservation
        )
        if publishedEffectiveVideoTrackId != resolvedTrackId {
            publishedEffectiveVideoTrackId = resolvedTrackId
        }
        let rawFixedTrackStatus = resolveFixedTrackStatus(
            abrPolicy: publishedTrackSelection.abrPolicy,
            effectiveVideoTrackId: resolvedTrackId,
            tracks: publishedTrackCatalog.videoTracks
        )
        handleFixedTrackConvergenceUpdate(
            status: rawFixedTrackStatus,
            effectiveVideoTrackId: resolvedTrackId,
            observation: videoVariantObservation,
            now: now
        )
        let resolvedPublishedStatus =
            publishedTrackSelection.abrPolicy.mode == .fixedTrack
            ? resolvePublishableFixedTrackStatus(
                rawStatus: rawFixedTrackStatus,
                lockedElapsed: fixedTrackConvergenceState?.lockedStartedAt.map {
                    now.timeIntervalSince($0)
                },
                hasPersistentMismatch: fixedTrackConvergenceState?
                    .hasHandledPersistentMismatch == true
            )
            : nil
        if publishedFixedTrackStatus != resolvedPublishedStatus {
            publishedFixedTrackStatus = resolvedPublishedStatus
        }
    }

    func resolvedEffectiveVideoTrackId(
        for item: AVPlayerItem?,
        observation: VesperVideoVariantObservation?
    ) -> String? {
        guard item != nil else {
            return nil
        }

        let videoTracks = publishedTrackCatalog.videoTracks
        guard !videoTracks.isEmpty else {
            return nil
        }

        let effectiveBitRate = observation?.bitRate.map(Double.init)
        let effectivePresentationSize = resolvedPresentationSize(for: observation)
        guard effectiveBitRate != nil || effectivePresentationSize != nil else {
            return nil
        }

        let requestedTrackId =
            publishedTrackSelection.abrPolicy.mode == .fixedTrack
            ? publishedTrackSelection.abrPolicy.trackId
            : nil

        return videoTracks.min { lhs, rhs in
            let lhsScore = effectiveVideoTrackScore(
                lhs,
                bitRate: effectiveBitRate,
                presentationSize: effectivePresentationSize,
                requestedTrackId: requestedTrackId
            )
            let rhsScore = effectiveVideoTrackScore(
                rhs,
                bitRate: effectiveBitRate,
                presentationSize: effectivePresentationSize,
                requestedTrackId: requestedTrackId
            )
            if lhsScore != rhsScore {
                return lhsScore < rhsScore
            }
            return comparePreferredEffectiveVideoTrack(lhs, over: rhs)
        }?.id
    }

    func resolvedVideoVariantObservation(
        for item: AVPlayerItem?
    ) -> VesperVideoVariantObservation? {
        guard let item else {
            return nil
        }
        return resolveVideoVariantObservation(
            bitRate: resolvedEffectiveVideoBitRate(for: item),
            presentationSize: resolvedEffectivePresentationSize(for: item)
        )
    }

    func resolvedEffectiveVideoBitRate(for item: AVPlayerItem) -> Double? {
        guard let event = item.accessLog()?.events.last else {
            return nil
        }

        if event.indicatedBitrate.isFinite, event.indicatedBitrate > 0 {
            return event.indicatedBitrate
        }
        if event.observedBitrate.isFinite, event.observedBitrate > 0 {
            return event.observedBitrate
        }
        return nil
    }

    func resolvedEffectivePresentationSize(for item: AVPlayerItem) -> CGSize? {
        let size = item.presentationSize
        guard size.width.isFinite, size.height.isFinite, size.width > 0, size.height > 0 else {
            return nil
        }
        return size
    }

    func resolvedPresentationSize(
        for observation: VesperVideoVariantObservation?
    ) -> CGSize? {
        guard
            let width = observation?.width,
            let height = observation?.height,
            width > 0,
            height > 0
        else {
            return nil
        }
        return CGSize(width: width, height: height)
    }

    func effectiveVideoTrackScore(
        _ track: VesperMediaTrack,
        bitRate: Double?,
        presentationSize: CGSize?,
        requestedTrackId: String?
    ) -> (Int, Int64, Int) {
        let sizeDistance = effectiveVideoTrackSizeDistance(track, presentationSize: presentationSize)
        let bitRateDistance = effectiveVideoTrackBitRateDistance(track, bitRate: bitRate)
        let requestedTrackPenalty: Int
        if let requestedTrackId {
            requestedTrackPenalty = requestedTrackId == track.id ? 0 : 1
        } else {
            requestedTrackPenalty = 0
        }
        return (sizeDistance, bitRateDistance, requestedTrackPenalty)
    }

    func effectiveVideoTrackSizeDistance(
        _ track: VesperMediaTrack,
        presentationSize: CGSize?
    ) -> Int {
        guard let presentationSize else {
            return 0
        }
        guard let width = track.width, let height = track.height else {
            return Int.max / 4
        }

        let currentMaxEdge = Int(max(presentationSize.width, presentationSize.height).rounded())
        let currentMinEdge = Int(min(presentationSize.width, presentationSize.height).rounded())
        let trackMaxEdge = max(width, height)
        let trackMinEdge = min(width, height)
        return abs(trackMaxEdge - currentMaxEdge) + abs(trackMinEdge - currentMinEdge)
    }

    func effectiveVideoTrackBitRateDistance(
        _ track: VesperMediaTrack,
        bitRate: Double?
    ) -> Int64 {
        guard let bitRate else {
            return 0
        }
        guard let trackBitRate = track.bitRate else {
            return Int64.max / 4
        }
        return Int64(abs(Double(trackBitRate) - bitRate).rounded())
    }

    func comparePreferredEffectiveVideoTrack(
        _ lhs: VesperMediaTrack,
        over rhs: VesperMediaTrack
    ) -> Bool {
        let lhsBitRate = lhs.bitRate ?? -1
        let rhsBitRate = rhs.bitRate ?? -1
        if lhsBitRate != rhsBitRate {
            return lhsBitRate > rhsBitRate
        }

        let lhsMaxEdge = max(lhs.width ?? 0, lhs.height ?? 0)
        let rhsMaxEdge = max(rhs.width ?? 0, rhs.height ?? 0)
        if lhsMaxEdge != rhsMaxEdge {
            return lhsMaxEdge > rhsMaxEdge
        }

        let lhsMinEdge = min(lhs.width ?? 0, lhs.height ?? 0)
        let rhsMinEdge = min(rhs.width ?? 0, rhs.height ?? 0)
        if lhsMinEdge != rhsMinEdge {
            return lhsMinEdge > rhsMinEdge
        }

        let lhsFrameRate = Int((lhs.frameRate ?? 0).rounded())
        let rhsFrameRate = Int((rhs.frameRate ?? 0).rounded())
        if lhsFrameRate != rhsFrameRate {
            return lhsFrameRate > rhsFrameRate
        }

        return (lhs.label ?? lhs.id) <= (rhs.label ?? rhs.id)
    }
}
