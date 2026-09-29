import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperSourceSessionTests: XCTestCase {
    private let manifestURL = URL(string: "https://fixture.test/manifest.mpd")!

    func testLocalPreloadSurvivesControllerCachePolicyAndManifestDeletion() async throws {
        let fixture = try DashStartupFixtureTransport()
        let manifest = await fixture.manifest
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("source-formal-\(UUID().uuidString).mpd")
        try manifest.write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        let session = try VesperSourceSession(cache: .shared, transport: fixture,
                                              capability: testHardwareVideoDecodeCapabilityProvider)
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose(); session.close() }
        let handle = try session.register(.init(uri: file.absoluteString, label: "local DASH", kind: .local, protocol: .dash))
        let preload = await (try handle.preload()).result
        XCTAssertEqual(preload.status, .completed)
        try FileManager.default.removeItem(at: file)
        let lease = try handle.acquire()
        defer { lease.release() }
        let source = try lease.sourceForActivation()
        _ = try bridge.makePlayerItem(for: source, url: file)
        let dashSession = try XCTUnwrap(bridge.currentDashSession)
        let (bytes, finalURL) = try await dashSession.networkClient.manifestData(for: file)
        XCTAssertEqual(bytes, manifest)
        XCTAssertEqual(finalURL, file)
    }

    func testLocalManifestThroughIndependentHandlePreload() async throws {
        let fixture = try DashStartupFixtureTransport()
        let data = await fixture.manifest
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("source-session-\(UUID().uuidString).mpd")
        try data.write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        let headers = ["Referer": "https://app.test/", "User-Agent": "VesperSourceSessionTest"]
        let cache = VesperDashStartupCache()
        let session = try VesperSourceSession(cache: cache, transport: HeaderCheckingStartupTransport(base: fixture, headers: headers),
                                          capability: testHardwareVideoDecodeCapabilityProvider)
        defer { session.close() }
        let handle = try session.register(.dash(url: file, headers: headers))
        let result = await (try handle.preload()).result
        XCTAssertEqual(result.status, .completed)
        let source = try session.acquire(handle).sourceForActivation()
        let scope = try XCTUnwrap(source.dashStartupScope)
        let bytes = await cache.read(scope: scope, resource: .init(url: file), headers: headers)
        XCTAssertEqual(bytes?.data, data)
        XCTAssertEqual(bytes?.finalURL, file)
        let requests = await fixture.requests
        XCTAssertEqual(requests, 3)
    }

    func testIndependentPreloadRetainsExactScopeAcrossAcquisitions() async throws {
        let cache = VesperDashStartupCache()
        let transport = try DashStartupFixtureTransport()
        let session = try VesperSourceSession(cache: cache, transport: transport,
                                          capability: testHardwareVideoDecodeCapabilityProvider)
        defer { session.close() }
        let source = VesperPlayerSource.dash(url: manifestURL, headers: ["Authorization": "Bearer one"])
        let handle = try session.register(source)
        XCTAssertNil(handle.source.dashStartupScope, "Public descriptor must not expose the accepted scope")
        let first = try session.acquire(handle)
        let second = try session.acquire(handle)
        let firstSource = try first.sourceForActivation()
        let secondSource = try second.sourceForActivation()
        XCTAssertNotNil(firstSource.dashStartupScope)
        XCTAssertEqual(firstSource.dashStartupScope, secondSource.dashStartupScope)
        let task = try handle.preload()
        let result = await task.result
        XCTAssertEqual(result.status, .completed)
        XCTAssertEqual(result.goal, .dashSegmentBaseStartup)
        XCTAssertEqual(result.capability, .playbackReusable)
        XCTAssertEqual(result.cacheHit, false)
        let scope = try XCTUnwrap(secondSource.dashStartupScope)
        let cached = await cache.read(scope: scope, resource: .init(url: manifestURL), headers: source.headers)
        XCTAssertNotNil(cached, "Acquired formal source must address the preload bytes")
        let again = try handle.preload()
        let repeated = await again.result
        XCTAssertEqual(repeated.cacheHit, true)
        let requests = await transport.requests
        XCTAssertEqual(requests, 4)
        first.release()
        XCTAssertThrowsError(try first.sourceForActivation())
        second.release()
    }

    func testRegistrationAndCredentialIsolation() async throws {
        let cache = VesperDashStartupCache()
        let transport = try DashStartupFixtureTransport()
        let session = try VesperSourceSession(cache: cache, transport: transport,
                                          capability: testHardwareVideoDecodeCapabilityProvider)
        defer { session.close() }
        let a = try session.register(.dash(url: manifestURL, headers: ["Authorization": "Bearer a"]))
        let b = try session.register(.dash(url: manifestURL, headers: ["Authorization": "Bearer b"]))
        let c = try session.register(a.source)
        let aSource = try session.acquire(a).sourceForActivation()
        let bSource = try session.acquire(b).sourceForActivation()
        let cSource = try session.acquire(c).sourceForActivation()
        XCTAssertNotEqual(aSource.dashStartupScope, bSource.dashStartupScope)
        XCTAssertNotEqual(aSource.dashStartupScope, cSource.dashStartupScope)
        let first = await (try a.preload()).result
        let second = await (try b.preload()).result
        XCTAssertEqual(first.cacheHit, false)
        XCTAssertEqual(second.cacheHit, false)
        let requestCount = await transport.requests
        XCTAssertEqual(requestCount, 8)
    }

    func testCloseRetainsLeaseButInvalidationRevokesEvenAfterClose() throws {
        let session = try VesperSourceSession()
        let handle = try session.register(.dash(url: manifestURL))
        let lease = try session.acquire(handle)
        handle.close()
        handle.close()
        XCTAssertThrowsError(try session.acquire(handle))
        XCTAssertThrowsError(try handle.preload())
        XCTAssertNoThrow(try lease.sourceForActivation())
        session.close()
        XCTAssertNoThrow(try lease.sourceForActivation())
        session.invalidate()
        XCTAssertThrowsError(try lease.sourceForActivation()) { XCTAssertEqual($0 as? VesperSourceSessionError, .invalidated) }
        XCTAssertThrowsError(try session.register(.dash(url: manifestURL)))
    }

    func testHandleInvalidationRevokesRetainedLeaseAndExpiryPreventsWork() throws {
        var now: UInt64 = 10
        let session = try VesperSourceSession(cache: .init(), transport: VesperDashStartupHTTPTransport(), now: { now })
        defer { session.close() }
        let handle = try session.register(.dash(url: manifestURL), expiresAtEpochMs: 20)
        let lease = try session.acquire(handle)
        now = 20
        XCTAssertThrowsError(try handle.preload()) { XCTAssertEqual($0 as? VesperSourceSessionError, .expired) }
        XCTAssertThrowsError(try session.acquire(handle))
        XCTAssertThrowsError(try lease.sourceForActivation())
        now = 10
        XCTAssertThrowsError(try lease.sourceForActivation()) { XCTAssertEqual($0 as? VesperSourceSessionError, .expired) }
        handle.close()
        handle.invalidate()
        XCTAssertThrowsError(try lease.sourceForActivation()) { XCTAssertEqual($0 as? VesperSourceSessionError, .invalidated) }
        XCTAssertThrowsError(try session.register(.dash(url: manifestURL), expiresAtEpochMs: 10))
    }

    func testMonotonicExpiryRejectsRollbackBeforeWallDeadline() throws {
        var wall: UInt64 = 100
        var monotonic: UInt64 = 10
        let session = try VesperSourceSession(cache: .init(), transport: VesperDashStartupHTTPTransport(),
                                              now: { wall }, monotonicNow: { monotonic })
        defer { session.close() }
        let handle = try session.register(.dash(url: manifestURL), expiresAtEpochMs: 200)
        let lease = try handle.acquire()
        wall = 50
        monotonic = 110
        XCTAssertThrowsError(try lease.sourceForActivation()) {
            XCTAssertEqual($0 as? VesperSourceSessionError, .expired)
        }
        XCTAssertThrowsError(try handle.preload()) {
            XCTAssertEqual($0 as? VesperSourceSessionError, .expired)
        }
    }

    func testSequenceDetachDoesNotCancelManuallySharedPreload() async throws {
        let loader = SourceSessionGateLoader()
        let session = try VesperSourceSession(cache: .init(), transport: VesperDashStartupHTTPTransport(), progressiveLoader: loader)
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in })
        let controller = VesperPlayerController(bridge)
        let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: UUID().uuidString))
        defer { sequence.dispose(); controller.dispose(); session.close() }
        let handle = try session.register(.remoteUrl(URL(string: "https://fixture.test/video.mp4")!))
        let task = try handle.preload()
        await loader.waitUntilStarted()
        try sequence.replace([.init(itemId: "a", contentIdentity: .init(providerNamespace: "test", value: "a"), source: handle)])
        try sequence.attach(to: controller)
        _ = try await sequence.activate("a", options: .init(playWhenReady: false))
        sequence.detach()
        XCTAssertEqual(task.snapshot.status, .running)
        await loader.open()
        let result = await task.result
        XCTAssertEqual(result.status, .completed)
    }

    func testSourceLimitForeignHandleUnsupportedAndDisabledBudget() async throws {
        let session = try VesperSourceSession(configuration: .init(maxSources: 1))
        let other = try VesperSourceSession()
        defer { session.close(); other.close() }
        let handle = try session.register(.remoteUrl(URL(string: "https://fixture.test/master.m3u8")!, protocol: .hls))
        XCTAssertThrowsError(try session.register(.dash(url: manifestURL))) { XCTAssertEqual($0 as? VesperSourceSessionError, .sourceLimit) }
        XCTAssertThrowsError(try other.preload(handle)) { XCTAssertEqual($0 as? VesperSourceSessionError, .foreignHandle) }
        let unsupported = await (try handle.preload()).result
        XCTAssertEqual(unsupported.status, .unsupported)
        XCTAssertEqual(unsupported.goal, .unsupported)
        XCTAssertEqual(unsupported.capability, .none)
        handle.close()
        XCTAssertNoThrow(try session.register(.dash(url: manifestURL)))
        let disabled = try VesperSourceSession(configuration: .init(maxMemoryBytes: 0))
        defer { disabled.close() }
        let result = await (try disabled.register(.dash(url: manifestURL)).preload()).result
        XCTAssertEqual(result.status, .unsupported)
        XCTAssertEqual(result.reasonCode, "cache_disabled")
        XCTAssertEqual(result.capability, .none)
    }

    func testCancellationKeepsWorkerSlotUntilUncooperativeLoaderExits() async throws {
        let loader = SourceSessionGateLoader()
        let cache = VesperDashStartupCache()
        let session = try VesperSourceSession(configuration: .init(maxConcurrentPreloads: 1, maxPendingPreloads: 1),
                                          cache: cache, transport: VesperDashStartupHTTPTransport(), progressiveLoader: loader)
        defer { session.close() }
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://fixture.test/video.mp4")!)
        let a = try session.register(source), b = try session.register(source), c = try session.register(source)
        let first = try a.preload()
        XCTAssertTrue(first === (try a.preload()), "Active requests for the same handle deduplicate")
        await loader.waitUntilStarted()
        let second = try b.preload()
        XCTAssertEqual(second.snapshot.status, .queued)
        XCTAssertThrowsError(try c.preload()) { XCTAssertEqual($0 as? VesperSourceSessionError, .preloadQueueFull) }
        first.cancel()
        let cancelled = await first.result
        XCTAssertEqual(cancelled.status, .cancelled)
        XCTAssertEqual(second.snapshot.status, .queued)
        XCTAssertThrowsError(try c.preload(), "Cancellation must not release a still-running worker's slot")
        await loader.open()
        let completed = await second.result
        XCTAssertEqual(completed.status, .completed)
        XCTAssertEqual(completed.capability, .downloadOnly)
        let inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 1, "Only the second worker may publish retained bytes")
    }

    func testDisposeCancelsAndFencesLateCommit() async throws {
        let loader = SourceSessionGateLoader()
        let cache = VesperDashStartupCache()
        let session = try VesperSourceSession(cache: cache, transport: VesperDashStartupHTTPTransport(), progressiveLoader: loader)
        let handle = try session.register(.remoteUrl(URL(string: "https://fixture.test/video.mp4")!))
        let task = try handle.preload()
        await loader.waitUntilStarted()
        session.dispose()
        let result = await task.result
        XCTAssertEqual(result.status, .cancelled)
        await loader.open()
        // Even a direct late publication through the worker's token must be rejected.
        let token = VesperPreloadCommitToken()
        token.cancel()
        do {
            _ = try await cache.store(scope: .init(), values: [.init(resource: .init(url: manifestURL), data: Data([1]), finalURL: manifestURL)],
                                      headers: [:], expectedGeneration: 0, commitToken: token)
            XCTFail("Revoked commit token accepted a late cache publication")
        } catch is CancellationError { }
        let inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 0)
    }

    func testTimeoutRetainsWorkerSlotAndQueue() async throws {
        let loader = SourceSessionGateLoader()
        let session = try VesperSourceSession(configuration: .init(maxConcurrentPreloads: 1), cache: .init(),
                                          transport: VesperDashStartupHTTPTransport(), progressiveLoader: loader)
        defer { session.close() }
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://fixture.test/video.mp4")!)
        let a = try session.register(source), b = try session.register(source)
        let first = try a.preload(options: .init(timeoutMs: 20))
        await loader.waitUntilStarted()
        let second = try b.preload()
        let timedOut = await first.result
        XCTAssertEqual(timedOut.status, .failed)
        XCTAssertEqual(timedOut.reasonCode, "timeout")
        XCTAssertEqual(second.snapshot.status, .queued)
        await loader.open()
        let result = await second.result
        XCTAssertEqual(result.status, .completed)
    }

    func testBudgetRejectsDashWithoutPartialCommit() async throws {
        let cache = VesperDashStartupCache()
        let fixture = try DashStartupFixtureTransport()
        let session = try VesperSourceSession(configuration: .init(maxMemoryBytes: 64), cache: cache, transport: fixture,
                                          capability: testHardwareVideoDecodeCapabilityProvider)
        defer { session.close() }
        let result = await (try session.register(.dash(url: manifestURL)).preload()).result
        XCTAssertEqual(result.status, .failed)
        XCTAssertEqual(result.reasonCode, "budget_exceeded")
        let inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 0)
    }

    func testPreloadFailureReportsTypedCauseWithoutProtectedRequestDetails() async throws {
        let session = try VesperSourceSession(cache: .init(), transport: FailingSourcePreloadTransport())
        defer { session.close() }
        let result = await (try session.register(.dash(url: manifestURL)).preload()).result
        XCTAssertEqual(result.status, .failed)
        XCTAssertEqual(result.reasonCode, "network_timeout")
    }

    func testPerTaskByteCapRejectsDashWithoutPartialCommit() async throws {
        let cache = VesperDashStartupCache()
        let session = try VesperSourceSession(cache: cache, transport: try DashStartupFixtureTransport(),
                                          capability: testHardwareVideoDecodeCapabilityProvider)
        defer { session.close() }
        let handle = try session.register(.dash(url: manifestURL))
        let result = await (try handle.preload(options: .init(maximumBytes: 64))).result
        XCTAssertEqual(result.status, .failed)
        let inventory = await cache.inventory()
        XCTAssertEqual(inventory.entries, 0)
    }

    func testAggregateStagingReservationBoundsPendingWork() async throws {
        let loader = SourceSessionGateLoader()
        let session = try VesperSourceSession(configuration: .init(maxConcurrentPreloads: 2, maxPendingPreloads: 1, maxMemoryBytes: 64),
                                          cache: .init(), transport: VesperDashStartupHTTPTransport(), progressiveLoader: loader)
        defer { session.close() }
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://fixture.test/video.mp4")!)
        let a = try session.register(source), b = try session.register(source), c = try session.register(source)
        let first = try a.preload(options: .init(maximumBytes: 64))
        await loader.waitUntilStarted()
        let second = try b.preload(options: .init(maximumBytes: 64))
        XCTAssertEqual(second.snapshot.status, .queued, "Concurrency allowance cannot override the staging byte budget")
        XCTAssertThrowsError(try c.preload())
        await loader.open()
        let firstResult = await first.result
        let secondResult = await second.result
        XCTAssertEqual(firstResult.actualBytes, 64)
        XCTAssertEqual(secondResult.actualBytes, 64)
    }

    func testQueuedDeadlineDoesNotReleaseAnUncooperativeWorkersReservation() async throws {
        let loader = SourceSessionGateLoader()
        let session = try VesperSourceSession(configuration: .init(maxConcurrentPreloads: 2, maxPendingPreloads: 1, maxMemoryBytes: 64),
                                              cache: .init(), transport: VesperDashStartupHTTPTransport(), progressiveLoader: loader)
        defer { session.close() }
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://fixture.test/video.mp4")!)
        let first = try session.register(source).preload(options: .init(maximumBytes: 64, timeoutMs: 100))
        await loader.waitUntilStarted()
        let queued = try session.register(source).preload(options: .init(maximumBytes: 64, timeoutMs: 20))
        XCTAssertEqual(queued.snapshot.status, .queued)
        let queueTimeout = await queued.result
        XCTAssertEqual(queueTimeout.status, .failed)
        XCTAssertEqual(queueTimeout.reasonCode, "timeout")
        let workerTimeout = await first.result
        XCTAssertEqual(workerTimeout.reasonCode, "timeout")
        let third = try session.register(source).preload(options: .init(maximumBytes: 64))
        XCTAssertEqual(third.snapshot.status, .queued)
        XCTAssertThrowsError(try session.register(source).preload())
        third.cancel()
        await loader.open()
    }

    func testInvalidBudgetsAreRejectedWithoutSilentlyChangingRequestedValues() throws {
        XCTAssertThrowsError(try VesperSourceSession(configuration: .init(maxConcurrentPreloads: 0)))
        XCTAssertThrowsError(try VesperSourceSession(configuration: .init(maxPendingPreloads: 33)))
        XCTAssertThrowsError(try VesperSourceSession(configuration: .init(maxMemoryBytes: 16 * 1024 * 1024 + 1)))
        let session = try VesperSourceSession()
        defer { session.close() }
        let handle = try session.register(.dash(url: manifestURL))
        XCTAssertThrowsError(try handle.preload(options: .init(maximumBytes: 0)))
        XCTAssertThrowsError(try handle.preload(options: .init(timeoutMs: 60_001)))
    }
}

