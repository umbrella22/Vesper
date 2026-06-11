@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

extension VesperDashSession {
    nonisolated var masterPlaylistURL: URL {
        Self.localURL(host: "master", pathComponents: [id, "master.m3u8"])
    }

    nonisolated func mediaPlaylistURL(for renditionId: String) -> URL {
        Self.localURL(host: "media", pathComponents: [id, renditionId + ".m3u8"])
    }

    nonisolated func segmentURL(for renditionId: String, segment: VesperDashSegmentRequest) -> URL {
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = "init.mp4"
        case let .media(index):
            segmentName = "\(index).m4s"
        }
        return Self.localURL(host: "segment", pathComponents: [id, renditionId, segmentName])
    }

    nonisolated static func localURL(host: String, pathComponents: [String]) -> URL {
        var components = URLComponents()
        components.scheme = scheme
        components.host = host
        components.percentEncodedPath = "/" + pathComponents
            .map { $0.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters) ?? $0 }
            .joined(separator: "/")
        if let url = components.url {
            return url
        }
        iosHostLog("failed to construct DASH local URL for host=\(host)")
        return URL(fileURLWithPath: "/")
    }

    nonisolated func route(for url: URL) -> VesperDashRoute? {
        guard url.scheme == Self.scheme else { return nil }
        let encodedPath = URLComponents(url: url, resolvingAgainstBaseURL: false)?.percentEncodedPath
            ?? url.path
        let components = encodedPath
            .split(separator: "/")
            .map(String.init)
        guard components.first == id else { return nil }

        switch url.host {
        case "master":
            return .master
        case "media":
            guard components.count >= 2 else { return nil }
            var encodedId = components[1]
            if encodedId.hasSuffix(".m3u8") {
                encodedId.removeLast(".m3u8".count)
            }
            return .media(encodedId.removingPercentEncoding ?? encodedId)
        case "segment":
            guard components.count >= 3 else { return nil }
            let renditionId = components[1].removingPercentEncoding ?? components[1]
            let segmentName = components[2]
            if segmentName == "init.mp4" {
                return .segment(renditionId, .initialization)
            }
            guard segmentName.hasSuffix(".m4s") else { return nil }
            let indexText = String(segmentName.dropLast(".m4s".count))
            guard let index = Int(indexText), index >= 0 else { return nil }
            return .segment(renditionId, .media(index))
        default:
            return nil
        }
    }
}
