import XCTest
@testable import VesperPlayerKit
import VesperPlayerKitBridgeShim

final class PreloadBudgetResolverTests: XCTestCase {
    func testRuntimeBridgeUInt32EncodingClampsNegativeAndOversizedValues() {
        XCTAssertEqual(encodeRuntimeUInt32(-1, field: "negative"), 0)
        XCTAssertEqual(
            encodeRuntimeUInt32(Int(UInt32.max) + 1, field: "oversized"),
            UInt32.max
        )
    }

    func testPreloadBudgetPayloadClampsConcurrentTaskCount() {
        let negative = VesperPreloadBudgetPolicy(maxConcurrentTasks: -1).toRuntimeBridgePayload()
        let oversized = VesperPreloadBudgetPolicy(
            maxConcurrentTasks: Int(UInt32.max) + 1
        ).toRuntimeBridgePayload()

        XCTAssertEqual(negative.max_concurrent_tasks, 0)
        XCTAssertEqual(oversized.max_concurrent_tasks, UInt32.max)
    }

    func testRuntimeBridgeInt32EncodingClampsBothResolutionAxes() {
        XCTAssertEqual(encodeRuntimeInt32(-1, field: "maxWidth"), 0)
        XCTAssertEqual(
            encodeRuntimeInt32(Int(Int32.max) + 1, field: "maxWidth"),
            Int32.max
        )
        XCTAssertEqual(encodeRuntimeInt32(-1, field: "maxHeight"), 0)
        XCTAssertEqual(
            encodeRuntimeInt32(Int(Int32.max) + 1, field: "maxHeight"),
            Int32.max
        )
    }

    func testTrackPreferencePayloadClampsResolutionWithoutChangingPublicPolicy() {
        let policy = VesperTrackPreferencePolicy(
            abrPolicy: .constrained(
                maxWidth: -1,
                maxHeight: Int(Int32.max) + 1
            )
        )

        let resolved = policy.resolvedForRuntime()

        XCTAssertEqual(policy.abrPolicy.maxWidth, -1)
        XCTAssertEqual(policy.abrPolicy.maxHeight, Int(Int32.max) + 1)
        XCTAssertEqual(resolved.abrPolicy.maxWidth, 0)
        XCTAssertEqual(resolved.abrPolicy.maxHeight, Int(Int32.max))
    }

    func testRetryPayloadClampsAttemptsWithoutChangingPublicPolicy() {
        let negative = VesperRetryPolicy(maxAttempts: Int.min)
        let oversized = VesperRetryPolicy(maxAttempts: Int.max)

        XCTAssertEqual(negative.maxAttempts, Int.min)
        XCTAssertEqual(negative.toRuntimeBridgePayload().max_attempts, 0)
        XCTAssertEqual(oversized.maxAttempts, Int.max)
        XCTAssertEqual(oversized.toRuntimeBridgePayload().max_attempts, Int32.max)
    }

    func testPlaylistPayloadClampsWindowsWithoutChangingPublicConfiguration() {
        let configuration = VesperPlaylistConfiguration(
            neighborWindow: VesperPlaylistNeighborWindow(
                previous: Int.min,
                next: Int.max
            ),
            preloadWindow: VesperPlaylistPreloadWindow(
                nearVisible: Int.min,
                prefetchOnly: Int.max
            )
        )
        var payload = configuration.toRuntimeBridgePayload()
        defer { freePlaylistCString(payload.playlist_id) }

        XCTAssertEqual(configuration.neighborWindow.previous, Int.min)
        XCTAssertEqual(configuration.neighborWindow.next, Int.max)
        XCTAssertEqual(configuration.preloadWindow.nearVisible, Int.min)
        XCTAssertEqual(configuration.preloadWindow.prefetchOnly, Int.max)
        XCTAssertEqual(payload.neighbor_previous, 0)
        XCTAssertEqual(payload.neighbor_next, UInt32.max)
        XCTAssertEqual(payload.preload_near_visible, 0)
        XCTAssertEqual(payload.preload_prefetch_only, UInt32.max)
    }

    func testPreloadBudgetResolvesSparseDefaultsFromRuntime() {
        let resolved = VesperPreloadBudgetPolicy(maxDiskBytes: 512).resolvedForRuntime()

        XCTAssertEqual(resolved.maxConcurrentTasks, 2)
        XCTAssertEqual(resolved.maxMemoryBytes, 64 * 1024 * 1024)
        XCTAssertEqual(resolved.maxDiskBytes, 512)
        XCTAssertEqual(resolved.warmupWindowMs, 30_000)
    }

