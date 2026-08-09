import Foundation

public enum VesperMediaTrackKind: String, Equatable {
    case video
    case audio
    case subtitle
}

public enum VesperTrackSupportStatus: String, Equatable {
    case supported
    case exceedsCapabilities
    case unsupported
    case unknown
}

public enum VesperTrackSupportReason: String, Equatable {
    case none
    case formatExceedsCapabilities
    case unsupportedType
    case unsupportedSubtype
    case unsupportedDrm
    case routeUnavailable
    case presentationUnavailable
    case runtimeFailure
    case platformUnknown
    case unknown
}

public enum VesperTrackSupportSource: String, Equatable {
    case runtimeTrackCatalog
    case capabilityProbe
    case runtimeFailure
    case unavailable
    case unknown
}

public struct VesperTrackSupportDiagnostics: Equatable {
    public let decoderName: String?
    public let surfaceKind: String?
    public let hdrType: String?
    public let secureDecoderRequired: Bool?
    public let secureOutputRequired: Bool?

    public init(
        decoderName: String? = nil,
        surfaceKind: String? = nil,
        hdrType: String? = nil,
        secureDecoderRequired: Bool? = nil,
        secureOutputRequired: Bool? = nil
    ) {
        self.decoderName = decoderName
        self.surfaceKind = surfaceKind
        self.hdrType = hdrType
        self.secureDecoderRequired = secureDecoderRequired
        self.secureOutputRequired = secureOutputRequired
    }
}

public struct VesperTrackSupport: Equatable {
    public let status: VesperTrackSupportStatus
    public let reason: VesperTrackSupportReason
    public let source: VesperTrackSupportSource
    public let statusRawValue: String?
    public let reasonRawValue: String?
    public let sourceRawValue: String?
    public let playbackPath: String?
    public let formatSupportRawValue: String?
    public let diagnostics: VesperTrackSupportDiagnostics

    public init(
        status: VesperTrackSupportStatus = .unknown,
        reason: VesperTrackSupportReason = .platformUnknown,
        source: VesperTrackSupportSource = .unavailable,
        statusRawValue: String? = nil,
        reasonRawValue: String? = nil,
        sourceRawValue: String? = nil,
        playbackPath: String? = nil,
        formatSupportRawValue: String? = nil,
        diagnostics: VesperTrackSupportDiagnostics = VesperTrackSupportDiagnostics()
    ) {
        self.status = status
        self.reason = reason
        self.source = source
        self.statusRawValue = statusRawValue
        self.reasonRawValue = reasonRawValue
        self.sourceRawValue = sourceRawValue
        self.playbackPath = playbackPath
        self.formatSupportRawValue = formatSupportRawValue
        self.diagnostics = diagnostics
    }

    public var canAttemptExplicitSelection: Bool {
        status == .supported || status == .unknown
    }
}

public struct VesperMediaTrack: Equatable, Identifiable {
    public let id: String
    public let kind: VesperMediaTrackKind
    public let label: String?
    public let language: String?
    public let codec: String?
    public let bitRate: Int64?
    public let width: Int?
    public let height: Int?
    public let frameRate: Double?
    public let channels: Int?
    public let sampleRate: Int?
    public let isDefault: Bool
    public let isForced: Bool
    public let support: VesperTrackSupport

    public init(
        id: String,
        kind: VesperMediaTrackKind,
        label: String? = nil,
        language: String? = nil,
        codec: String? = nil,
        bitRate: Int64? = nil,
        width: Int? = nil,
        height: Int? = nil,
        frameRate: Double? = nil,
        channels: Int? = nil,
        sampleRate: Int? = nil,
        isDefault: Bool = false,
        isForced: Bool = false,
        support: VesperTrackSupport = VesperTrackSupport(),
    ) {
        self.id = id
        self.kind = kind
        self.label = label
        self.language = language
        self.codec = codec
        self.bitRate = bitRate
        self.width = width
        self.height = height
        self.frameRate = frameRate
        self.channels = channels
        self.sampleRate = sampleRate
        self.isDefault = isDefault
        self.isForced = isForced
        self.support = support
    }
}

public struct VesperTrackCatalog: Equatable {
    public let tracks: [VesperMediaTrack]
    public let adaptiveVideo: Bool
    public let adaptiveAudio: Bool
    public let catalogRevision: Int64
    public let playbackPath: String?

    public init(
        tracks: [VesperMediaTrack] = [],
        adaptiveVideo: Bool = false,
        adaptiveAudio: Bool = false,
        catalogRevision: Int64 = 0,
        playbackPath: String? = nil,
    ) {
        self.tracks = tracks
        self.adaptiveVideo = adaptiveVideo
        self.adaptiveAudio = adaptiveAudio
        self.catalogRevision = max(0, catalogRevision)
        self.playbackPath = playbackPath
    }

    public var videoTracks: [VesperMediaTrack] {
        tracks.filter { $0.kind == .video }
    }

    public var audioTracks: [VesperMediaTrack] {
        tracks.filter { $0.kind == .audio }
    }

    public var subtitleTracks: [VesperMediaTrack] {
        tracks.filter { $0.kind == .subtitle }
    }

    public static let empty = VesperTrackCatalog()
}

extension VesperMediaTrack {
    /// Applies the host playback-path context to a track catalog entry.
    ///
    /// AVPlayer does not expose a per-variant decoder guarantee equivalent to
    /// Media3's format-support result. Entries that have no stronger evidence
    /// therefore remain `unknown`, while the catalog source and path identify
    /// where the observation came from.
    func catalogEntry(forPlaybackPath playbackPath: String) -> VesperMediaTrack {
        let catalogSource: VesperTrackSupportSource =
            support.source == .unavailable ? .runtimeTrackCatalog : support.source
        let catalogSupport = VesperTrackSupport(
            status: support.status,
            reason: support.reason,
            source: catalogSource,
            statusRawValue: support.statusRawValue,
            reasonRawValue: support.reasonRawValue,
            sourceRawValue: support.sourceRawValue,
            playbackPath: playbackPath,
            formatSupportRawValue: support.formatSupportRawValue,
            diagnostics: support.diagnostics
        )
        return VesperMediaTrack(
            id: id,
            kind: kind,
            label: label,
            language: language,
            codec: codec,
            bitRate: bitRate,
            width: width,
            height: height,
            frameRate: frameRate,
            channels: channels,
            sampleRate: sampleRate,
            isDefault: isDefault,
            isForced: isForced,
            support: catalogSupport
        )
    }
}
