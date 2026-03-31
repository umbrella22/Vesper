import Foundation

public enum VesperMediaTrackKind: String, Equatable {
    case video
    case audio
    case subtitle
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
    }
}

public struct VesperTrackCatalog: Equatable {
    public let tracks: [VesperMediaTrack]
    public let adaptiveVideo: Bool
    public let adaptiveAudio: Bool

    public init(
        tracks: [VesperMediaTrack] = [],
        adaptiveVideo: Bool = false,
        adaptiveAudio: Bool = false,
    ) {
        self.tracks = tracks
        self.adaptiveVideo = adaptiveVideo
        self.adaptiveAudio = adaptiveAudio
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
