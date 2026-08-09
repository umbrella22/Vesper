@preconcurrency import AVFoundation
import CoreGraphics
import Foundation

struct LoadedTrackCatalogState {
    let catalog: VesperTrackCatalog
    let audioGroup: AVMediaSelectionGroup?
    let subtitleGroup: AVMediaSelectionGroup?
    let videoVariantPinsByTrackId: [String: LoadedVideoVariantPin]
    let audioOptionsByTrackId: [String: AVMediaSelectionOption]
    let subtitleOptionsByTrackId: [String: AVMediaSelectionOption]
    /// Subtitle lifecycle state derived during catalog loading. Drives the
    /// Flutter subtitle state channel.
    let subtitleState: VesperSubtitleState
}

struct TrackCatalogFingerprint: Equatable {
    let sourceEpoch: UInt64
    let playbackPath: String?
    let drmKeySystem: String?
    let tracks: [VesperMediaTrack]
    let adaptiveVideo: Bool
    let adaptiveAudio: Bool
}

struct LoadedVideoVariantState {
    let tracks: [VesperMediaTrack]
    let pinsByTrackId: [String: LoadedVideoVariantPin]

    static let empty = LoadedVideoVariantState(
        tracks: [],
        pinsByTrackId: [:]
    )
}

struct ResolvedMaximumVideoResolution: Equatable {
    let width: Int
    let height: Int
}

struct LoadedVideoVariantPin: Equatable {
    let peakBitRate: Double?
    let maxWidth: Int?
    let maxHeight: Int?

    var hasAnyLimit: Bool {
        peakBitRate != nil || (maxWidth != nil && maxHeight != nil)
    }
}

@available(iOS 15.0, *)
struct LoadedVideoVariantDescriptor: Equatable {
    let codec: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRate: Double?

    init?(_ variant: AVAssetVariant) {
        guard let videoAttributes = variant.videoAttributes else {
            return nil
        }

        let presentationSize = videoAttributes.presentationSize
        let width = LoadedVideoVariantDescriptor.intOrNil(presentationSize.width)
        let height = LoadedVideoVariantDescriptor.intOrNil(presentationSize.height)
        let peakBitRate = variant.peakBitRate.flatMap(
            LoadedVideoVariantDescriptor.bitRateOrNil
        )
        let frameRate = videoAttributes.nominalFrameRate.flatMap(
            LoadedVideoVariantDescriptor.doubleOrNil
        )
        let codec = videoAttributes.codecTypes.first.map { value in
            fourCharCodeString(value)
        }

        guard peakBitRate != nil || (width != nil && height != nil) else {
            return nil
        }

        self.codec = codec
        self.peakBitRate = peakBitRate
        self.width = width
        self.height = height
        self.frameRate = frameRate
    }

    var deduplicationKey: LoadedVideoVariantDeduplicationKey {
        LoadedVideoVariantDeduplicationKey(
            codec: codec,
            peakBitRate: peakBitRate,
            width: width,
            height: height,
            frameRate: frameRate.map { Int(($0 * 100).rounded()) }
        )
    }

    var stableTrackId: String {
        stableVideoVariantTrackId(
            codec: codec,
            peakBitRate: peakBitRate,
            width: width,
            height: height,
            frameRate: frameRate
        )
    }

    var trackLabel: String {
        if let height {
            return "\(height)p"
        }
        if let width, let height {
            return "\(width)x\(height)"
        }
        if let peakBitRate {
            return "\(peakBitRate)"
        }
        return "Video"
    }

    private static func intOrNil(_ value: CGFloat) -> Int? {
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int(value.rounded())
    }

    private static func bitRateOrNil(_ value: Double) -> Int64? {
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int64(value.rounded())
    }

    private static func doubleOrNil(_ value: Double) -> Double? {
        guard value.isFinite, value > 0 else {
            return nil
        }
        return value
    }

    static func preferredOrdering(
        _ lhs: LoadedVideoVariantDescriptor,
        over rhs: LoadedVideoVariantDescriptor
    ) -> LoadedVideoVariantDescriptor {
        let lhsBitRate = lhs.peakBitRate ?? -1
        let rhsBitRate = rhs.peakBitRate ?? -1
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

        return lhs.trackLabel <= rhs.trackLabel ? lhs : rhs
    }
}

struct LoadedVideoVariantDeduplicationKey: Hashable {
    let codec: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRate: Int?
}
