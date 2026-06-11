@preconcurrency import AVFoundation
import CoreGraphics
import Foundation

func abrPolicyRequiresLoadedVideoVariantCatalog(_ policy: VesperAbrPolicy) -> Bool {
    switch policy.mode {
    case .fixedTrack:
        return true
    case .constrained:
        let hasWidthLimit = policy.maxWidth != nil
        let hasHeightLimit = policy.maxHeight != nil
        return hasWidthLimit != hasHeightLimit
    case .auto:
        return false
    }
}

func sourceSupportsVideoVariantCatalog(_ source: VesperPlayerSource?) -> Bool {
    guard let source else {
        return false
    }
    return source.protocol == .hls || source.protocol == .dash
}

func resolveFixedTrackStatus(
    abrPolicy: VesperAbrPolicy,
    effectiveVideoTrackId: String?,
    tracks: [VesperMediaTrack]
) -> VesperFixedTrackStatus? {
    guard
        abrPolicy.mode == .fixedTrack,
        let requestedTrackId = abrPolicy.trackId,
        !requestedTrackId.isEmpty
    else {
        return nil
    }

    guard tracks.contains(where: { $0.id == requestedTrackId }) else {
        return .pending
    }

    guard let effectiveVideoTrackId else {
        return .pending
    }

    if effectiveVideoTrackId == requestedTrackId {
        return .locked
    }

    return .fallback
}

func resolvePublishableFixedTrackStatus(
    rawStatus: VesperFixedTrackStatus?,
    lockedElapsed: TimeInterval?,
    hasPersistentMismatch: Bool
) -> VesperFixedTrackStatus? {
    switch rawStatus {
    case .locked:
        guard let lockedElapsed else {
            return .pending
        }
        return lockedElapsed >= 0.75 ? .locked : .pending
    case .fallback:
        return hasPersistentMismatch ? .fallback : .pending
    case .pending:
        return .pending
    case nil:
        return nil
    }
}

func resolveFixedTrackRecoveryPolicy(
    requestedTrackId: String,
    tracks: [VesperMediaTrack]
) -> VesperAbrPolicy {
    guard let requestedTrack = tracks.first(where: { $0.id == requestedTrackId }) else {
        return .auto()
    }

    let hasResolutionLimit = requestedTrack.width != nil && requestedTrack.height != nil
    let hasBitRateLimit = requestedTrack.bitRate != nil
    guard hasResolutionLimit || hasBitRateLimit else {
        return .auto()
    }

    return .constrained(
        maxBitRate: requestedTrack.bitRate,
        maxWidth: hasResolutionLimit ? requestedTrack.width : nil,
        maxHeight: hasResolutionLimit ? requestedTrack.height : nil
    )
}

func shouldEscalatePersistentFixedTrackFallback(
    status: VesperFixedTrackStatus?,
    observation: VesperVideoVariantObservation?,
    playbackState: PlaybackStateUi,
    isBuffering: Bool,
    elapsed: TimeInterval
) -> Bool {
    guard status == .fallback else {
        return false
    }
    guard observation != nil else {
        return false
    }
    guard playbackState == .playing, !isBuffering else {
        return false
    }
    return elapsed >= 2.0
}

func resolveVideoVariantObservation(
    bitRate: Double?,
    presentationSize: CGSize?
) -> VesperVideoVariantObservation? {
    let normalizedBitRate: Int64?
    if let bitRate, bitRate.isFinite, bitRate > 0 {
        normalizedBitRate = Int64(bitRate.rounded())
    } else {
        normalizedBitRate = nil
    }

    let normalizedWidth: Int?
    let normalizedHeight: Int?
    if
        let presentationSize,
        presentationSize.width.isFinite,
        presentationSize.height.isFinite,
        presentationSize.width > 0,
        presentationSize.height > 0
    {
        normalizedWidth = Int(presentationSize.width.rounded())
        normalizedHeight = Int(presentationSize.height.rounded())
    } else {
        normalizedWidth = nil
        normalizedHeight = nil
    }

    guard normalizedBitRate != nil || (normalizedWidth != nil && normalizedHeight != nil) else {
        return nil
    }

    return VesperVideoVariantObservation(
        bitRate: normalizedBitRate,
        width: normalizedWidth,
        height: normalizedHeight
    )
}
