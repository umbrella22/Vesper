@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    nonisolated var masterPlaylistURL: URL {
        Self.localURL(host: "master", pathComponents: [id, "master.m3u8"])
    }

    nonisolated func mediaPlaylistURL(for renditionId: String) -> URL {
        Self.localURL(host: "media", pathComponents: [id, renditionId + ".m3u8"])
    }

    nonisolated func segmentURL(
        for renditionId: String,
        segment: VesperDashSegmentRequest,
        fileExtension: String? = nil
    ) -> URL {
        // When `fileExtension` is provided (e.g. `"vtt"` for WebVTT subtitle
        // renditions), use it instead of the default `.m4s`/`init.mp4` so
        // the AVPlayer resource loader receives a MIME-aware URL. This is
        // required for WebVTT subtitle segments where `.m4s` would mislabel
        // the payload.
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = fileExtension.map { "init.\($0)" } ?? "init.mp4"
        case let .media(index):
            segmentName = fileExtension.map { "\(index).\($0)" } ?? "\(index).m4s"
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
            if segmentName == "init.mp4" || segmentName == "init.vtt" {
                return .segment(renditionId, .initialization)
            }
            // Media segments may use either `.m4s` (audio/video) or `.vtt`
            // (WebVTT subtitle renditions). The route must accept both so
            // the resource loader can serve MIME-aware subtitle URLs.
            let knownSuffixes = [".m4s", ".vtt"]
            guard let suffix = knownSuffixes.first(where: { segmentName.hasSuffix($0) }) else {
                return nil
            }
            let indexText = String(segmentName.dropLast(suffix.count))
            guard let index = Int(indexText), index >= 0 else { return nil }
            return .segment(renditionId, .media(index))
        default:
            return nil
        }
    }
}
