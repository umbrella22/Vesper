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

/// An external subtitle track to attach to a `VesperPlayerSource`.
///
/// Unlike Android (where ExoPlayer's TextRenderer parses SRT/WebVTT natively),
/// AVPlayer does not consume standalone SRT files. The iOS host kit parses
/// side-loaded subtitles and renders them through a dedicated overlay driven
/// by `AVPlayer.currentTime()`.
public struct VesperExternalSubtitleSource: Equatable, Codable {
    /// Source-local stable identity used for selection and diagnostics.
    public let id: String
    /// Subtitle file URI (local `file://`, or remote `https://`).
    public let uri: String
    /// Subtitle codec MIME type. Unknown values are preserved so a newer
    /// backend can add support without changing the public DTO.
    public let mimeType: String
    /// Optional BCP-47 language tag for track selection.
    public let language: String?
    /// Optional human-readable label.
    public let label: String?
    /// Request headers used only for this subtitle resource.
    public let headers: [String: String]
    /// Whether automatic selection should prefer this source.
    public let isDefault: Bool
    /// Whether this source is forced narrative text.
    public let isForced: Bool
    public init(
        id: String,
        uri: String,
        mimeType: String = VesperExternalSubtitleSource.mimeSubrip,
        language: String? = nil,
        label: String? = nil,
        headers: [String: String] = [:],
        isDefault: Bool = false,
        isForced: Bool = false
    ) {
        self.id = id
        self.uri = uri
        self.mimeType = mimeType
        self.language = language
        self.label = label
        self.headers = headers
        self.isDefault = isDefault
        self.isForced = isForced
    }

    public static let mimeSubrip = "application/x-subrip"
    public static let mimeWebVtt = "text/vtt"
    public static let mimeSsa = "text/x-ssa"
}

/// @deprecated Use `VesperExternalSubtitleSource`.
@available(*, deprecated, message: "Use VesperExternalSubtitleSource instead.")
public typealias VesperSubtitleSideLoad = VesperExternalSubtitleSource
