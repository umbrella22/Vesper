@preconcurrency import AVFoundation
import CoreGraphics
import Foundation

func resolveRequestedVideoVariantTrackId(
    _ requestedTrackId: String,
    tracks: [VesperMediaTrack]
) -> String? {
    guard !requestedTrackId.isEmpty else {
        return nil
    }

    if tracks.contains(where: { $0.id == requestedTrackId }) {
        return requestedTrackId
    }

    guard
        let requestedFingerprint = StableVideoVariantFingerprint(trackId: requestedTrackId),
        requestedFingerprint.hasComparableFields
    else {
        return nil
    }

    return tracks
        .filter { $0.kind == .video }
        .min { lhs, rhs in
            let lhsScore = requestedVideoVariantTrackScore(lhs, requested: requestedFingerprint)
            let rhsScore = requestedVideoVariantTrackScore(rhs, requested: requestedFingerprint)
            if lhsScore != rhsScore {
                return lhsScore < rhsScore
            }
            return preferredVideoVariantTrack(lhs, over: rhs).id == lhs.id
        }?
        .id
}

struct RequestedVideoVariantTrackScore: Comparable {
    let codecPenalty: Int
    let sizeMissingPenalty: Int
    let sizeDistance: Int
    let bitRateMissingPenalty: Int
    let bitRateDistance: Int64
    let frameRateMissingPenalty: Int
    let frameRateDistance: Int64
    let inverseWidth: Int
    let inverseHeight: Int
    let inverseBitRate: Int
    let trackId: String

    static func < (
        lhs: RequestedVideoVariantTrackScore,
        rhs: RequestedVideoVariantTrackScore
    ) -> Bool {
        if lhs.codecPenalty != rhs.codecPenalty {
            return lhs.codecPenalty < rhs.codecPenalty
        }
        if lhs.sizeMissingPenalty != rhs.sizeMissingPenalty {
            return lhs.sizeMissingPenalty < rhs.sizeMissingPenalty
        }
        if lhs.sizeDistance != rhs.sizeDistance {
            return lhs.sizeDistance < rhs.sizeDistance
        }
        if lhs.bitRateMissingPenalty != rhs.bitRateMissingPenalty {
            return lhs.bitRateMissingPenalty < rhs.bitRateMissingPenalty
        }
        if lhs.bitRateDistance != rhs.bitRateDistance {
            return lhs.bitRateDistance < rhs.bitRateDistance
        }
        if lhs.frameRateMissingPenalty != rhs.frameRateMissingPenalty {
            return lhs.frameRateMissingPenalty < rhs.frameRateMissingPenalty
        }
        if lhs.frameRateDistance != rhs.frameRateDistance {
            return lhs.frameRateDistance < rhs.frameRateDistance
        }
        if lhs.inverseWidth != rhs.inverseWidth {
            return lhs.inverseWidth < rhs.inverseWidth
        }
        if lhs.inverseHeight != rhs.inverseHeight {
            return lhs.inverseHeight < rhs.inverseHeight
        }
        if lhs.inverseBitRate != rhs.inverseBitRate {
            return lhs.inverseBitRate < rhs.inverseBitRate
        }
        return lhs.trackId < rhs.trackId
    }
}

func requestedVideoVariantTrackScore(
    _ track: VesperMediaTrack,
    requested: StableVideoVariantFingerprint
) -> RequestedVideoVariantTrackScore {
    let candidate = StableVideoVariantFingerprint(track: track)
    let codecPenalty = requestedCodecPenalty(
        requested.codecComponent,
        candidate.codecComponent
    )
    let widthDistance = requestedVariantDistance(requested.width, candidate.width)
    let heightDistance = requestedVariantDistance(requested.height, candidate.height)
    let bitRateDistance = requestedVariantDistance(requested.peakBitRate, candidate.peakBitRate)
    let frameRateDistance = requestedVariantDistance(
        requested.frameRateBucket,
        candidate.frameRateBucket
    )

    return RequestedVideoVariantTrackScore(
        codecPenalty: codecPenalty,
        sizeMissingPenalty: widthDistance.missingPenalty + heightDistance.missingPenalty,
        sizeDistance: widthDistance.distance + heightDistance.distance,
        bitRateMissingPenalty: bitRateDistance.missingPenalty,
        bitRateDistance: bitRateDistance.distance,
        frameRateMissingPenalty: frameRateDistance.missingPenalty,
        frameRateDistance: Int64(frameRateDistance.distance),
        inverseWidth: Int.max - (track.width ?? 0),
        inverseHeight: Int.max - (track.height ?? 0),
        inverseBitRate: Int.max - Int(clamping: track.bitRate ?? 0),
        trackId: track.id
    )
}

func requestedCodecPenalty(_ requested: String?, _ candidate: String?) -> Int {
    guard let requested else {
        return 0
    }
    guard let candidate else {
        return 1
    }
    return requested == candidate ? 0 : 3
}

func requestedVariantDistance(
    _ requested: Int?,
    _ candidate: Int?
) -> (missingPenalty: Int, distance: Int) {
    guard let requested else {
        return (0, 0)
    }
    guard let candidate else {
        return (1, Int.max / 4)
    }
    return (0, abs(candidate - requested))
}

func requestedVariantDistance(
    _ requested: Int64?,
    _ candidate: Int64?
) -> (missingPenalty: Int, distance: Int64) {
    guard let requested else {
        return (0, 0)
    }
    guard let candidate else {
        return (1, Int64.max / 4)
    }
    return (0, abs(candidate - requested))
}

func preferredVideoVariantTrack(
    _ lhs: VesperMediaTrack,
    over rhs: VesperMediaTrack
) -> VesperMediaTrack {
    let lhsBitRate = lhs.bitRate ?? -1
    let rhsBitRate = rhs.bitRate ?? -1
    if lhsBitRate != rhsBitRate {
        return lhsBitRate > rhsBitRate ? lhs : rhs
    }

    let lhsMaxEdge = max(lhs.width ?? 0, lhs.height ?? 0)
    let rhsMaxEdge = max(rhs.width ?? 0, rhs.height ?? 0)
    if lhsMaxEdge != rhsMaxEdge {
        return lhsMaxEdge > rhsMaxEdge ? lhs : rhs
    }

    let lhsMinEdge = min(lhs.width ?? 0, lhs.height ?? 0)
    let rhsMinEdge = min(rhs.width ?? 0, rhs.height ?? 0)
    if lhsMinEdge != rhsMinEdge {
        return lhsMinEdge > rhsMinEdge ? lhs : rhs
    }

    let lhsFrameRate = Int((lhs.frameRate ?? 0).rounded())
    let rhsFrameRate = Int((rhs.frameRate ?? 0).rounded())
    if lhsFrameRate != rhsFrameRate {
        return lhsFrameRate > rhsFrameRate ? lhs : rhs
    }

    return (lhs.label ?? lhs.id) <= (rhs.label ?? rhs.id) ? lhs : rhs
}
