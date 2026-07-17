import Foundation

/// Minimal subtitle styling shared by the stable mobile host kits.
///
/// Per-cue typography, animation, and layout remain platform- or
/// content-specific concerns.
public struct VesperSubtitleStyle: Equatable, Codable {
    /// Text scale factor relative to the platform default. `1.0` keeps default.
    public let fontScale: Float
    /// Whether subtitle rendering is visible.
    public let visible: Bool

    public init(fontScale: Float = 1.0, visible: Bool = true) {
        self.fontScale = fontScale
        self.visible = visible
    }

    public static let `default` = VesperSubtitleStyle()
}

/// A side-loaded external subtitle track to attach to a `VesperPlayerSource`.
///
/// Unlike Android (where ExoPlayer's TextRenderer parses SRT/WebVTT natively),
/// AVPlayer does not consume standalone SRT files. The iOS host kit parses
/// side-loaded subtitles and renders them through a dedicated overlay driven
/// by `AVPlayer.currentTime()`.
public struct VesperSubtitleSideLoad: Equatable, Codable {
    /// Subtitle file URI (local `file://`, or remote `https://`).
    public let uri: String
    /// Subtitle codec MIME hint.
    public let mimeType: VesperSubtitleMimeType
    /// Optional BCP-47 language tag for track selection.
    public let language: String?
    /// Optional human-readable label.
    public let label: String?
    public init(
        uri: String,
        mimeType: VesperSubtitleMimeType = .subrip,
        language: String? = nil,
        label: String? = nil
    ) {
        self.uri = uri
        self.mimeType = mimeType
        self.language = language
        self.label = label
    }
}

/// Subtitle codec hint for side-loaded tracks.
public enum VesperSubtitleMimeType: String, Equatable, Codable {
    /// SRT (`application/x-subrip`).
    case subrip
    /// WebVTT (`text/vtt`).
    case webvtt
    /// SSA / ASS (`text/x-ssa`).
    case ssa

    /// MIME type string consumed by parsers.
    public var rawMime: String {
        switch self {
        case .subrip: return "application/x-subrip"
        case .webvtt: return "text/vtt"
        case .ssa: return "text/x-ssa"
        }
    }

    /// Parses a MIME string into the coarse hint, falling back to SRT.
    public static func from(rawMime: String?) -> VesperSubtitleMimeType {
        guard let raw = rawMime?.lowercased() else { return .subrip }
        if raw.contains("vtt") { return .webvtt }
        if raw.contains("ass") || raw.contains("ssa") { return .ssa }
        return .subrip
    }
}
