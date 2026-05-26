@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

private let vesperDashATSFailureMessage =
    "iOS DASH playback requires HTTPS media URLs. The SDK does not relax App Transport Security for http:// resources; host apps that need insecure HTTP must fetch those resources outside the SDK and provide local file URLs."
private let vesperDashNetworkStallTimeoutSeconds: TimeInterval = 30
private let vesperDashNetworkResourceTimeoutSeconds: TimeInterval = 60

private struct VesperDashSegmentCacheKey: Hashable {
    let renditionId: String
    let segment: VesperDashSegmentRequest
}

private struct VesperDashCachedSegmentFile {
    let url: URL
    let size: UInt64
    var lastAccessedAt: Date

    var isInitialization: Bool {
        segment == .initialization
    }

    private let segment: VesperDashSegmentRequest

    init(url: URL, size: UInt64, segment: VesperDashSegmentRequest, lastAccessedAt: Date) {
        self.url = url
        self.size = size
        self.segment = segment
        self.lastAccessedAt = lastAccessedAt
    }
}

private enum VesperDashResourceResponse {
    case resource(VesperLocalResourceBody)
    case redirect(URL)
}

enum VesperDashSegmentPayload {
    case data(Data, contentType: String)
    case file(url: URL, offset: UInt64, size: UInt64, removeAfterServing: Bool, contentType: String)

    var size: UInt64 {
        switch self {
        case let .data(data, _):
            return UInt64(data.count)
        case let .file(_, _, size, _, _):
            return size
        }
    }

    var contentType: String {
        switch self {
        case let .data(_, contentType):
            return contentType
        case let .file(_, _, _, _, contentType):
            return contentType
        }
    }

    var isTemporaryFile: Bool {
        if case .file(_, _, _, true, _) = self {
            return true
        }
        return false
    }

    var localResourceBody: VesperLocalResourceBody {
        switch self {
        case let .data(data, contentType):
            .data(data, contentType: avResourceContentType(forSegmentContentType: contentType))
        case let .file(url, offset, size, removeAfterServing, contentType):
            .file(
                url: url,
                offset: offset,
                length: size,
                contentType: avResourceContentType(forSegmentContentType: contentType),
                removeAfterServing: removeAfterServing,
                growingPolicy: nil
            )
        }
    }

    func readData() throws -> Data {
        switch self {
        case let .data(data, _):
            return data
        case let .file(url, offset, size, removeAfterServing, _):
            defer {
                if removeAfterServing {
                    try? FileManager.default.removeItem(at: url)
                }
            }
            let length = try checkedInt(size, field: "segment payload length")
            let handle = try FileHandle(forReadingFrom: url)
            defer { closeFileHandle(handle, context: "segment payload") }
            try handle.seek(toOffset: offset)
            let data = try handle.read(upToCount: length) ?? Data()
            guard data.count == length else {
                throw VesperDashBridgeError.network("segment file is shorter than requested")
            }
            return data
        }
    }

    func cleanupIfTemporary() {
        if case let .file(url, _, _, true, _) = self {
            try? FileManager.default.removeItem(at: url)
        }
    }
}

private func avResourceContentType(forSegmentContentType contentType: String) -> String {
    let normalized = contentType.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    if normalized.contains("vtt") {
        return "public.webvtt"
    }
    if normalized.hasPrefix("public.") {
        return contentType
    }
    return "public.mpeg-4"
}

private struct VesperDashSegmentPayloadResult {
    let payload: VesperDashSegmentPayload
    let cacheHit: Bool
    let segmentType: String
    let byteRange: VesperDashByteRange?
    let delivery: String
    let coalesced: Bool

    init(
        payload: VesperDashSegmentPayload,
        cacheHit: Bool,
        segmentType: String,
        byteRange: VesperDashByteRange?,
        delivery: String,
        coalesced: Bool = false
    ) {
        self.payload = payload
        self.cacheHit = cacheHit
        self.segmentType = segmentType
        self.byteRange = byteRange
        self.delivery = delivery
        self.coalesced = coalesced
    }

    func markingCoalesced(
        payload: VesperDashSegmentPayload? = nil,
        cacheHit: Bool? = nil,
        delivery: String? = nil
    ) -> VesperDashSegmentPayloadResult {
        VesperDashSegmentPayloadResult(
            payload: payload ?? self.payload,
            cacheHit: cacheHit ?? self.cacheHit,
            segmentType: segmentType,
            byteRange: byteRange,
            delivery: delivery ?? self.delivery,
            coalesced: true
        )
    }
}

final class VesperDashResourceLoaderDelegate: NSObject, AVAssetResourceLoaderDelegate {
    let resourceLoadingQueue: DispatchQueue

