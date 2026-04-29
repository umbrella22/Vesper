import Foundation

public enum VesperPlayerSourceKind: String, Equatable {
    case local
    case remote
}

public enum VesperPlayerSourceProtocol: String, Equatable {
    case unknown
    case file
    case content
    case progressive
    case hls
    case dash
}

public struct VesperPlayerSource: Equatable {
    public let uri: String
    public let label: String
    public let kind: VesperPlayerSourceKind
    public let `protocol`: VesperPlayerSourceProtocol
    public let headers: [String: String]

    public init(
        uri: String,
        label: String,
        kind: VesperPlayerSourceKind,
        protocol: VesperPlayerSourceProtocol,
        headers: [String: String] = [:],
    ) {
        self.uri = uri
        self.label = label
        self.kind = kind
        self.protocol = `protocol`
        self.headers = headers
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
    ) -> VesperPlayerSource {
        VesperPlayerSource(
            uri: url.absoluteString,
            label: label ?? url.absoluteString,
            kind: .remote,
            protocol: `protocol` ?? inferRemoteProtocol(for: url),
            headers: headers
        )
    }

    public static func hls(
        url: URL,
        label: String? = nil,
        headers: [String: String] = [:]
    ) -> VesperPlayerSource {
        remoteUrl(url, label: label, protocol: .hls, headers: headers)
    }

    public static func dash(
        url: URL,
        label: String? = nil,
        headers: [String: String] = [:]
    ) -> VesperPlayerSource {
        remoteUrl(url, label: label, protocol: .dash, headers: headers)
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
