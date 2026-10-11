import Foundation
import XCTest
#if !VESPER_HEALTH_STANDALONE
@testable import VesperPlayerKit
#endif

final class VesperDashSupportedRouteRegressionTests: XCTestCase {
    func testAVPrefetchCacheDoesNotReplaceNativeRedirect() async throws {
        let client = HealthDashClient()
        let session = VesperDashSession(
            sourceURL: client.manifestURL,
            networkClient: client,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider
        )
        let manifest = try await session.loadManifest()
        let selected = try await session.selectedPlayableRepresentations(manifest: manifest, variantPolicy: .all)
        for playable in selected.video + selected.audio {
            await session.runBackgroundSegmentPrefetch(renditionId: playable.renditionId, segmentCount: 3, prefetchMediaSegments: true)
            let before = client.mediaRequestCount
            let cached = try await session.resolveSegmentPayload(renditionId: playable.renditionId, segment: .media(0))
            try healthRequire(cached.cacheHit, "bounded prefetch must populate private cache")
            let request = try await session.segmentRedirectRequest(renditionId: playable.renditionId, segment: .media(0))
            try healthRequire(request.url?.scheme == "https", "A/V must retain native HTTPS delivery")
            try healthRequire(request.url?.lastPathComponent == "\(playable.representation.id)-1.m4s", "native redirect must target origin, not private cache")
            try healthRequire(request.value(forHTTPHeaderField: "Authorization") == "Bearer fixture", "redirect must retain source headers")
            try healthRequire(client.mediaRequestCount == before, "forming redirect must not initiate another SDK fetch")
            try healthRequire(before == (playable.adaptationSet.kind == .video ? 3 : 6), "each rendition prefetch is bounded to init plus two media requests")
        }
    }

    func testDynamicTemplateRefreshWhileOldPrefetchCompletesPreservesNativeRoute() async throws {
        let client = HealthDashClient(dynamic: true, gateMedia: true)
        defer { client.releaseMedia() }
        let session = VesperDashSession(
            sourceURL: client.manifestURL,
            networkClient: client,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider
        )
        let first = try await session.loadManifest()
        let selected = try await session.selectedPlayableRepresentations(manifest: first, variantPolicy: .all)
        let rendition = try healthUnwrap(selected.video.first?.renditionId)
        let old = Task { try await session.resolveSegmentPayload(renditionId: rendition, segment: .media(101)) }
        try await healthWait { client.mediaRequestCount == 1 }
        client.advanceManifest()
        await session.healthExpireManifest()
        let refreshed = try await session.loadManifest()
        try healthRequire(refreshed.type == .dynamic, "probe must exercise supported dynamic template")
        let beforeCompletion = try await session.segmentRedirectRequest(renditionId: rendition, segment: .media(102))
        client.releaseMedia()
        _ = try await old.value
        let afterCompletion = try await session.segmentRedirectRequest(renditionId: rendition, segment: .media(102))
        try healthRequire(beforeCompletion.url?.lastPathComponent == "video-202000.m4s", "refresh must resolve the new live timeline")
        try healthRequire(afterCompletion.url == beforeCompletion.url, "old worker completion must not change current native redirect")
        try healthRequire(client.manifestRequestCount == 2, "refresh must coalesce into exactly one new manifest request")
        try healthRequire(client.mediaRequestCount == 1, "native redirect must bypass old private media result")
    }

    func testDirectNetworkRequestCancellationStopsURLSessionTask() async throws {
        HealthStalledProtocol.reset()
        let client = HealthCancellationClient()
        let task = Task { try await client.data(for: URL(string: "https://cancel.test/media")!) }
        defer { task.cancel() }
        try await healthWait { HealthStalledProtocol.started }
        task.cancel()
        do {
            _ = try await task.value
            throw HealthProbeFailure(message: "cancelled URLSession request unexpectedly succeeded")
        } catch let error as URLError {
            try healthRequire(error.code == .cancelled, "URLSession must report cancellation, not a media error")
        } catch is CancellationError {
            // Both Foundation and Swift cancellation representations are valid.
        }
        try await healthWait { HealthStalledProtocol.stopped }
    }

    func testFinitePrefetchReleasesSessionAndRemovesPrivateCacheAfterOwnerRelease() async throws {
        let client = HealthDashClient(gateMedia: true)
        defer { client.releaseMedia() }
        var session: VesperDashSession? = VesperDashSession(
            sourceURL: client.manifestURL,
            networkClient: client,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider
        )
        weak var released = session
        let directory = try healthUnwrap(session?.segmentCacheDirectory)
        let manifest = try await session!.loadManifest()
        let selected = try await session!.selectedPlayableRepresentations(manifest: manifest, variantPolicy: .all)
        let rendition = try healthUnwrap(selected.video.first?.renditionId)
        await session!.startBackgroundSegmentPrefetch(renditionId: rendition, segmentCount: 30, prefetchMediaSegments: true)
        try await healthWait { client.mediaRequestCount == 3 }
        session!.closeStartupResources() // Startup-only close is not full session shutdown.
        session = nil
        try healthRequire(released != nil, "in-flight finite work may temporarily own the session")
        client.releaseMedia()
        // Weak references can clear before deinit finishes its file cleanup.
        try await healthWait {
            released == nil && !FileManager.default.fileExists(atPath: directory.path)
        }
        try healthRequire(!FileManager.default.fileExists(atPath: directory.path), "deinit must remove private session cache")
        try healthRequire(client.mediaRequestCount == 3, "teardown probe must not grow beyond bounded prefetch")
    }
}

