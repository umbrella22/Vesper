@preconcurrency import AVFoundation
import Foundation
internal import VesperPlayerKitBridgeShim

actor VesperDashSession {
    typealias BenchmarkEventRecorder = @MainActor @Sendable (String, [String: String]) -> Void
    typealias VideoDecodeCapabilityProvider = @Sendable (
        VesperDashPlayableRepresentation
    ) -> VesperDashVideoDecodeCapability

    nonisolated static let scheme = "vesper-dash"
    nonisolated static let segmentCacheMaxBytes: UInt64 = 256 * 1024 * 1024
    nonisolated static let segmentCacheMaxEntryCount = 160
    nonisolated static let segmentCacheMaxSingleMediaBytes: UInt64 = 32 * 1024 * 1024
    nonisolated static let startupMediaSegmentPrefetchLimit = 2
    nonisolated static let defaultDynamicManifestRefreshIntervalMs: UInt64 = 2_000
    nonisolated static let minimumDynamicManifestRefreshIntervalMs: UInt64 = 500

    nonisolated let id: String
    nonisolated let sourceURL: URL
    nonisolated let segmentCacheDirectory: URL

    let networkClient: VesperDashNetworkClient
    var manifest: VesperDashManifest?
    var manifestLoadedAt: Date?
    var masterPlaylistCache: Data?
    var mediaPlaylistCacheByRenditionId: [String: Data] = [:]
    var manifestLoadTask: Task<VesperDashManifest, Error>?
    var mediaPlaylistTasksByRenditionId: [String: Task<Data, Error>] = [:]
    var sidxLoadTasksByRenditionId: [String: Task<VesperDashSidxBox, Error>] = [:]
    var segmentDownloadTasksByKey: [VesperDashSegmentCacheKey: Task<VesperDashSegmentPayloadResult, Error>] = [:]
    var selectedPlayableByPolicy: [VesperDashMasterPlaylistVariantPolicy: VesperDashSelectedPlayableResponse] = [:]
    var playableByRenditionId: [String: VesperDashPlayableRepresentation] = [:]
    var videoDecodeCapabilitiesCache: [VesperDashVideoDecodeCapability]?
    var sidxByRenditionId: [String: VesperDashSidxBox] = [:]
    var mediaSegmentsByRenditionId: [String: [VesperDashMediaSegment]] = [:]
    var templateSegmentsByRenditionId: [String: [VesperDashTemplateSegment]] = [:]
    var cachedSegmentFiles: [VesperDashSegmentCacheKey: VesperDashCachedSegmentFile] = [:]
    var segmentCacheTotalBytes: UInt64 = 0
    var backgroundPrefetchRenditionIds: Set<String> = []
    var backgroundPrefetchLargeMediaRenditionIds: Set<String> = []
    let videoDecodeCapabilityProvider: VideoDecodeCapabilityProvider
    let benchmarkEventRecorder: BenchmarkEventRecorder?

    init(
        sourceURL: URL,
        headers: [String: String] = [:],
        networkClient: VesperDashNetworkClient? = nil,
        videoDecodeCapabilityProvider: VideoDecodeCapabilityProvider? = nil,
        benchmarkEventRecorder: BenchmarkEventRecorder? = nil
    ) {
        let sessionId = UUID().uuidString
        id = sessionId
        self.sourceURL = sourceURL
        segmentCacheDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-dash-\(sessionId)", isDirectory: true)
        self.networkClient = networkClient ?? VesperDashNetworkClient(headers: headers)
        if let videoDecodeCapabilityProvider {
            self.videoDecodeCapabilityProvider = videoDecodeCapabilityProvider
        } else {
            self.videoDecodeCapabilityProvider = { playable in
                Self.defaultVideoDecodeCapability(for: playable)
            }
        }
        self.benchmarkEventRecorder = benchmarkEventRecorder
    }

    deinit {
        removeFileIfPresent(
            segmentCacheDirectory,
            context: "DASH segment cache directory"
        )
    }
}