    private let session: VesperDashSession
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(session: VesperDashSession) {
        self.session = session
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.dash.resource-loader.\(session.id)"
        )
        super.init()
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        shouldWaitForLoadingOfRequestedResource loadingRequest: AVAssetResourceLoadingRequest
    ) -> Bool {
        guard
            let url = loadingRequest.request.url,
            let route = session.route(for: url)
        else {
            return false
        }

        let requestId = ObjectIdentifier(loadingRequest)
        let task = Task { [weak self, session, loadingRequest] in
            do {
                let response: VesperDashResourceResponse
                switch route {
                case .master:
                    response = .resource(
                        .data(
                            try await session.masterPlaylistData(),
                            contentType: "public.m3u-playlist"
                        )
                    )
                case let .media(renditionId):
                    response = .resource(
                        .data(
                            try await session.mediaPlaylistData(renditionId: renditionId),
                            contentType: "public.m3u-playlist"
                        )
                    )
                case let .segment(renditionId, segment):
                    switch segment {
                    case .initialization:
                        // Init segments are small and AVPlayer normally fetches them once, so
                        // return the raw bytes through the resource loader. This keeps init
                        // delivery visible to benchmark events and avoids relying on local HTTP
                        // behavior for EXT-X-MAP.
                        let initData = try await session.segmentData(
                            renditionId: renditionId,
                            segment: .initialization
                        )
#if DEBUG
                        iosHostLog(
                            "dashResourceInit rendition=\(renditionId) bytes=\(initData.count)"
                        )
#endif
                        // contentType must be a UTI, not a MIME type. fMP4 / ISO BMFF maps to public.mpeg-4.
                        response = .resource(.data(initData, contentType: "public.mpeg-4"))
                    case .media:
                        response = .resource(
                            try await session.segmentResourcePayload(
                                renditionId: renditionId,
                                segment: segment
                            ).localResourceBody
                        )
                    }
                }
                self?.finish(loadingRequest, requestId: requestId, response: response)
            } catch {
                self?.finish(loadingRequest, requestId: requestId, error: error)
            }
        }
        tasks[requestId] = task
        return true
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        didCancel loadingRequest: AVAssetResourceLoadingRequest
    ) {
        let requestId = ObjectIdentifier(loadingRequest)
        tasks.removeValue(forKey: requestId)?.cancel()
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        response: VesperDashResourceResponse
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard let self else { return }
            self.tasks.removeValue(forKey: requestId)

            switch response {
            case let .resource(body):
                VesperLocalResourceResponder.finish(loadingRequest, body: body)
            case let .redirect(url):
                var request = URLRequest(url: url)
                request.cachePolicy = .returnCacheDataElseLoad
                loadingRequest.redirect = request
#if DEBUG
                iosHostLog(
                    "dashResourceRedirect from=\(loadingRequest.request.url?.absoluteString ?? "nil") to=\(url.absoluteString)"
                )
#endif
                loadingRequest.response = HTTPURLResponse(
                    url: loadingRequest.request.url ?? url,
                    statusCode: 302,
                    httpVersion: nil,
                    headerFields: ["Location": url.absoluteString]
                )
                loadingRequest.finishLoading()
            }
        }
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        error: Error
    ) {
        resourceLoadingQueue.async { [weak self] in
            self?.tasks.removeValue(forKey: requestId)
            VesperLocalResourceResponder.finish(loadingRequest, error: error)
        }
    }
}

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

    private let networkClient: VesperDashNetworkClient
    private var manifest: VesperDashManifest?
    private var manifestLoadedAt: Date?
    private var masterPlaylistCache: Data?
    private var mediaPlaylistCacheByRenditionId: [String: Data] = [:]
    private var manifestLoadTask: Task<VesperDashManifest, Error>?
    private var mediaPlaylistTasksByRenditionId: [String: Task<Data, Error>] = [:]
    private var sidxLoadTasksByRenditionId: [String: Task<VesperDashSidxBox, Error>] = [:]
    private var segmentDownloadTasksByKey: [VesperDashSegmentCacheKey: Task<VesperDashSegmentPayloadResult, Error>] = [:]
    private var selectedPlayableByPolicy: [VesperDashMasterPlaylistVariantPolicy: VesperDashSelectedPlayableResponse] = [:]
    private var playableByRenditionId: [String: VesperDashPlayableRepresentation] = [:]
    private var videoDecodeCapabilitiesCache: [VesperDashVideoDecodeCapability]?
    private var sidxByRenditionId: [String: VesperDashSidxBox] = [:]
    private var mediaSegmentsByRenditionId: [String: [VesperDashMediaSegment]] = [:]
    private var templateSegmentsByRenditionId: [String: [VesperDashTemplateSegment]] = [:]
    private var cachedSegmentFiles: [VesperDashSegmentCacheKey: VesperDashCachedSegmentFile] = [:]
    private var segmentCacheTotalBytes: UInt64 = 0
    private var backgroundPrefetchRenditionIds: Set<String> = []
    private var backgroundPrefetchLargeMediaRenditionIds: Set<String> = []
    private let videoDecodeCapabilityProvider: VideoDecodeCapabilityProvider
    private let benchmarkEventRecorder: BenchmarkEventRecorder?

    nonisolated var masterPlaylistURL: URL {
        Self.localURL(host: "master", pathComponents: [id, "master.m3u8"])
    }

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
        do {
            try FileManager.default.removeItem(at: segmentCacheDirectory)
        } catch {
            iosHostLog("failed to remove DASH segment cache directory: \(error.localizedDescription)")
        }
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

    nonisolated private static func localURL(host: String, pathComponents: [String]) -> URL {
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

    func masterPlaylistData() async throws -> Data {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent("dash_master_playlist_request_start")
        if let masterPlaylistCache, manifest?.type != .dynamic {
            await recordBenchmarkEvent(
                "dash_master_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: masterPlaylistCache.count,
                    cacheHit: true
                )
            )
            return masterPlaylistCache
        }

        do {
            let manifest = try await loadManifest()
            let variantPolicy = VesperDashMasterPlaylistVariantPolicy.all
            let videoDecodeCapabilities = try videoDecodeCapabilities(for: manifest)
            let playlist = try VesperDashHlsBuilder.buildMasterPlaylist(
                manifest: manifest,
                variantPolicy: variantPolicy,
                videoDecodeCapabilities: videoDecodeCapabilities,
                mediaURL: { [weak self] renditionId in
                    guard let self else { return "" }
                    return self.mediaPlaylistURL(for: renditionId).absoluteString
                }
            )
            let data = Data(playlist.utf8)
            if manifest.type != .dynamic {
                masterPlaylistCache = data
            }

            let startupSelected = try selectedPlayableRepresentations(
                manifest: manifest,
                variantPolicy: .startupSingleVariant
            )
            startStartupPrefetch(for: startupSelected.audio + startupSelected.video, manifest: manifest)
#if DEBUG
            iosHostLog(
                "dashMasterPlaylist policy=all startupVideo=\(startupRenditionSummary(startupSelected.video)) startupAudio=\(startupRenditionSummary(startupSelected.audio))"
            )
#endif
            await recordBenchmarkEvent(
                "dash_master_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: data.count,
                    cacheHit: false,
                    extra: masterPlaylistDecodeSelectionAttributes(startupSelected: startupSelected)
                )
            )
            return data
        } catch {
            await recordBenchmarkEvent(
                "dash_master_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: nil,
                    cacheHit: false,
                    error: error
                )
            )
            throw error
        }
    }

    func manifestTrackCatalogSnapshot() async throws -> VesperDashManifestTrackCatalogSnapshot {
        let manifest = try await loadManifest()
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )
        return VesperDashManifestTrackCatalogSnapshot(
            audio: selected.audio,
            video: selected.video,
            subtitles: selected.subtitles
        )
    }

    func mediaPlaylistData(renditionId: String) async throws -> Data {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent(
            "dash_media_playlist_request_start",
            attributes: ["renditionId": renditionId]
        )
        if let cached = mediaPlaylistCacheByRenditionId[renditionId], manifest?.type != .dynamic {
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: cached.count,
                    cacheHit: true,
                    extra: [
                        "renditionId": renditionId,
                        "coalesced": "false",
                    ]
                )
            )
            return cached
        }

        if let inFlightTask = mediaPlaylistTasksByRenditionId[renditionId] {
            do {
                let data = try await inFlightTask.value
                await recordBenchmarkEvent(
                    "dash_media_playlist_request_end",
                    attributes: playlistBenchmarkEndAttributes(
                        startedAt: startedAt,
                        bytes: data.count,
                        cacheHit: false,
                        extra: [
                            "renditionId": renditionId,
                            "coalesced": "true",
                        ]
                    )
                )
                return data
            } catch {
                await recordBenchmarkEvent(
                    "dash_media_playlist_request_end",
                    attributes: playlistBenchmarkEndAttributes(
                        startedAt: startedAt,
                        bytes: nil,
                        cacheHit: false,
                        error: error,
                        extra: [
                            "renditionId": renditionId,
                            "coalesced": "true",
                        ]
                    )
                )
                throw error
            }
        }

        let buildTask = Task { try await self.buildMediaPlaylistData(renditionId: renditionId) }
        mediaPlaylistTasksByRenditionId[renditionId] = buildTask
        do {
            let data = try await buildTask.value
            mediaPlaylistTasksByRenditionId[renditionId] = nil
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: data.count,
                    cacheHit: false,
                    extra: [
                        "renditionId": renditionId,
                        "coalesced": "false",
                    ]
                )
            )
            return data
        } catch {
            mediaPlaylistTasksByRenditionId[renditionId] = nil
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: nil,
                    cacheHit: false,
                    error: error,
                    extra: [
                        "renditionId": renditionId,
                        "coalesced": "false",
                    ]
                )
            )
            throw error
        }
    }

    private func buildMediaPlaylistData(renditionId: String) async throws -> Data {
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
            if manifest.type == .dynamic {
                throw VesperDashBridgeError.unsupportedManifest(
                    "dynamic DASH SegmentBase is not supported on iOS"
                )
            }
            let segments = try await mediaSegments(for: playable, segmentBase: segmentBase)
            let mediaURL = playable.representation.baseURL
            let playlist = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
                map: VesperDashHlsMap(uri: mediaURL, byteRange: segmentBase.initialization),
                playlistKind: .vod,
                mediaSequence: nil,
                segments: segments.map {
                    VesperDashHlsSegment(
                        duration: $0.duration,
                        uri: mediaURL,
                        byteRange: $0.range
                    )
                }
            )
            let data = Data(playlist.utf8)
            mediaPlaylistCacheByRenditionId[renditionId] = data
            return data
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        let segments = try templateSegments(
            for: playable,
            manifest: manifest,
            segmentTemplate: segmentTemplate
        )
        startBackgroundSegmentPrefetch(
            renditionId: playable.renditionId,
            segmentCount: segments.count,
            prefetchMediaSegments: shouldPrefetchTemplateMediaSegments(
                playable: playable,
                segments: segments
            )
        )
        // Point EXT-X-MAP and media segments at the vesper-dash:// scheme so
        // every DASH-derived HLS resource goes through AVAssetResourceLoaderDelegate.
        // Missing init bytes surface as 'frmt', so the custom scheme keeps
        // delivery deterministic and visible to benchmark events.
        let initializationURL = segmentTemplate.initialization.map { _ in
            self.segmentURL(for: playable.renditionId, segment: .initialization).absoluteString
        }
        let playlistKind: VesperDashHlsPlaylistKind = manifest.type == .dynamic ? .live : .vod
        let mediaSequence = manifest.type == .dynamic ? segments.first?.number : nil
        let playlist = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
            map: initializationURL.map { VesperDashHlsMap(uri: $0, byteRange: nil) },
            playlistKind: playlistKind,
            mediaSequence: mediaSequence,
            segments: try segments.enumerated().map { index, segment in
                let segmentIndex = try hlsSegmentIndex(
                    manifest: manifest,
                    segment: segment,
                    fallbackIndex: index
                )
                let segmentURL = self.segmentURL(for: playable.renditionId, segment: .media(segmentIndex))
                return VesperDashHlsSegment(
                    duration: segment.duration,
                    uri: segmentURL.absoluteString,
                    byteRange: nil
                )
            }
        )