private struct HealthProbeFailure: Error { let message: String }
private func healthRequire(_ condition: Bool, _ message: String) throws {
    if !condition { throw HealthProbeFailure(message: message) }
}
private func healthUnwrap<T>(_ value: T?) throws -> T {
    guard let value else { throw HealthProbeFailure(message: "missing fixture value") }
    return value
}
private func healthWait(_ predicate: () -> Bool) async throws {
    for _ in 0..<2_000 {
        if predicate() { return }
        try await Task.sleep(nanoseconds: 1_000_000)
    }
    throw HealthProbeFailure(message: "two-second fixture wait expired")
}

private extension VesperDashSession {
    func healthExpireManifest() { manifestLoadedAt = .distantPast }
}

private final class HealthDashClient: VesperDashNetworkClient {
    let manifestURL = URL(string: "https://origin.test/manifest.mpd")!
    private let dynamic: Bool
    private let lock = NSLock()
    private var generation = 0
    private var manifestRequests = 0
    private var mediaRequests = 0
    private var gated: Bool
    private var continuations: [CheckedContinuation<Void, Never>] = []

    init(dynamic: Bool = false, gateMedia: Bool = false) {
        self.dynamic = dynamic
        gated = gateMedia
        super.init(headers: ["Authorization": "Bearer fixture"])
    }
    private func locked<T>(_ operation: () -> T) -> T {
        lock.lock(); defer { lock.unlock() }; return operation()
    }
    var manifestRequestCount: Int { locked { manifestRequests } }
    var mediaRequestCount: Int { locked { mediaRequests } }
    func advanceManifest() { locked { generation += 1 } }
    func releaseMedia() {
        let waiting = locked { () -> [CheckedContinuation<Void, Never>] in
            gated = false
            let waiting = continuations; continuations = []; return waiting
        }
        waiting.forEach { $0.resume() }
    }
    override func manifestData(for url: URL) async throws -> (Data, URL) {
        let generation = locked { manifestRequests += 1; return self.generation }
        let template = dynamic
            ? """
              <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="$RepresentationID$-$Time$.m4s" startNumber="\(101 + generation)">
                <SegmentTimeline><S t="\(200000 + 2000 * generation)" d="2000" r="2"/></SegmentTimeline>
              </SegmentTemplate>
              """
            : """
              <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="$RepresentationID$-$Number$.m4s" startNumber="1" duration="2000"/>
              """
        let audio = dynamic ? "" : """
          <AdaptationSet mimeType="audio/mp4" contentType="audio">\(template)
            <Representation id="audio" bandwidth="64000" codecs="mp4a.40.2"/>
          </AdaptationSet>
          """
        let mpd = """
          <MPD type="\(dynamic ? "dynamic" : "static")" \(dynamic ? "minimumUpdatePeriod=\"PT0.5S\"" : "mediaPresentationDuration=\"PT6S\"")>
            <Period><AdaptationSet mimeType="video/mp4" contentType="video">\(template)
              <Representation id="video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
            </AdaptationSet>\(audio)</Period>
          </MPD>
          """
        return (Data(mpd.utf8), url)
    }
    override func data(for url: URL, byteRange: VesperDashByteRange? = nil) async throws -> Data {
        locked { mediaRequests += 1 }
        await withCheckedContinuation { continuation in
            let wait = locked { () -> Bool in
                if gated { continuations.append(continuation); return true }; return false
            }
            if !wait { continuation.resume() }
        }
        return Data(url.lastPathComponent.utf8)
    }
    override func download(for url: URL, byteRange: VesperDashByteRange? = nil, to destinationURL: URL) async throws -> UInt64 {
        let data = try await data(for: url, byteRange: byteRange)
        try FileManager.default.createDirectory(at: destinationURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: destinationURL)
        return UInt64(data.count)
    }
}

private final class HealthCancellationClient: VesperDashNetworkClient {
    override func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [HealthStalledProtocol.self]
        configuration.timeoutIntervalForResource = 2
        return URLSession(configuration: configuration)
    }
}

private final class HealthStalledProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    private static var didStart = false
    private static var didStop = false
    static var started: Bool { lock.lock(); defer { lock.unlock() }; return didStart }
    static var stopped: Bool { lock.lock(); defer { lock.unlock() }; return didStop }
    static func reset() { lock.lock(); defer { lock.unlock() }; didStart = false; didStop = false }
    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "cancel.test" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() { Self.lock.lock(); Self.didStart = true; Self.lock.unlock() }
    override func stopLoading() { Self.lock.lock(); Self.didStop = true; Self.lock.unlock() }
}

#if VESPER_HEALTH_STANDALONE
@main
private struct HealthProbeMain {
    static func main() async throws {
        let test = VesperDashSupportedRouteRegressionTests()
        try await test.testAVPrefetchCacheDoesNotReplaceNativeRedirect()
        print("PASS S21: A/V prefetch init+2; private hit; HTTPS redirect retained; SDK redirect fetches=0")
        try await test.testDynamicTemplateRefreshWhileOldPrefetchCompletesPreservesNativeRoute()
        print("PASS S22: old worker -> refresh -> native number/time URL -> old completion; current redirect unchanged")
        try await test.testDirectNetworkRequestCancellationStopsURLSessionTask()
        print("PASS S23: URLSession request cancellation reports cancelled and invokes stopLoading")
        try await test.testFinitePrefetchReleasesSessionAndRemovesPrivateCacheAfterOwnerRelease()
        print("PASS S23: owner release -> finite init+2 completes -> session deinit -> cache removed")
    }
}
#endif