    func testPreloadBudgetPreservesExplicitZeroOverrides() {
        let resolved =
            VesperPreloadBudgetPolicy(
                maxConcurrentTasks: 0,
                maxMemoryBytes: 0,
                maxDiskBytes: 0,
                warmupWindowMs: 0
            ).resolvedForRuntime()

        XCTAssertEqual(resolved.maxConcurrentTasks, 0)
        XCTAssertEqual(resolved.maxMemoryBytes, 0)
        XCTAssertEqual(resolved.maxDiskBytes, 0)
        XCTAssertEqual(resolved.warmupWindowMs, 0)
    }
}


@MainActor
final class VesperNativePreloadCoordinatorLifecycleTests: XCTestCase {
    func testWarmupRestartsAndOldCompletionCannotTargetTheNewSession() async throws {
        let probe = WarmupExecutionProbe(test: self)
        let coordinator = VesperNativePreloadCoordinator(
            budgetPolicy: .init(maxConcurrentTasks: 1, maxMemoryBytes: 32 * 1024, maxDiskBytes: 0)
        ) { request in await probe.execute(request) }
        defer { probe.releaseAll(); coordinator.close() }
        coordinator.configure(cachePolicy: .init(enabled: true, memoryCapacity: 64 * 1024, diskCapacity: 0))
        let url = URL(string: "https://example.com/video.mp4")!
        let source = VesperPlayerSource.remoteUrl(url)
        coordinator.warmCurrentSource(source: source, url: url)
        await fulfillment(of: [probe.started[0]], timeout: 5)
        let first = try XCTUnwrap(probe.requests.first)
        coordinator.cancelCurrent()
        XCTAssertEqual(coordinator.sessionHandle, 0)
        // The same locator must be admitted again; the cancelled planner owns no dedup reservation.
        coordinator.warmCurrentSource(source: source, url: url)
        await fulfillment(of: [probe.started[1]], timeout: 5)
        XCTAssertEqual(probe.requests.count, 2)
        let second = try XCTUnwrap(probe.requests.last)
        XCTAssertNotEqual(first.sessionHandle, second.sessionHandle)
        XCTAssertEqual(second.sessionHandle, coordinator.sessionHandle)
        probe.gates[0].continuation.finish()
        await fulfillment(of: [probe.finished[0]], timeout: 5)
        XCTAssertEqual(probe.completions[0], false)
        probe.gates[1].continuation.finish()
        await fulfillment(of: [probe.finished[1]], timeout: 5)
        XCTAssertEqual(probe.completions[1], true)
        coordinator.close()
        coordinator.close()
        coordinator.configure(cachePolicy: .init(enabled: true, memoryCapacity: 64 * 1024, diskCapacity: 0))
        coordinator.warmCurrentSource(source: source, url: url)
        XCTAssertTrue(coordinator.isClosed)
        XCTAssertEqual(coordinator.sessionHandle, 0)
    }

    func testAcceptedScopeAndDisabledCacheDoNotStartGenericWarmup() {
        let coordinator = VesperNativePreloadCoordinator(budgetPolicy: .init()) { _ in
            XCTFail("ineligible sources must not create a generic warmup")
        }
        defer { coordinator.close() }
        let url = URL(string: "https://example.com/manifest.mpd")!
        coordinator.warmCurrentSource(source: .remoteUrl(url), url: url)
        XCTAssertEqual(coordinator.sessionHandle, 0)
        coordinator.configure(cachePolicy: .init(enabled: true, memoryCapacity: 64 * 1024, diskCapacity: 0))
        var scoped = VesperPlayerSource.remoteUrl(url)
        scoped.dashStartupScope = .init(owner: "test")
        coordinator.warmCurrentSource(source: scoped, url: url)
        XCTAssertEqual(coordinator.sessionHandle, 0)
    }
}

@MainActor
private final class WarmupExecutionProbe {
    let started: [XCTestExpectation]
    let finished: [XCTestExpectation]
    let gates = [AsyncStream<Void>.makeStream(), AsyncStream<Void>.makeStream()]
    var requests: [VesperNativePreloadCoordinator.WarmupRequest] = []
    var completions: [Bool] = []
    init(test: XCTestCase) {
        started = (0..<2).map { test.expectation(description: "warmup started \($0)") }
        finished = (0..<2).map { test.expectation(description: "warmup settled \($0)") }
    }
    func execute(_ request: VesperNativePreloadCoordinator.WarmupRequest) async {
        let index = requests.count
        guard index < gates.count else { XCTFail("unexpected extra warmup"); return }
        requests.append(request)
        started[index].fulfill()
        let gate = gates[index].stream
        // Keep an old completion in flight even after cancellation.
        await withCheckedContinuation { continuation in
            Task.detached {
                for await _ in gate {}
                continuation.resume()
            }
        }
        completions.append(vesper_runtime_preload_session_complete(request.sessionHandle, request.taskId))
        finished[index].fulfill()
    }
    func releaseAll() { gates.forEach { $0.continuation.finish() } }
}
