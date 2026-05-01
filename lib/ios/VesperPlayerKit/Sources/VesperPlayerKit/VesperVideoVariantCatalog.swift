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

private func resolvedMaximumVideoResolutionReference(
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

private func resolvedMaximumVideoResolutionReferenceScore(
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

func stableVideoVariantTrackId(
    codec: String?,
    peakBitRate: Int64?,
    width: Int?,
    height: Int?,
    frameRate: Double?
) -> String {
    let frameRateBucket = frameRate.flatMap { value -> Int? in
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int((value * 100).rounded())
    }

    let components = [
        "c\(sanitizedStableVideoVariantTrackIdComponent(codec))",
        "b\(peakBitRate.map(String.init) ?? "na")",
        "w\(width.map(String.init) ?? "na")",
        "h\(height.map(String.init) ?? "na")",
        "f\(frameRateBucket.map(String.init) ?? "na")",
    ]
    return "video:hls:" + components.joined(separator: ":")
}

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

private func sanitizedStableVideoVariantTrackIdComponent(_ value: String?) -> String {
    let rawValue = value?.lowercased() ?? "na"
    let sanitizedScalars = rawValue.unicodeScalars.map { scalar -> UnicodeScalar in
        if CharacterSet.alphanumerics.contains(scalar) {
            return scalar
        }
        return "_"
    }
    let sanitized = String(String.UnicodeScalarView(sanitizedScalars))
        .replacingOccurrences(of: "_+", with: "_", options: .regularExpression)
        .trimmingCharacters(in: CharacterSet(charactersIn: "_"))
    return sanitized.isEmpty ? "na" : sanitized
}

private struct StableVideoVariantFingerprint {
    let codecComponent: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRateBucket: Int?

    init?(trackId: String) {
        let components = trackId.split(separator: ":")
        guard components.count >= 7, components[0] == "video", components[1] == "hls" else {
            return nil
        }

        var codecComponent: String?
        var peakBitRate: Int64?
        var width: Int?
        var height: Int?
        var frameRateBucket: Int?

        for component in components.dropFirst(2) {
            guard let prefix = component.first else {
                continue
            }
            let rawValue = String(component.dropFirst())
            switch prefix {
            case "c":
                codecComponent = rawValue == "na" ? nil : rawValue
            case "b":
                peakBitRate = rawValue == "na" ? nil : Int64(rawValue)
            case "w":
                width = rawValue == "na" ? nil : Int(rawValue)
            case "h":
                height = rawValue == "na" ? nil : Int(rawValue)
            case "f":
                frameRateBucket = rawValue == "na" ? nil : Int(rawValue)
            default:
                continue
            }
        }

        self.codecComponent = codecComponent
        self.peakBitRate = peakBitRate
        self.width = width
        self.height = height
        self.frameRateBucket = frameRateBucket
    }

    init(track: VesperMediaTrack) {
        codecComponent = track.codec.map(sanitizedStableVideoVariantTrackIdComponent)
        peakBitRate = track.bitRate
        width = track.width
        height = track.height
        frameRateBucket = track.frameRate.flatMap { value in
            guard value.isFinite, value > 0 else {
                return nil
            }
            return Int((Double(value) * 100).rounded())
        }
    }

    var hasComparableFields: Bool {
        codecComponent != nil ||
            peakBitRate != nil ||
            width != nil ||
            height != nil ||
            frameRateBucket != nil
    }
}

private struct RequestedVideoVariantTrackScore: Comparable {
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

private func requestedVideoVariantTrackScore(
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

private func requestedCodecPenalty(_ requested: String?, _ candidate: String?) -> Int {
    guard let requested else {
        return 0
    }
    guard let candidate else {
        return 1
    }
    return requested == candidate ? 0 : 3
}

private func requestedVariantDistance(
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

private func requestedVariantDistance(
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

private func preferredVideoVariantTrack(
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

private func fourCharCodeString(_ value: UInt32) -> String {
    let scalarValues = [
        UInt8((value >> 24) & 0xFF),
        UInt8((value >> 16) & 0xFF),
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
    let printable = scalarValues.allSatisfy { (0x20 ... 0x7E).contains($0) }
    guard printable else {
        return String(format: "0x%08X", value)
    }
    return String(bytes: scalarValues, encoding: .ascii) ?? String(format: "0x%08X", value)
}
