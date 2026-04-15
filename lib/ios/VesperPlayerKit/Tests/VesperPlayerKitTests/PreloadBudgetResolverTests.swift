import XCTest
@testable import VesperPlayerKit

final class PreloadBudgetResolverTests: XCTestCase {
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
