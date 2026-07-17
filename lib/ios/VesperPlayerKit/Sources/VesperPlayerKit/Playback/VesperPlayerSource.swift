import Foundation

public enum VesperPlayerSourceKind: String, Equatable, Codable {
    case local
    case remote
}

public enum VesperPlayerSourceProtocol: String, Equatable, Codable {
    case unknown
    case file
    case content
    case progressive
    case hls
    case dash
    case rtmp
    case rtsp
    case flv
}

public struct VesperPlayerDrmConfiguration: Equatable, Codable {
    public let keySystem: String
    public let licenseUri: String
    public let licenseHeaders: [String: String]
    public let fairPlayCertificateUri: String?
    public let fairPlayCertificateBase64: String?
    public let multiSession: Bool

    public init(
        keySystem: String,
        licenseUri: String,
        licenseHeaders: [String: String] = [:],
        fairPlayCertificateUri: String? = nil,
        fairPlayCertificateBase64: String? = nil,
        multiSession: Bool = false
    ) {
        self.keySystem = keySystem
        self.licenseUri = licenseUri
        self.licenseHeaders = licenseHeaders
        self.fairPlayCertificateUri = fairPlayCertificateUri
        self.fairPlayCertificateBase64 = fairPlayCertificateBase64
        self.multiSession = multiSession
    }
}

public struct VesperPlayerSource: Equatable, Codable {
    public let uri: String
    public let label: String
    public let kind: VesperPlayerSourceKind
    public let `protocol`: VesperPlayerSourceProtocol
    public let headers: [String: String]
    public let drmConfiguration: VesperPlayerDrmConfiguration?
    /// Optional side-loaded external subtitle tracks (SRT/ASS/WebVTT URIs).
    ///
    /// Unlike Android (where ExoPlayer's TextRenderer consumes them
    /// natively), AVPlayer does not parse standalone SRT files. The iOS host
    /// kit renders side-loaded subtitles through a dedicated overlay; see
    /// `VesperSubtitleOverlayRenderer`.
    public let subtitleConfigurations: [VesperSubtitleSideLoad]

    public init(
        uri: String,
        label: String,
        kind: VesperPlayerSourceKind,
        protocol: VesperPlayerSourceProtocol,
        headers: [String: String] = [:],
        drmConfiguration: VesperPlayerDrmConfiguration? = nil,
        subtitleConfigurations: [VesperSubtitleSideLoad] = []
    ) {
        self.uri = uri
        self.label = label
        self.kind = kind
        self.protocol = `protocol`
        self.headers = headers
        self.drmConfiguration = drmConfiguration
        self.subtitleConfigurations = subtitleConfigurations
    }

    public static func localFile(url: URL, label: String? = nil) -> VesperPlayerSource {
        VesperPlayerSource(
            uri: url.absoluteString,
            label: label ?? url.lastPathComponent,
            kind: .local,
            protocol: inferLocalProtocol(for: url)
        )
    }

    public static func remoteUrl(
        _ url: URL,
        label: String? = nil,
        protocol: VesperPlayerSourceProtocol? = nil,
        headers: [String: String] = [:],
        drmConfiguration: VesperPlayerDrmConfiguration? = nil,
    ) -> VesperPlayerSource {
        VesperPlayerSource(
            uri: url.absoluteString,
            label: label ?? url.absoluteString,
            kind: .remote,
            protocol: `protocol` ?? inferRemoteProtocol(for: url),
            headers: headers,
            drmConfiguration: drmConfiguration
        )
    }

    public static func hls(
        url: URL,
        label: String? = nil,
        headers: [String: String] = [:],
        drmConfiguration: VesperPlayerDrmConfiguration? = nil
    ) -> VesperPlayerSource {
        remoteUrl(
            url,
            label: label,
            protocol: .hls,
            headers: headers,
            drmConfiguration: drmConfiguration
        )
    }

    public static func dash(
        url: URL,
        label: String? = nil,
        headers: [String: String] = [:],
        drmConfiguration: VesperPlayerDrmConfiguration? = nil
    ) -> VesperPlayerSource {
        remoteUrl(
            url,
            label: label,
            protocol: .dash,
            headers: headers,
            drmConfiguration: drmConfiguration
        )
    }

    private static func inferLocalProtocol(for url: URL) -> VesperPlayerSourceProtocol {
        switch url.scheme?.lowercased() {
        case "file":
            .file
        case "content":
            .content
        default:
            .unknown
        }
    }

    private static func inferRemoteProtocol(for url: URL) -> VesperPlayerSourceProtocol {
        let lowercased = url.absoluteString.lowercased()
        let lowercasedPath = lowercased
            .split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false)
            .first
            .map(String.init) ?? lowercased
        let normalizedPath = lowercasedPath
            .split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false)
            .first
            .map(String.init) ?? lowercasedPath
        if let scheme = url.scheme?.lowercased() {
            if scheme == "rtmp" || scheme == "rtmps" {
                return .rtmp
            }
            if scheme == "rtsp" || scheme == "rtsps" {
                return .rtsp
            }
        }
        if normalizedPath.hasSuffix(".m3u8") {
            return .hls
        }
        if normalizedPath.hasSuffix(".mpd") {
            return .dash
        }
        if let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" {
            return .progressive
        }
        return .unknown
    }
}
