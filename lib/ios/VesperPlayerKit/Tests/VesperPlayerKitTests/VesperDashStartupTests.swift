import XCTest
@testable import VesperPlayerKit

final class VesperDashStartupTests: XCTestCase {
    private let manifestURL = URL(string: "https://fixture.test/manifest.mpd")!

    @MainActor
    func testSequenceAcceptsDashGoalThroughRebuiltNativeBridge() throws {
        let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: "dash-goal-test"))
        defer { sequence.dispose() }
        try sequence.replace([.init(itemId: "a", contentIdentity: .init(providerNamespace: "test", value: "a"),
                                    source: .dash(url: manifestURL),
                                    cacheIdentity: .init(providerNamespace: "test", contentIdentity: "a", renditionIdentity: "v1",
                                                         resourceIdentity: "startup", accessPartition: "public", sourceRevision: 1),
                                    sourceRevision: 1)])
        XCTAssertEqual(sequence.snapshot.items.first?.sourceState, "resolved")
        XCTAssertNotNil(sequence.snapshot.wire["warmupTasks"])
    }

    func testFormalSessionAndHTTPReuseWarmBytesAndRefetchAfterClear() async throws {
        let cache = VesperDashStartupCache()
        let scope = VesperDashStartupScope()
        let transport = try DashStartupFixtureTransport()
        let source = VesperPlayerSource.dash(url: manifestURL, headers: ["Authorization": "Bearer one"])
        let result = try await vesperWarmDashStartup(source: source, scope: scope, cache: cache, transport: transport,
                                                    capability: testHardwareVideoDecodeCapabilityProvider)
        XCTAssertFalse(result.hit)
        var count = await transport.requests
        XCTAssertEqual(count, 4)
        let client = VesperDashStartupNetworkClient(scope: scope, headers: source.headers, cache: cache, transport: transport)
        let session = VesperDashSession(sourceURL: manifestURL, networkClient: client,
                                        videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider)
        defer { session.closeStartupResources() }
        let playlist = String(decoding: try await session.mediaPlaylistData(renditionId: "v1"), as: UTF8.self)
        let localURLs = try XCTUnwrap(try NSRegularExpression(pattern: "http://127\\.0\\.0\\.1:[0-9]+/[A-Fa-f0-9-]+\\.mp4"))
            .matches(in: playlist, range: NSRange(playlist.startIndex..., in: playlist)).map {
                URL(string: String(playlist[Range($0.range, in: playlist)!]))!
            }
        XCTAssertEqual(localURLs.count, 2, playlist)
        XCTAssertTrue(playlist.contains("https://fixture.test/video.mp4"), "Later ranges keep their upstream URL")
        let net = URLSession(configuration: .ephemeral)
        defer { net.invalidateAndCancel() }
        let initData = try await net.data(from: localURLs[0]).0
        let mediaData = try await net.data(from: localURLs[1]).0
        let video = await transport.video
        XCTAssertEqual(initData, video.subdata(in: 0..<771))
        XCTAssertEqual(mediaData, video.subdata(in: 847..<11669))
        count = await transport.requests
        XCTAssertEqual(count, 4, "Formal MPD/SIDX/init/first-media reads must reuse the prewarm")
        var request = URLRequest(url: localURLs[1])
        request.setValue("bytes=2-9", forHTTPHeaderField: "Range")
        let (partial, response) = try await net.data(for: request)
        XCTAssertEqual(partial, video.subdata(in: 849..<857))
        XCTAssertEqual((response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Range"), "bytes 2-9/10822")
        request.httpMethod = "HEAD"
        let (head, headResponse) = try await net.data(for: request)
        XCTAssertTrue(head.isEmpty)
        XCTAssertEqual((headResponse as? HTTPURLResponse)?.statusCode, 206)
        let unknown = localURLs[0].deletingLastPathComponent().appendingPathComponent("unknown.mp4")
        let rejected = try await net.data(from: unknown).1 as? HTTPURLResponse
        XCTAssertEqual(rejected?.statusCode, 404)
        await cache.clear()
        let refetched = try await net.data(from: localURLs[1]).0
        XCTAssertEqual(refetched, mediaData)
        count = await transport.requests
        XCTAssertEqual(count, 5, "Registered routes refetch the original bounded range after eviction")
        session.closeStartupResources()
        do { _ = try await net.data(from: localURLs[0]); XCTFail("Closed listener accepted a request") } catch {}
    }

    func testPartialFailureAndCancelledWarmupNeverCommit() async throws {
        let cache = VesperDashStartupCache()
        let transport = try DashStartupFixtureTransport(failAt: 4)
        do {
            _ = try await vesperWarmDashStartup(source: .dash(url: manifestURL), scope: .init(), cache: cache, transport: transport,
                                               capability: testHardwareVideoDecodeCapabilityProvider)
            XCTFail("Failed media request must abort the staged set")
        } catch {}
        var inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 0)
        let suspended = expectation(description: "third resource in flight")
        let blocked = BlockingStartupTransport(base: try DashStartupFixtureTransport(), suspended: suspended)
        let task = Task {
            return try await vesperWarmDashStartup(source: .dash(url: manifestURL), scope: .init(), cache: cache, transport: blocked,
                                                  capability: testHardwareVideoDecodeCapabilityProvider)
        }
        await fulfillment(of: [suspended], timeout: 3)
        task.cancel()
        do { _ = try await task.value; XCTFail("Cancelled warmup completed") } catch {}
        inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 0)
    }

    func testConfiguredBudgetCapsWarmupAndAllSourcesOwnedBySequence() async throws {
        let cache = VesperDashStartupCache()
        do {
            _ = try await vesperWarmDashStartup(source: .dash(url: manifestURL), scope: .init(), cache: cache,
                                               transport: DashStartupFixtureTransport(), maximumBytes: 1,
                                               capability: testHardwareVideoDecodeCapabilityProvider)
            XCTFail("A one-byte budget must reject a complete MPD")
        } catch {}
        let inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 0)
        let old = VesperDashStartupScope(owner: "sequence")
        let new = VesperDashStartupScope(owner: "sequence")
        let resource = VesperDashStartupResource(url: manifestURL)
        let value = VesperDashStartupBytes(resource: resource, data: Data([1, 2]), finalURL: manifestURL)
        _ = try await cache.store(scope: old, values: [value], headers: [:], expectedGeneration: 0, maximumBytes: 2)
        _ = try await cache.store(scope: new, values: [value], headers: [:], expectedGeneration: 0, maximumBytes: 2)
        let evicted = await cache.read(scope: old, resource: resource, headers: [:])
        let retained = await cache.read(scope: new, resource: resource, headers: [:])
        XCTAssertNil(evicted)
        XCTAssertEqual(retained?.data, value.data)
    }

    func testManifestFinalURLSurvivesBothCacheHitAndMiss() async throws {
        let cache = VesperDashStartupCache()
        let scope = VesperDashStartupScope()
        let client = RedirectStartupClient(scope: scope, headers: [:], cache: cache)
        let finalURL = URL(string: "https://fixture.test/redirected/manifest.mpd")!
        let value = VesperDashStartupBytes(resource: .init(url: manifestURL), data: RedirectManifestProtocol.manifest, finalURL: finalURL)
        _ = try await cache.store(scope: scope, values: [value], headers: [:], expectedGeneration: 0)
        for clear in [false, true] {
            if clear { await cache.clear() }
            let session = VesperDashSession(sourceURL: manifestURL, networkClient: client,
                                            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider)
            defer { session.closeStartupResources() }
            let manifest = try await session.loadManifest()
            XCTAssertEqual(manifest.periods[0].adaptationSets[0].representations[0].baseURL, "https://fixture.test/redirected/video.mp4")
        }
    }

    func testCacheIsolatesCredentialsRevisionTTLAndClearGeneration() async throws {
        let clock = StartupTestClock()
        let cache = VesperDashStartupCache(nowMs: { clock.now })
        let scope = VesperDashStartupScope(ttlMs: 10)
        let value = VesperDashStartupBytes(resource: .init(url: manifestURL), data: Data([1, 2]), finalURL: manifestURL)
        let generation = await cache.currentGeneration()
        _ = try await cache.store(scope: scope, values: [value], headers: ["Authorization": "one"], expectedGeneration: generation)
        let same = await cache.read(scope: scope, resource: value.resource, headers: ["authorization": "one"])
        XCTAssertEqual(same?.data, value.data)
        let credentials = await cache.read(scope: scope, resource: value.resource, headers: ["Authorization": "two"])
        let revision = await cache.read(scope: .init(), resource: value.resource, headers: ["Authorization": "one"])
        XCTAssertNil(credentials)
        XCTAssertNil(revision)
        clock.advance(11)
        let expired = await cache.read(scope: scope, resource: value.resource, headers: ["Authorization": "one"])
        XCTAssertNil(expired)
        await cache.clear()
        let stored = try await cache.store(scope: scope, values: [value], headers: [:], expectedGeneration: generation)
        XCTAssertFalse(stored)
    }

    func testStrictRangeResponseAndSegmentLocalRangeValidation() throws {
        let resource = VesperDashStartupResource(url: manifestURL, range: try .init(start: 50, end: 59))
        for (status, fields) in [(200, ["Content-Length": "10"]), (206, ["Content-Range": "bytes 0-9/100"]),
                                 (206, ["Content-Range": "bytes 50-59/100", "Content-Encoding": "gzip"]),
                                 (206, ["Content-Range": "bytes 50-59/100", "Content-Length": "100"])] {
            let response = HTTPURLResponse(url: manifestURL, statusCode: status, httpVersion: nil, headerFields: fields)!
            XCTAssertThrowsError(try VesperDashStartupHTTPTransport.validate(response, resource: resource, maximumBytes: 10))
        }
        let valid = HTTPURLResponse(url: manifestURL, statusCode: 206, httpVersion: nil,
                                    headerFields: ["Content-Range": "bytes 50-59/100", "Content-Length": "10"])!
        XCTAssertNoThrow(try VesperDashStartupHTTPTransport.validate(valid, resource: resource, maximumBytes: 10))
        XCTAssertEqual(VesperDashStartupServer.responseRange("bytes=-3", count: 10), 7..<10)
        XCTAssertEqual(VesperDashStartupServer.responseRange("bytes=3-", count: 10), 3..<10)
        for invalid in ["bytes=0-1,3-4", "bytes=10-", "bytes=5-3", "bytes=+1-2", "bytes=0-99999999999999999999999"] {
            XCTAssertNil(VesperDashStartupServer.responseRange(invalid, count: 10))
        }
    }
}

