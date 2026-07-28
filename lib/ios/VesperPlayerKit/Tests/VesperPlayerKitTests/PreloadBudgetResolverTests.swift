import XCTest
@testable import VesperPlayerKit

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