private struct FailingSourcePreloadTransport: VesperDashStartupTransport {
    func fetch(_ resource: VesperDashStartupResource, headers: [String: String], maximumBytes: Int) async throws -> VesperDashStartupBytes {
        throw URLError(.timedOut, userInfo: [NSLocalizedDescriptionKey: "https://protected.test/media?token=secret"])
    }
}

/// Deliberately ignores cooperative cancellation to exercise slot retention and commit fencing.
private actor SourceSessionGateLoader: VesperSequenceWarmupLoading {
    private var opened = false
    private var started = false
    private var starts: [CheckedContinuation<Void, Never>] = []
    private var gates: [CheckedContinuation<Void, Never>] = []
    func waitUntilStarted() async {
        if started { return }
        await withCheckedContinuation { starts.append($0) }
    }
    func open() {
        opened = true
        let pending = gates
        gates.removeAll()
        pending.forEach { $0.resume() }
    }
    func load(request: URLRequest, maximumBytes: Int) async throws -> VesperSequenceWarmupHTTPResponse {
        started = true
        let pending = starts
        starts.removeAll()
        pending.forEach { $0.resume() }
        if !opened { await withCheckedContinuation { gates.append($0) } }
        return .init(statusCode: 206, data: Data(repeating: 7, count: maximumBytes))
    }
}