#if DEBUG
        iosHostLog(
            "dashMediaPlaylist rendition=\(playable.renditionId) resourceLoaderSegments=true count=\(segments.count) init=\(initializationURL ?? "none")"
        )
        // Log the first playlist lines to diagnose HLS tag concatenation
        // regressions, such as EXT-X-PLAYLIST-TYPE and EXT-X-MAP being glued
        // onto one line by a missing trailing newline in a multiline string.
        let head = playlist
            .split(separator: "\n", omittingEmptySubsequences: false)
            .prefix(7)
            .joined(separator: " | ")
        iosHostLog("dashMediaPlaylist head=\(head)")
#endif
        let data = Data(playlist.utf8)
        mediaPlaylistCacheByRenditionId[renditionId] = data
        return data
    }

    private func hlsSegmentIndex(
        manifest: VesperDashManifest,
        segment: VesperDashTemplateSegment,
        fallbackIndex: Int
    ) throws -> Int {
        guard manifest.type == .dynamic else {
            return fallbackIndex
        }
        return try checkedInt(segment.number, field: "DASH live segment number")
    }

    private func dashSegmentContentType(
        for playable: VesperDashPlayableRepresentation,
        segment: VesperDashSegmentRequest
    ) -> String {
        if segment == .initialization {
            return "video/mp4"
        }
        let mimeType = playable.representation.mimeType
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        if mimeType == "text/vtt" || mimeType == "text/webvtt" || mimeType.contains("vtt") {
            return "text/vtt"
        }
        return "video/mp4"
    }

    func segmentData(renditionId: String, segment: VesperDashSegmentRequest) async throws -> Data {
        try await segmentPayload(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: "resourceLoader"
        ).readData()
    }

    func segmentResourcePayload(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayload {
        try await segmentPayload(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: "resourceLoader"
        )
    }

    private func segmentPayload(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String = "playback"
    ) async throws -> VesperDashSegmentPayload {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent(
            segmentBenchmarkEventName(segment, suffix: "start"),
            attributes: segmentBenchmarkStartAttributes(
                renditionId: renditionId,
                segment: segment,
                requestOrigin: requestOrigin
            )
        )
        do {
            let result = try await resolveSegmentPayload(renditionId: renditionId, segment: segment)
            await recordBenchmarkEvent(
                segmentBenchmarkEventName(segment, suffix: "end"),
                attributes: segmentBenchmarkEndAttributes(
                    startedAt: startedAt,
                    renditionId: renditionId,
                    segment: segment,
                    requestOrigin: requestOrigin,
                    result: result
                )
            )
            return result.payload
        } catch {
            await recordBenchmarkEvent(
                segmentBenchmarkEventName(segment, suffix: "end"),
                attributes: segmentBenchmarkEndAttributes(
                    startedAt: startedAt,
                    renditionId: renditionId,
                    segment: segment,
                    requestOrigin: requestOrigin,
                    error: error
                )
            )
            throw error
        }
    }

    private func resolveSegmentPayload(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayloadResult {
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
            if manifest.type == .dynamic {
                throw VesperDashBridgeError.unsupportedManifest(
                    "dynamic DASH SegmentBase is not supported on iOS"
                )
            }
            guard let mediaURL = URL(string: playable.representation.baseURL) else {
                throw VesperDashBridgeError.invalidManifest(
                    "invalid media URL \(playable.representation.baseURL)"
                )
            }

            let byteRange: VesperDashByteRange
            switch segment {
            case .initialization:
                byteRange = segmentBase.initialization
            case let .media(index):
                let segments = try await mediaSegments(for: playable, segmentBase: segmentBase)
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(renditionId)"
                    )
                }
                byteRange = segments[index].range
            }

            if mediaURL.isFileURL {
                let payload = VesperDashSegmentPayload.file(
                    url: mediaURL,
                    offset: byteRange.start,
                    size: byteRange.length,
                    removeAfterServing: false,
                    contentType: dashSegmentContentType(for: playable, segment: segment)
                )
                return VesperDashSegmentPayloadResult(
                    payload: payload,
                    cacheHit: false,
                    segmentType: "base",
                    byteRange: byteRange,
                    delivery: "localFile"
                )
            }
            let data = try await networkClient.data(for: mediaURL, byteRange: byteRange)
            return VesperDashSegmentPayloadResult(
                payload: .data(
                    data,
                    contentType: dashSegmentContentType(for: playable, segment: segment)
                ),
                cacheHit: false,
                segmentType: "base",
                byteRange: byteRange,
                delivery: "networkData"
            )
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        return try await cachedSegmentTemplatePayload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
    }

    private func cachedSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayloadResult {
        let contentType = dashSegmentContentType(for: playable, segment: segment)
        let key = VesperDashSegmentCacheKey(
            renditionId: playable.renditionId,
            segment: segment
        )
        let cacheURL = segmentCacheURL(
            renditionId: playable.renditionId,
            segment: segment
        )
        if let cached = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType) {
            return VesperDashSegmentPayloadResult(
                payload: cached,
                cacheHit: true,
                segmentType: "template",
                byteRange: nil,
                delivery: "cacheFile"
            )
        }

        if shouldCoalesceSegmentTemplateDownload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment,
            allowSkippingLargeMediaEntry: true
        ) {
            return try await coalescedSegmentTemplatePayload(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: true,
                contentType: contentType
            )
        }

        return try await fetchSegmentTemplatePayload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment,
            cacheURL: cacheURL,
            key: key,
            allowSkippingLargeMediaEntry: true,
            contentType: contentType
        )
    }

    private func coalescedSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayloadResult {
        if let inFlightTask = segmentDownloadTasksByKey[key] {
            let result = try await inFlightTask.value
            if let cached = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType) {
                return result.markingCoalesced(
                    payload: cached,
                    cacheHit: true,
                    delivery: "coalescedCacheFile"
                )
            }
            guard !result.payload.isTemporaryFile else {
                return try await fetchSegmentTemplatePayload(
                    manifest: manifest,
                    playable: playable,
                    segmentTemplate: segmentTemplate,
                    segment: segment,
                    cacheURL: cacheURL,
                    key: key,
                    allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
                    contentType: contentType
                )
            }
            return result.markingCoalesced()
        }

        let downloadTask = Task {
            try await self.fetchSegmentTemplatePayload(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
                contentType: contentType
            )
        }
        segmentDownloadTasksByKey[key] = downloadTask
        do {
            let result = try await downloadTask.value
            segmentDownloadTasksByKey[key] = nil
            return result
        } catch {
            segmentDownloadTasksByKey[key] = nil
            throw error
        }
    }

    private func fetchSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayloadResult {
        if case .media = segment {
            let payload = try await fetchSegmentTemplateFile(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
                contentType: contentType
            )
            return VesperDashSegmentPayloadResult(
                payload: payload,
                cacheHit: false,
                segmentType: "template",
                byteRange: nil,
                delivery: payload.isTemporaryFile ? "temporaryFile" : "cacheFile"
            )
        }

        let data = try await fetchSegmentTemplateData(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        try Task.checkCancellation()
        if try writeSegmentTemplateCache(
            data,
            to: cacheURL,
            key: key,
            allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry
        ) {
            let payload = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType)
                ?? .data(data, contentType: contentType)
            return VesperDashSegmentPayloadResult(
                payload: payload,
                cacheHit: false,
                segmentType: "template",
                byteRange: nil,
                delivery: "cacheFile"
            )
        }
        return VesperDashSegmentPayloadResult(
            payload: .data(data, contentType: contentType),
            cacheHit: false,
            segmentType: "template",
            byteRange: nil,
            delivery: "networkData"
        )
    }

    private func shouldCoalesceSegmentTemplateDownload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        allowSkippingLargeMediaEntry: Bool
    ) -> Bool {
        guard allowSkippingLargeMediaEntry else {
            return true
        }
        guard case let .media(index) = segment else {
            return true
        }
        guard
            let bandwidth = playable.representation.bandwidth,
            bandwidth > 0,
            let templateSegment = try? templateSegmentForRequest(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                index: index
            )
        else {
            return false
        }
        let estimatedBytes = templateSegment.duration * Double(bandwidth) / 8
        return estimatedBytes.isFinite
            && estimatedBytes <= Double(Self.segmentCacheMaxSingleMediaBytes)
    }

    private func templateSegmentForRequest(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        index: Int
    ) throws -> VesperDashTemplateSegment {
        let segments = try templateSegments(
            for: playable,
            manifest: manifest,
            segmentTemplate: segmentTemplate
        )
        if manifest.type == .dynamic {
            guard let matched = segments.first(where: { $0.number == UInt64(index) }) else {
                throw VesperDashBridgeError.invalidManifest(
                    "missing media segment number \(index) for rendition \(playable.renditionId)"
                )
            }
            return matched
        }
        guard segments.indices.contains(index) else {
            throw VesperDashBridgeError.invalidManifest(
                "missing media segment \(index) for rendition \(playable.renditionId)"
            )
        }
        return segments[index]
    }

    private func fetchSegmentTemplateFile(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayload {
        let url = try templateSegmentURL(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        let temporaryURL = temporarySegmentDownloadURL(renditionId: playable.renditionId, segment: segment)
        let size = try await networkClient.download(for: url, to: temporaryURL)
#if DEBUG
        logTopLevelBoxes(
            fileURL: temporaryURL,
            totalBytes: size,
            label: "dashSegmentTemplate",
            renditionId: playable.renditionId,
            segment: segment
        )
#endif
        return try materializeSegmentTemplateFile(
            from: temporaryURL,
            to: cacheURL,
            size: size,
            key: key,
            allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
            contentType: contentType
        )
    }

    private func materializeSegmentTemplateFile(
        from temporaryURL: URL,
        to cacheURL: URL,
        size: UInt64,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) throws -> VesperDashSegmentPayload {
        if allowSkippingLargeMediaEntry,
           case .media = key.segment,
           size > Self.segmentCacheMaxSingleMediaBytes {
#if DEBUG
            iosHostLog(
                "dashSegmentCache streamLarge rendition=\(key.renditionId) segment=\(key.segment) bytes=\(size)"
            )
#endif
            return .file(
                url: temporaryURL,
                offset: 0,
                size: size,
                removeAfterServing: true,
                contentType: contentType
            )
        }

        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        let addsEntry = cachedSegmentFiles[key] == nil
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
        }
        try trimSegmentCache(reserving: size, addingEntry: addsEntry, protecting: key)
        removeFileIfPresent(cacheURL, context: "existing DASH segment cache file")
        try FileManager.default.moveItem(at: temporaryURL, to: cacheURL)
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: cacheURL,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingAdd(size)
        try trimSegmentCache(reserving: 0, addingEntry: false, protecting: key)
        return .file(
            url: cacheURL,
            offset: 0,
            size: size,
            removeAfterServing: false,
            contentType: contentType
        )
    }

    private func temporarySegmentDownloadURL(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = "init"
        case let .media(index):
            segmentName = "\(index)"
        }
        return segmentCacheDirectory
            .appendingPathComponent("tmp-\(encodedId)-\(segmentName)-\(UUID().uuidString)", isDirectory: false)
    }

    private func fetchSegmentTemplateData(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> Data {
        let url = try templateSegmentURL(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        let data = try await networkClient.data(for: url)
        // Preserve the original fMP4 segment bytes. This used to strip
        // top-level sidx boxes from media segments, but many DASH encoders
        // write tfhd.base_data_offset as an absolute offset from the segment
        // start. Removing sidx shifts mdat forward, causing AVPlayer to read
        // garbage bytes and report CoreMediaErrorDomain 1718449215 ('frmt').
        // HLS fMP4 allows sidx to remain in segments, and AVPlayer ignores it.
#if DEBUG
        logTopLevelBoxes(
            data: data,
            label: "dashSegmentTemplate",
            renditionId: playable.renditionId,
            segment: segment
        )
#endif
        return data
    }

#if DEBUG
    private func logTopLevelBoxes(
        data: Data,
        label: String,
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) {
        let bytes = [UInt8](data.prefix(4_096))
        var cursor = 0
        var types: [String] = []
        while cursor < bytes.count, types.count < 8 {
            guard let header = try? VesperMp4BoxHeader.parse(bytes: bytes, start: cursor) else { break }
            let typeString = String(bytes: header.boxType, encoding: .ascii) ?? "????"
            types.append(typeString)
            if header.end <= cursor { break }
            cursor = header.end
        }
        iosHostLog(
            "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(data.count) topBoxes=\(types.joined(separator: ","))"
        )
    }

    private func logTopLevelBoxes(
        fileURL: URL,
        totalBytes: UInt64,
        label: String,
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) {
        guard
            let handle = try? FileHandle(forReadingFrom: fileURL),
            let data = try? handle.read(upToCount: 4_096)
        else {
            iosHostLog(
                "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(totalBytes) topBoxes=<unreadable>"
            )
            return
        }
        closeFileHandle(handle, context: "\(label) MP4 box inspection")
        let bytes = [UInt8](data)
        var cursor = 0
        var types: [String] = []
        while cursor < bytes.count, types.count < 8 {
            guard let header = try? VesperMp4BoxHeader.parse(bytes: bytes, start: cursor) else { break }
            let typeString = String(bytes: header.boxType, encoding: .ascii) ?? "????"
            types.append(typeString)
            if header.end <= cursor { break }
            cursor = header.end
        }
        iosHostLog(
            "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(totalBytes) topBoxes=\(types.joined(separator: ","))"
        )
    }
#endif

    private func cachedSegmentFilePayload(
        for key: VesperDashSegmentCacheKey,
        at url: URL,
        contentType: String
    ) -> VesperDashSegmentPayload? {
        guard FileManager.default.fileExists(atPath: url.path) else {
            if let existing = cachedSegmentFiles.removeValue(forKey: key) {
                segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
            }
            return nil
        }
        let size = fileSize(at: url) ?? cachedSegmentFiles[key]?.size ?? 0
        touchCachedSegmentFile(key: key, url: url, size: size)
        return .file(
            url: url,
            offset: 0,
            size: size,
            removeAfterServing: false,
            contentType: contentType
        )
    }

    private func cachedSegmentFileExists(
        for key: VesperDashSegmentCacheKey,
        at url: URL
    ) -> Bool {
        guard FileManager.default.fileExists(atPath: url.path) else {
            if let existing = cachedSegmentFiles.removeValue(forKey: key) {
                segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
            }
            return false
        }
        let size = fileSize(at: url) ?? cachedSegmentFiles[key]?.size ?? 0
        touchCachedSegmentFile(key: key, url: url, size: size)
        return true
    }

    @discardableResult
    private func writeSegmentTemplateCache(
        _ data: Data,
        to url: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool
    ) throws -> Bool {
        let size = UInt64(data.count)
        if allowSkippingLargeMediaEntry,
           case .media = key.segment,
           size > Self.segmentCacheMaxSingleMediaBytes {
#if DEBUG
            iosHostLog(
                "dashSegmentCache skipLarge rendition=\(key.renditionId) segment=\(key.segment) bytes=\(size)"
            )
#endif
            return false
        }

        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        let addsEntry = cachedSegmentFiles[key] == nil
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
        }
        try trimSegmentCache(reserving: size, addingEntry: addsEntry, protecting: key)
        try data.write(to: url, options: .atomic)
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: url,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingAdd(size)
        try trimSegmentCache(reserving: 0, addingEntry: false, protecting: key)
        return true
    }

    private func touchCachedSegmentFile(
        key: VesperDashSegmentCacheKey,
        url: URL,
        size: UInt64
    ) {
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes
                .saturatingSubtract(existing.size)
                .saturatingAdd(size)
            cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
                url: url,
                size: size,
                segment: key.segment,
                lastAccessedAt: Date()
            )
            return
        }
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: url,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingAdd(size)
    }

    private func fileSize(at url: URL) -> UInt64? {
        guard
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let value = attributes[.size] as? NSNumber
        else {
            return nil
        }
        return value.uint64Value
    }

    private func trimSegmentCache(
        reserving additionalBytes: UInt64,
        addingEntry: Bool,
        protecting protectedKey: VesperDashSegmentCacheKey
    ) throws {
        var projectedBytes = segmentCacheTotalBytes.saturatingAdd(additionalBytes)
        while
            cachedSegmentFiles.count + (addingEntry ? 1 : 0) > Self.segmentCacheMaxEntryCount ||
            projectedBytes > Self.segmentCacheMaxBytes
        {
            guard let eviction = nextSegmentCacheEviction(protecting: protectedKey) else {
                return
            }
            cachedSegmentFiles[eviction.key] = nil
            segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(eviction.file.size)
            projectedBytes = projectedBytes.saturatingSubtract(eviction.file.size)
            removeFileIfPresent(eviction.file.url, context: "evicted DASH segment cache file")
#if DEBUG
            iosHostLog(
                "dashSegmentCache evict rendition=\(eviction.key.renditionId) segment=\(eviction.key.segment) bytes=\(eviction.file.size)"
            )
#endif
        }
    }

    private func nextSegmentCacheEviction(
        protecting protectedKey: VesperDashSegmentCacheKey
    ) -> (key: VesperDashSegmentCacheKey, file: VesperDashCachedSegmentFile)? {
        let candidate = cachedSegmentFiles
            .filter { key, _ in key != protectedKey }
            .min { lhs, rhs in
                let lhsInit = lhs.value.isInitialization
                let rhsInit = rhs.value.isInitialization
                if lhsInit != rhsInit {
                    return !lhsInit
                }
                return lhs.value.lastAccessedAt < rhs.value.lastAccessedAt
            }
        return candidate.map { (key: $0.key, file: $0.value) }
    }

    private func startBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int,
        prefetchMediaSegments: Bool
    ) {
        guard !sourceURL.isFileURL,
              segmentCount > 0,
              !backgroundPrefetchRenditionIds.contains(renditionId)
        else {
            return
        }
        backgroundPrefetchRenditionIds.insert(renditionId)
        let shouldPrefetchMediaSegments = prefetchMediaSegments
            && !backgroundPrefetchLargeMediaRenditionIds.contains(renditionId)
        Task(priority: .utility) { [weak self] in
            await self?.runBackgroundSegmentPrefetch(
                renditionId: renditionId,
                segmentCount: segmentCount,
                prefetchMediaSegments: shouldPrefetchMediaSegments
            )
        }
    }

    private func startStartupPrefetch(
        for playables: [VesperDashPlayableRepresentation],
        manifest: VesperDashManifest
    ) {
        guard !playables.isEmpty else {
            return
        }
        startBackgroundPrefetch(for: playables, manifest: manifest)
        let renditionIds = playables.map(\.renditionId)
        Task(priority: .userInitiated) { [weak self] in
            await self?.runStartupMediaPlaylistPrefetch(renditionIds: renditionIds)
        }
    }

    private func runStartupMediaPlaylistPrefetch(renditionIds: [String]) async {
        await recordBenchmarkEvent(
            "dash_startup_prefetch_start",
            attributes: ["renditionIds": renditionIds.joined(separator: ",")]
        )
        let succeeded = await withTaskGroup(of: Bool.self, returning: Int.self) { group in
            for renditionId in renditionIds {
                group.addTask { [weak self] in
                    guard let self else {
                        return false
                    }
                    do {
                        _ = try await self.mediaPlaylistData(renditionId: renditionId)
                        return true
                    } catch {
                        iosHostLog(
                            "dashStartupPrefetch failed rendition=\(renditionId) error=\(error.localizedDescription)"
                        )
                        return false
                    }
                }
            }

            var count = 0
            for await ok in group where ok {
                count += 1
            }
            return count
        }
        await recordBenchmarkEvent(
            "dash_startup_prefetch_end",
            attributes: [
                "requested": "\(renditionIds.count)",
                "succeeded": "\(succeeded)",
            ]
        )
    }

    private func startBackgroundPrefetch(
        for playables: [VesperDashPlayableRepresentation],
        manifest: VesperDashManifest
    ) {
        for playable in playables {
            guard let segmentTemplate = playable.representation.segmentTemplate,
                  let segments = try? templateSegments(
                    for: playable,
                    manifest: manifest,
                    segmentTemplate: segmentTemplate
                  )
            else {
                continue
            }
            startBackgroundSegmentPrefetch(
                renditionId: playable.renditionId,
                segmentCount: segments.count,
                prefetchMediaSegments: shouldPrefetchTemplateMediaSegments(
                    playable: playable,
                    segments: segments
                )
            )
        }
    }

    private func shouldPrefetchTemplateMediaSegments(
        playable: VesperDashPlayableRepresentation,
        segments: [VesperDashTemplateSegment]
    ) -> Bool {
        guard let bandwidth = playable.representation.bandwidth, bandwidth > 0 else {
            return true
        }
        let maxDuration = segments.map(\.duration).max() ?? 0
        guard maxDuration.isFinite, maxDuration > 0 else {
            return true
        }
        let estimatedBytes = maxDuration * Double(bandwidth) / 8
        guard estimatedBytes.isFinite else {
            return false
        }
        let shouldPrefetch = estimatedBytes <= Double(Self.segmentCacheMaxSingleMediaBytes)
#if DEBUG
        if !shouldPrefetch {
            iosHostLog(
                "dashSegmentPrefetch skipMedia rendition=\(playable.renditionId) estimatedBytes=\(String(format: "%.0f", estimatedBytes)) limit=\(Self.segmentCacheMaxSingleMediaBytes)"
            )
        }
#endif
        return shouldPrefetch
    }

    private func startupRenditionSummary(
        _ playables: [VesperDashPlayableRepresentation]
    ) -> String {
        guard !playables.isEmpty else {
            return "none"
        }
        return playables
            .map(startupRenditionDescription)
            .joined(separator: ";")
    }

    private func startupRenditionDescription(
        _ playable: VesperDashPlayableRepresentation
    ) -> String {
        let representation = playable.representation
        let capability = videoDecodeCapabilitiesCache?.first { $0.renditionId == playable.renditionId }
        return [
            "id=\(playable.renditionId)",
            "codec=\(emptyAsNil(representation.codecs))",
            "codecFamily=\(capability?.codecFamily.rawValue ?? "unknown")",
            "hardwareDecodeSupported=\(capability.map { "\($0.hardwareDecodeSupported)" } ?? "unknown")",
            "width=\(representation.width.map(String.init) ?? "nil")",
            "height=\(representation.height.map(String.init) ?? "nil")",
            "bitrate=\(representation.bandwidth.map(String.init) ?? "nil")",
            "frameRate=\(representation.frameRate ?? "nil")",
            "segmentType=\(dashSegmentTypeName(representation))",
        ].joined(separator: ",")
    }

    private func masterPlaylistDecodeSelectionAttributes(
        startupSelected: VesperDashSelectedPlayableResponse
    ) -> [String: String] {
        guard let startupVideo = startupSelected.video.first else {
            return [:]
        }
        var attributes: [String: String] = [
            "startupVideoRenditionId": startupVideo.renditionId,
            "startupVideoCodec": startupVideo.representation.codecs,
            "selectionReason": "hardware_decode_startup",
        ]
        if let capability = videoDecodeCapabilitiesCache?.first(where: {
            $0.renditionId == startupVideo.renditionId
        }) {
            attributes["codecFamily"] = capability.codecFamily.rawValue
            attributes["hardwareDecodeSupported"] = "\(capability.hardwareDecodeSupported)"
            if let decoderName = capability.decoderName {
                attributes["decoderName"] = decoderName
            }
        }
        return attributes
    }

    private func recordBenchmarkEvent(
        _ eventName: String,
        attributes: [String: String] = [:]
    ) async {
        guard let benchmarkEventRecorder else {
            return
        }
        await benchmarkEventRecorder(eventName, attributes)
    }

    private func playlistBenchmarkEndAttributes(
        startedAt: UInt64,
        bytes: Int?,
        cacheHit: Bool,
        error: Error? = nil,
        extra: [String: String] = [:]
    ) -> [String: String] {
        var attributes = extra
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["cacheHit"] = "\(cacheHit)"
        if let bytes {
            attributes["bytes"] = "\(bytes)"
        }
        if let error {
            attributes["error"] = error.localizedDescription
        }
        return attributes
    }

    private func segmentBenchmarkEventName(
        _ segment: VesperDashSegmentRequest,
        suffix: String
    ) -> String {
        switch segment {
        case .initialization:
            return "dash_init_segment_request_\(suffix)"
        case .media:
            return "dash_media_segment_request_\(suffix)"
        }
    }

    private func segmentBenchmarkStartAttributes(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String
    ) -> [String: String] {
        segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
    }

    private func segmentBenchmarkEndAttributes(
        startedAt: UInt64,
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String,
        result: VesperDashSegmentPayloadResult
    ) -> [String: String] {
        var attributes = segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["bytes"] = "\(result.payload.size)"
        attributes["cacheHit"] = "\(result.cacheHit)"
        attributes["coalesced"] = "\(result.coalesced)"
        attributes["segmentType"] = result.segmentType
        attributes["delivery"] = result.delivery
        attributes["contentType"] = result.payload.contentType
        if let byteRange = result.byteRange {
            attributes["byteRange"] = "\(byteRange.start)-\(byteRange.end)"
        }
        return attributes
    }

    private func segmentBenchmarkEndAttributes(
        startedAt: UInt64,
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String,
        error: Error
    ) -> [String: String] {
        var attributes = segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["error"] = error.localizedDescription
        return attributes
    }

    private func segmentBenchmarkBaseAttributes(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String
    ) -> [String: String] {
        var attributes = [
            "renditionId": renditionId,
            "segmentKind": dashSegmentKindName(segment),
            "requestOrigin": requestOrigin,
        ]
        if case let .media(index) = segment {
            attributes["index"] = "\(index)"
        }
        return attributes
    }

    private func elapsedMillisecondsString(since startedAt: UInt64) -> String {
        let now = DispatchTime.now().uptimeNanoseconds
        let elapsedNs = now >= startedAt ? now - startedAt : 0
        return "\(elapsedNs / 1_000_000)"
    }

    private func runBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int,
        prefetchMediaSegments: Bool
    ) async {
        let prefetchLimit = prefetchMediaSegments
            ? min(segmentCount, Self.startupMediaSegmentPrefetchLimit)
            : 0
        let requests = backgroundPrefetchRequests(
            count: prefetchLimit,
            includeMediaSegments: prefetchMediaSegments
        )
        let concurrency = min(4, requests.count)
        guard concurrency > 0 else { return }

        await withTaskGroup(of: Bool.self) { group in
            var nextIndex = 0
            var shouldStopMediaPrefetch = false
            for _ in 0..<concurrency {
                let request = requests[nextIndex]
                nextIndex += 1
                group.addTask { [weak self] in
                    await self?.prefetchIgnoringFailure(
                        renditionId: renditionId,
                        segment: request
                    ) ?? false
                }
            }

            while let shouldContinue = await group.next() {
                if !shouldContinue {
                    shouldStopMediaPrefetch = true
                }
                guard !shouldStopMediaPrefetch, nextIndex < requests.count else {
                    continue
                }
                let request = requests[nextIndex]
                nextIndex += 1
                group.addTask { [weak self] in
                    await self?.prefetchIgnoringFailure(
                        renditionId: renditionId,
                        segment: request
                    ) ?? false
                }
            }
        }
#if DEBUG
        iosHostLog(
            "dashSegmentPrefetch completed rendition=\(renditionId) mediaPrefetch=\(prefetchMediaSegments) count=\(requests.count)"
        )
#endif
    }

    private func prefetchIgnoringFailure(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async -> Bool {
        do {
            let payload = try await segmentPayload(
                renditionId: renditionId,
                segment: segment,
                requestOrigin: "prefetch"
            )
            let shouldContinue = !(segment.isMedia && payload.isTemporaryFile)
            if !shouldContinue {
                backgroundPrefetchLargeMediaRenditionIds.insert(renditionId)
#if DEBUG
                iosHostLog(
                    "dashSegmentPrefetch stopLargeMedia rendition=\(renditionId) segment=\(segment) bytes=\(payload.size)"
                )
#endif
            }
            payload.cleanupIfTemporary()
            return shouldContinue
        } catch {
            iosHostLog(
                "dashSegmentPrefetch failed rendition=\(renditionId) segment=\(segment) error=\(error.localizedDescription)"
            )
            return true
        }
    }

    func segmentRedirectURL(renditionId: String, segment: VesperDashSegmentRequest) async throws -> URL {
        let key = VesperDashSegmentCacheKey(renditionId: renditionId, segment: segment)
        let url = segmentCacheURL(renditionId: renditionId, segment: segment)
        if cachedSegmentFileExists(for: key, at: url) {
            return url
        }

        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentTemplate = playable.representation.segmentTemplate {
            let payloadResult = try await coalescedSegmentTemplatePayload(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: url,
                key: key,
                allowSkippingLargeMediaEntry: false,
                contentType: dashSegmentContentType(for: playable, segment: segment)
            )
            guard case let .file(fileURL, 0, _, false, _) = payloadResult.payload else {
                throw VesperDashBridgeError.network("DASH segment redirect requires a persistent local file")
            }
            return fileURL
        }

        let data = try await segmentData(renditionId: renditionId, segment: segment)
        _ = try writeSegmentTemplateCache(
            data,
            to: url,
            key: key,
            allowSkippingLargeMediaEntry: false
        )
        return url
    }

    private func segmentCacheURL(renditionId: String, segment: VesperDashSegmentRequest) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let fileName: String
        switch segment {
        case .initialization:
            fileName = "\(encodedId)-init.mp4"
        case let .media(index):
            fileName = "\(encodedId)-\(index).m4s"
        }
        return segmentCacheDirectory.appendingPathComponent(fileName, isDirectory: false)
    }

    private func templateSegmentURL(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) throws -> URL {
        let template: String
        let number: UInt64?
        let time: UInt64?
        switch segment {
        case .initialization:
            guard let initialization = segmentTemplate.initialization else {
                throw VesperDashBridgeError.unsupportedManifest(
                    "Representation \(playable.representation.id) does not provide SegmentTemplate initialization"
                )
            }
            template = initialization
            number = nil
            time = nil
        case let .media(index):
            let segments = try templateSegments(
                for: playable,
                manifest: manifest,
                segmentTemplate: segmentTemplate
            )
            let selectedSegment: VesperDashTemplateSegment
            if manifest.type == .dynamic {
                guard let matched = segments.first(where: { $0.number == UInt64(index) }) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment number \(index) for rendition \(playable.renditionId)"
                    )
                }
                selectedSegment = matched
            } else {
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(playable.renditionId)"
                    )
                }
                selectedSegment = segments[index]
            }
            template = segmentTemplate.media
            number = selectedSegment.number
            time = selectedSegment.time
        }

        return try expandedTemplateURL(
            playable: playable,
            template: template,
            number: number,
            time: time
        )
    }

    private func expandedTemplateURL(
        playable: VesperDashPlayableRepresentation,
        template: String,
        number: UInt64?,
        time: UInt64?
    ) throws -> URL {
        let expanded = try VesperDashTemplateExpander.expand(
            template,
            representation: playable.representation,
            number: number,
            time: time
        )
        let resolved = resolveDashURI(base: playable.representation.baseURL, reference: expanded)
        guard let url = URL(string: resolved) else {
            throw VesperDashBridgeError.invalidManifest("invalid segment URL \(resolved)")
        }
        return url
    }

    private func selectedPlayableRepresentations(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy
    ) throws -> VesperDashSelectedPlayableResponse {
        if let cached = selectedPlayableByPolicy[variantPolicy] {
            return cached
        }
        let videoDecodeCapabilities = try videoDecodeCapabilities(for: manifest)
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: variantPolicy,
            videoDecodeCapabilities: videoDecodeCapabilities
        )
        let response = VesperDashSelectedPlayableResponse(
            audio: selected.audio,
            video: selected.video,
            subtitles: selected.subtitles
        )
        selectedPlayableByPolicy[variantPolicy] = response
        if variantPolicy == .all {
            playableByRenditionId = Dictionary(
                uniqueKeysWithValues: (response.audio + response.video + response.subtitles).map {
                    ($0.renditionId, $0)
                }
            )
        }
        return response
    }

    private func videoDecodeCapabilities(
        for manifest: VesperDashManifest
    ) throws -> [VesperDashVideoDecodeCapability] {
        if let cached = videoDecodeCapabilitiesCache {
            return cached
        }
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all,
            videoDecodeCapabilities: nil
        )
        let capabilities = selected.video.map(videoDecodeCapability)
        videoDecodeCapabilitiesCache = capabilities
        return capabilities
    }

    private func videoDecodeCapability(
        for playable: VesperDashPlayableRepresentation
    ) -> VesperDashVideoDecodeCapability {
        videoDecodeCapabilityProvider(playable)
    }

    private nonisolated static func defaultVideoDecodeCapability(
        for playable: VesperDashPlayableRepresentation
    ) -> VesperDashVideoDecodeCapability {
        let candidate = VesperHardwareDecodeCandidateCodec(codecName: playable.representation.codecs)
        let hardwareDecodeSupported = VesperCodecSupport.hardwareDecodeSupported(
            for: playable.representation.codecs
        )
        return VesperDashVideoDecodeCapability(
            renditionId: playable.renditionId,
            codecFamily: candidate.dashCodecFamily,
            hardwareDecodeSupported: hardwareDecodeSupported,
            decoderName: hardwareDecodeSupported ? "VideoToolbox" : nil
        )
    }

    private func mediaSegments(
        for playable: VesperDashPlayableRepresentation,
        segmentBase: VesperDashSegmentBase
    ) async throws -> [VesperDashMediaSegment] {
        if let cached = mediaSegmentsByRenditionId[playable.renditionId] {
            return cached
        }
        let sidx = try await loadSidx(for: playable)
        let segments = try VesperDashHlsBuilder.mediaSegments(
            segmentBase: segmentBase,
            sidx: sidx
        )
        mediaSegmentsByRenditionId[playable.renditionId] = segments
        return segments
    }

    private func templateSegments(
        for playable: VesperDashPlayableRepresentation,
        manifest: VesperDashManifest,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> [VesperDashTemplateSegment] {
        if let cached = templateSegmentsByRenditionId[playable.renditionId] {
            return cached
        }
        let segments = try VesperDashHlsBuilder.templateSegments(
            manifestType: manifest.type,
            durationMs: manifest.durationMs,
            segmentTemplate: segmentTemplate
        )
        templateSegmentsByRenditionId[playable.renditionId] = segments
        return segments
    }

    private func loadManifest() async throws -> VesperDashManifest {
        if let manifest, !shouldRefreshManifest(manifest) {
            return manifest
        }
        if let manifestLoadTask {
            return try await manifestLoadTask.value
        }

        let task = Task { try await self.fetchManifestFromNetwork() }
        manifestLoadTask = task
        let parsed: VesperDashManifest
        do {
            parsed = try await task.value
            manifestLoadTask = nil
        } catch {
            manifestLoadTask = nil
            throw error
        }
        if manifest != nil, parsed.type == .dynamic {
            clearManifestDerivedCaches()
        }
        manifest = parsed
        manifestLoadedAt = Date()
        return parsed
    }

    private func fetchManifestFromNetwork() async throws -> VesperDashManifest {
        let data = try await networkClient.data(for: sourceURL)
        return try VesperDashManifestParser.parse(data: data, manifestURL: sourceURL)
    }

    private func shouldRefreshManifest(_ manifest: VesperDashManifest) -> Bool {
        guard manifest.type == .dynamic else {
            return false
        }
        guard let manifestLoadedAt else {
            return true
        }
        let refreshIntervalMs = max(
            manifest.minimumUpdatePeriodMs ?? Self.defaultDynamicManifestRefreshIntervalMs,
            Self.minimumDynamicManifestRefreshIntervalMs
        )
        return Date().timeIntervalSince(manifestLoadedAt) * 1_000 >= Double(refreshIntervalMs)
    }

    private func clearManifestDerivedCaches() {
        masterPlaylistCache = nil
        mediaPlaylistCacheByRenditionId = [:]
        selectedPlayableByPolicy = [:]
        playableByRenditionId = [:]
        videoDecodeCapabilitiesCache = nil
        sidxByRenditionId = [:]
        mediaSegmentsByRenditionId = [:]
        templateSegmentsByRenditionId = [:]
        mediaPlaylistTasksByRenditionId = [:]
        sidxLoadTasksByRenditionId = [:]
        segmentDownloadTasksByKey = [:]
    }

    private func playableRepresentation(renditionId: String) async throws -> VesperDashPlayableRepresentation {
        if let cached = playableByRenditionId[renditionId] {
            return cached
        }
        let manifest = try await loadManifest()
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )
        guard let playable = (selected.audio + selected.video + selected.subtitles).first(where: {
            $0.renditionId == renditionId
        }) else {
            throw VesperDashBridgeError.invalidManifest(
                "missing DASH representation for rendition \(renditionId)"
            )
        }
        return playable
    }

    private func loadSidx(for playable: VesperDashPlayableRepresentation) async throws -> VesperDashSidxBox {
        if let cached = sidxByRenditionId[playable.renditionId] {
            return cached
        }
        if let inFlightTask = sidxLoadTasksByRenditionId[playable.renditionId] {
            return try await inFlightTask.value
        }
        guard let segmentBase = playable.representation.segmentBase else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase"
            )
        }
        let task = Task {
            try await self.fetchSidx(
                playable: playable,
                segmentBase: segmentBase
            )
        }
        sidxLoadTasksByRenditionId[playable.renditionId] = task
        do {
            let sidx = try await task.value
            sidxLoadTasksByRenditionId[playable.renditionId] = nil
            sidxByRenditionId[playable.renditionId] = sidx
            return sidx
        } catch {
            sidxLoadTasksByRenditionId[playable.renditionId] = nil
            throw error
        }
    }

    private func fetchSidx(
        playable: VesperDashPlayableRepresentation,
        segmentBase: VesperDashSegmentBase
    ) async throws -> VesperDashSidxBox {
        guard let mediaURL = URL(string: playable.representation.baseURL) else {
            throw VesperDashBridgeError.invalidManifest(
                "invalid media URL \(playable.representation.baseURL)"
            )
        }
        let data = try await networkClient.data(for: mediaURL, byteRange: segmentBase.indexRange)
        return try VesperDashSidxParser.parse(data: data)
    }
}

