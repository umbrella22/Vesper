@preconcurrency import AVFoundation
import CoreGraphics
import Foundation

func resolveConstrainedMaximumVideoResolution(
    maxWidth: Int?,
    maxHeight: Int?,
    tracks: [VesperMediaTrack]
) -> ResolvedMaximumVideoResolution? {
    switch (maxWidth, maxHeight) {
    case let (width?, height?):
        guard width > 0, height > 0 else {
            return nil
        }
        return ResolvedMaximumVideoResolution(width: width, height: height)
    case let (width?, nil):
        guard width > 0 else {
            return nil
        }
        guard
            let reference = resolvedMaximumVideoResolutionReference(
                requestedWidth: width,
                requestedHeight: nil,
                tracks: tracks
            )
        else {
            return nil
        }
        let height = max(
            Int((Double(reference.height) / Double(reference.width) * Double(width)).rounded()),
            1
        )
        return ResolvedMaximumVideoResolution(width: width, height: height)
    case let (nil, height?):
        guard height > 0 else {
            return nil
        }
        guard
            let reference = resolvedMaximumVideoResolutionReference(
                requestedWidth: nil,
                requestedHeight: height,
                tracks: tracks
            )
        else {
            return nil
        }
        let width = max(
            Int((Double(reference.width) / Double(reference.height) * Double(height)).rounded()),
            1
        )
        return ResolvedMaximumVideoResolution(width: width, height: height)
    case (nil, nil):
        return nil
    }
}

func resolvedMaximumVideoResolutionReference(
    requestedWidth: Int?,
    requestedHeight: Int?,
    tracks: [VesperMediaTrack]
) -> ResolvedMaximumVideoResolution? {
    let candidates = tracks.compactMap { track -> ResolvedMaximumVideoResolution? in
        guard
            let width = track.width,
            let height = track.height,
            width > 0,
            height > 0
        else {
            return nil
        }
        return ResolvedMaximumVideoResolution(width: width, height: height)
    }
    guard !candidates.isEmpty else {
        return nil
    }

    return candidates.min { lhs, rhs in
        let lhsScore = resolvedMaximumVideoResolutionReferenceScore(
            lhs,
            requestedWidth: requestedWidth,
            requestedHeight: requestedHeight
        )
        let rhsScore = resolvedMaximumVideoResolutionReferenceScore(
            rhs,
            requestedWidth: requestedWidth,
            requestedHeight: requestedHeight
        )
        if lhsScore != rhsScore {
            return lhsScore < rhsScore
        }
        return lhs.width > rhs.width
    }
}

func resolvedMaximumVideoResolutionReferenceScore(
    _ candidate: ResolvedMaximumVideoResolution,
    requestedWidth: Int?,
    requestedHeight: Int?
) -> (Int, Int, Int, Int, Int) {
    let primaryDistance: Int
    let secondaryDistance: Int
    if let requestedHeight {
        primaryDistance = abs(candidate.height - requestedHeight)
        secondaryDistance = requestedWidth.map { abs(candidate.width - $0) } ?? 0
    } else if let requestedWidth {
        primaryDistance = abs(candidate.width - requestedWidth)
        secondaryDistance = requestedHeight.map { abs(candidate.height - $0) } ?? 0
    } else {
        primaryDistance = 0
        secondaryDistance = 0
    }

    let exceedPenalty =
        (requestedWidth.map { candidate.width > $0 ? 1 : 0 } ?? 0) +
        (requestedHeight.map { candidate.height > $0 ? 1 : 0 } ?? 0)

    return (
        primaryDistance,
        secondaryDistance,
        exceedPenalty,
        Int.max - candidate.width,
        Int.max - candidate.height
    )
}
