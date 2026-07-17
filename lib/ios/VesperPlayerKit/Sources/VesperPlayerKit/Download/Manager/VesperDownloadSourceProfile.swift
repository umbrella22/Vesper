import Combine
import Foundation
import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif
public struct VesperDownloadSource: Equatable, Codable {
    public let source: VesperPlayerSource
    public let contentFormat: VesperDownloadContentFormat
    public let manifestUri: String?

    public init(
        source: VesperPlayerSource,
        contentFormat: VesperDownloadContentFormat? = nil,
        manifestUri: String? = nil
    ) {
        self.source = source
        self.contentFormat = contentFormat ?? Self.inferContentFormat(for: source)
        self.manifestUri = manifestUri
    }

    private static func inferContentFormat(for source: VesperPlayerSource) -> VesperDownloadContentFormat {
        switch source.protocol {
        case .hls:
            return .hlsSegments
        case .dash:
            return .dashSegments
        case .file, .content, .progressive:
            return .singleFile
        case .unknown, .rtmp, .rtsp, .flv:
            // Live streaming protocols (RTMP/RTSP/FLV) are continuous streams,
            // not segment-based downloads. The planner treats them as Unknown
            // and rejects download attempts with a capability error rather
            // than silently producing an empty task.
            return .unknown
        }
    }
}

public struct VesperDownloadProfile: Equatable, Codable {
    public let variantId: String?
    public let preferredAudioLanguage: String?
    public let preferredSubtitleLanguage: String?
    public let selectedTrackIds: [String]
    public let targetOutputFormat: VesperDownloadOutputFormat?
    public let targetDirectory: URL?
    public let allowMeteredNetwork: Bool

    public init(
        variantId: String? = nil,
        preferredAudioLanguage: String? = nil,
        preferredSubtitleLanguage: String? = nil,
        selectedTrackIds: [String] = [],
        targetOutputFormat: VesperDownloadOutputFormat? = nil,
        targetDirectory: URL? = nil,
        allowMeteredNetwork: Bool = false
    ) {
        self.variantId = variantId
        self.preferredAudioLanguage = preferredAudioLanguage
        self.preferredSubtitleLanguage = preferredSubtitleLanguage
        self.selectedTrackIds = selectedTrackIds
        self.targetOutputFormat = targetOutputFormat
        self.targetDirectory = targetDirectory
        self.allowMeteredNetwork = allowMeteredNetwork
    }
}