class VesperDashNetworkClient {
    private let headers: [String: String]

    init(headers: [String: String] = [:]) {
        self.headers = headers
    }

    func data(for url: URL, byteRange: VesperDashByteRange? = nil) async throws -> Data {
        if url.isFileURL {
            return try readLocalFile(url: url, byteRange: byteRange)
        }
        try rejectInsecureHTTPURL(url)

        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        if let byteRange {
            request.setValue("bytes=\(byteRange.start)-\(byteRange.end)", forHTTPHeaderField: "Range")
        }
        let session = makeSession()
        defer { session.invalidateAndCancel() }
        let (data, response) = try await session.data(for: request)
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            throw VesperDashBridgeError.network("HTTP \(httpResponse.statusCode) for \(url.absoluteString)")
        }
        return data
    }

    func download(
        for url: URL,
        byteRange: VesperDashByteRange? = nil,
        to destinationURL: URL
    ) async throws -> UInt64 {
        if !url.isFileURL {
            try rejectInsecureHTTPURL(url)
        }
        try FileManager.default.createDirectory(
            at: destinationURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        removeFileIfPresent(destinationURL, context: "existing DASH download destination")

        if url.isFileURL {
            return try copyLocalFile(url: url, byteRange: byteRange, to: destinationURL)
        }

        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        if let byteRange {
            request.setValue("bytes=\(byteRange.start)-\(byteRange.end)", forHTTPHeaderField: "Range")
        }
        let session = makeSession()
        defer { session.invalidateAndCancel() }
        let (temporaryURL, response) = try await session.download(for: request)
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            removeFileIfPresent(temporaryURL, context: "failed DASH download temporary file")
            throw VesperDashBridgeError.network("HTTP \(httpResponse.statusCode) for \(url.absoluteString)")
        }
        try FileManager.default.moveItem(at: temporaryURL, to: destinationURL)
        return fileSize(at: destinationURL) ?? 0
    }

    private func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = true
        configuration.timeoutIntervalForRequest = vesperDashNetworkStallTimeoutSeconds
        configuration.timeoutIntervalForResource = vesperDashNetworkResourceTimeoutSeconds
        return URLSession(configuration: configuration)
    }

    private func rejectInsecureHTTPURL(_ url: URL) throws {
        guard url.scheme?.lowercased() == "http" else {
            return
        }
        throw VesperDashBridgeError.network("\(vesperDashATSFailureMessage) URL: \(url.absoluteString)")
    }

    private func readLocalFile(url: URL, byteRange: VesperDashByteRange?) throws -> Data {
        guard let byteRange else {
            return try Data(contentsOf: url)
        }

        let length = try checkedInt(byteRange.length, field: "local file byte range length")
        let handle = try FileHandle(forReadingFrom: url)
        defer { closeFileHandle(handle, context: "local byte range") }
        try handle.seek(toOffset: byteRange.start)
        let data = try handle.read(upToCount: length) ?? Data()
        guard data.count == length else {
            throw VesperDashBridgeError.network("local file byte range is shorter than requested")
        }
        return data
    }

    private func copyLocalFile(
        url: URL,
        byteRange: VesperDashByteRange?,
        to destinationURL: URL
    ) throws -> UInt64 {
        guard let byteRange else {
            try FileManager.default.copyItem(at: url, to: destinationURL)
            return fileSize(at: destinationURL) ?? 0
        }

        let input = try FileHandle(forReadingFrom: url)
        defer { closeFileHandle(input, context: "local copy input") }
        FileManager.default.createFile(atPath: destinationURL.path, contents: nil)
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { closeFileHandle(output, context: "local copy output") }

        try input.seek(toOffset: byteRange.start)
        var remaining = byteRange.length
        while remaining > 0 {
            let readCount = remaining > 256 * 1024 ? 256 * 1024 : Int(remaining)
            let data = try input.read(upToCount: readCount) ?? Data()
            guard !data.isEmpty else {
                throw VesperDashBridgeError.network("local file byte range is shorter than requested")
            }
            try output.write(contentsOf: data)
            remaining = remaining.saturatingSubtract(UInt64(data.count))
        }
        return byteRange.length
    }

    private func fileSize(at url: URL) -> UInt64? {
        guard
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let value = attributes[.size] as? NSNumber
        else {
            return nil
        }
        return value.uint64Value
    }
}

