import Combine
import Foundation
internal import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif

@usableFromInline let vesperDownloadDefaultMinProgressBytes: UInt64 = 512 * 1024
@usableFromInline let vesperDownloadDefaultMinProgressIntervalMs: UInt64 = 250
@usableFromInline let vesperDownloadDefaultStalledTransferTimeoutMs: UInt64 = 30_000

public typealias VesperDownloadAssetId = String
public typealias VesperDownloadTaskId = UInt64

let vesperDownloadATSFailureMessage =
    "iOS offline downloads require HTTPS media URLs. The SDK does not relax App Transport Security for http:// resources; host apps that need insecure HTTP must fetch those resources outside the SDK and provide local file URLs."

public enum VesperDownloadContentFormat: Int, Equatable, Codable {
    case hlsSegments = 0
    case dashSegments = 1
    case flvSegments = 2
    case singleFile = 3
    case unknown = 4
}

public enum VesperDownloadOutputFormat: Int, Equatable, Codable {
    case mp4 = 0
    case mkv = 1
    case original = 2
}

public struct VesperDownloadConfiguration: Equatable {
    public let autoStart: Bool
    public let runPostProcessorsOnCompletion: Bool
    public let resumePartialDownloads: Bool
    public let restoreTasksOnStartup: Bool
    public let baseDirectory: URL?
    public let pluginLibraryPaths: [String]
    public let rangeChunkBytes: UInt64?
    public let minProgressBytes: UInt64
    public let minProgressIntervalMs: UInt64
    public let stalledTransferTimeoutMs: UInt64

    public init(
        autoStart: Bool = true,
        runPostProcessorsOnCompletion: Bool = true,
        resumePartialDownloads: Bool = true,
        restoreTasksOnStartup: Bool = true,
        baseDirectory: URL? = nil,
        pluginLibraryPaths: [String] = [],
        rangeChunkBytes: UInt64? = nil,
        minProgressBytes: UInt64 = vesperDownloadDefaultMinProgressBytes,
        minProgressIntervalMs: UInt64 = vesperDownloadDefaultMinProgressIntervalMs,
        stalledTransferTimeoutMs: UInt64 = vesperDownloadDefaultStalledTransferTimeoutMs
    ) {
        self.autoStart = autoStart
        self.runPostProcessorsOnCompletion = runPostProcessorsOnCompletion
        self.resumePartialDownloads = resumePartialDownloads
        self.restoreTasksOnStartup = restoreTasksOnStartup
        self.baseDirectory = baseDirectory
        self.pluginLibraryPaths = pluginLibraryPaths
        self.rangeChunkBytes = rangeChunkBytes.flatMap { $0 > 0 ? $0 : nil }
        self.minProgressBytes = max(minProgressBytes, 1)
        self.minProgressIntervalMs = minProgressIntervalMs
        self.stalledTransferTimeoutMs = stalledTransferTimeoutMs
    }
}