private struct BlockingStartupTransport: VesperDashStartupTransport {
    let base: DashStartupFixtureTransport
    let suspended: XCTestExpectation
    func fetch(_ resource: VesperDashStartupResource, headers: [String: String], maximumBytes: Int) async throws -> VesperDashStartupBytes {
        let value = try await base.fetch(resource, headers: headers, maximumBytes: maximumBytes)
        if await base.requests == 3 {
            suspended.fulfill()
            try await Task.sleep(for: .seconds(30))
        }
        return value
    }
}

private final class RedirectStartupClient: VesperDashStartupNetworkClient {
    override func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [RedirectManifestProtocol.self]
        return URLSession(configuration: configuration)
    }
}

private final class RedirectManifestProtocol: URLProtocol {
    static let manifest = Data("""
    <MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v1" codecs="avc1.64001f">
    <BaseURL>video.mp4</BaseURL><SegmentBase indexRange="771-846"><Initialization range="0-770"/></SegmentBase>
    </Representation></AdaptationSet></Period></MPD>
    """.utf8)
    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "fixture.test" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let finalURL = URL(string: "https://fixture.test/redirected/manifest.mpd")!
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: finalURL, statusCode: 200, httpVersion: nil, headerFields: [:])!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.manifest)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private final class StartupTestClock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: UInt64 = 100
    var now: UInt64 { lock.withLock { value } }
    func advance(_ delta: UInt64) { lock.withLock { value += delta } }
}