private struct VesperMp4BoxHeader {
    let boxType: [UInt8]
    let end: Int

    static func parse(bytes: [UInt8], start: Int) throws -> VesperMp4BoxHeader {
        let remaining = bytes.count - start
        guard remaining >= 8 else {
            throw VesperDashBridgeError.invalidMp4("truncated MP4 box header")
        }
        let size32 = try readBigEndianUInt32(bytes, offset: start, field: "MP4 box size")
        let boxType = Array(bytes[(start + 4)..<(start + 8)])
        let boxSize: Int
        let headerSize: Int
        if size32 == 0 {
            boxSize = remaining
            headerSize = 8
        } else if size32 == 1 {
            guard remaining >= 16 else {
                throw VesperDashBridgeError.invalidMp4("truncated extended MP4 box header")
            }
            let size64 = try readBigEndianUInt64(bytes, offset: start + 8, field: "extended MP4 box size")
            boxSize = try checkedInt(size64, field: "extended MP4 box size")
            headerSize = 16
        } else {
            boxSize = Int(size32)
            headerSize = 8
        }
        guard boxSize >= headerSize else {
            throw VesperDashBridgeError.invalidMp4("MP4 box size is smaller than its header")
        }
        guard boxSize <= remaining else {
            throw VesperDashBridgeError.invalidMp4("MP4 box exceeds input data")
        }
        return VesperMp4BoxHeader(
            boxType: boxType,
            end: start + boxSize
        )
    }
}

private func resolveDashURI(base: String, reference: String) -> String {
    let reference = reference.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !reference.isEmpty else { return base }
    if URL(string: reference)?.scheme != nil {
        return reference
    }
    guard let baseURL = URL(string: base),
          let resolved = URL(string: reference, relativeTo: baseURL)?.absoluteURL
    else {
        return reference
    }
    return resolved.absoluteString
}

private func dashSegmentTypeName(_ representation: VesperDashRepresentation) -> String {
    if representation.segmentTemplate != nil {
        return "template"
    }
    if representation.segmentBase != nil {
        return "base"
    }
    return "unknown"
}

private func emptyAsNil(_ value: String) -> String {
    value.isEmpty ? "nil" : value
}

private func dashSegmentKindName(_ segment: VesperDashSegmentRequest) -> String {
    switch segment {
    case .initialization:
        return "initialization"
    case .media:
        return "media"
    }
}

func applyHttpHeaders(_ headers: [String: String], to request: inout URLRequest) {
    for (field, value) in headers where !field.isEmpty {
        request.setValue(value, forHTTPHeaderField: field)
    }
    if request.value(forHTTPHeaderField: "Accept-Encoding") == nil {
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
    }
}

let vesperAVURLAssetHTTPHeaderFieldsKey = "AVURLAssetHTTPHeaderFieldsKey"

private let dashPathComponentAllowedCharacters: CharacterSet = {
    var characters = CharacterSet.urlPathAllowed
    characters.remove(charactersIn: "/")
    return characters
}()

private func checkedInt(_ value: UInt64, field: String) throws -> Int {
    guard value <= UInt64(Int.max) else {
        throw VesperDashBridgeError.invalidMp4("\(field) exceeds Int.max")
    }
    return Int(value)
}

private func closeFileHandle(_ handle: FileHandle, context: String) {
    do {
        try handle.close()
    } catch {
        iosHostLog("failed to close \(context) file handle: \(error.localizedDescription)")
    }
}

private func removeFileIfPresent(_ url: URL, context: String) {
    guard FileManager.default.fileExists(atPath: url.path) else {
        return
    }
    do {
        try FileManager.default.removeItem(at: url)
    } catch {
        iosHostLog("failed to remove \(context): \(error.localizedDescription)")
    }
}

private func startupPrefetchSegmentIndices(count: Int) -> [Int] {
    guard count > 0 else {
        return []
    }
    let candidates = [
        0,
        min(1, count - 1),
        min((count + 2) / 3, count - 1),
        min(((count * 2) + 2) / 3, count - 1),
    ]
    return Array(Set(candidates)).sorted()
}

private func backgroundPrefetchRequests(
    count: Int,
    includeMediaSegments: Bool = true
) -> [VesperDashSegmentRequest] {
    guard includeMediaSegments, count > 0 else {
        return [.initialization]
    }
    let prioritized = startupPrefetchSegmentIndices(count: count)
    let orderedIndices = prioritized + (0..<count).filter { !prioritized.contains($0) }
    return [.initialization] + orderedIndices.map(VesperDashSegmentRequest.media)
}

private func readBigEndianUInt32(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt32 {
    guard offset >= 0, offset + 4 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    return (UInt32(bytes[offset]) << 24)
        | (UInt32(bytes[offset + 1]) << 16)
        | (UInt32(bytes[offset + 2]) << 8)
        | UInt32(bytes[offset + 3])
}

private func readBigEndianUInt64(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt64 {
    guard offset >= 0, offset + 8 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    var value: UInt64 = 0
    for byte in bytes[offset..<(offset + 8)] {
        value = (value << 8) | UInt64(byte)
    }
    return value
}

private extension UInt64 {
    func saturatingAdd(_ rhs: UInt64) -> UInt64 {
        let (value, overflow) = addingReportingOverflow(rhs)
        return overflow ? UInt64.max : value
    }

    func saturatingSubtract(_ rhs: UInt64) -> UInt64 {
        let (value, overflow) = subtractingReportingOverflow(rhs)
        return overflow ? 0 : value
    }
}
