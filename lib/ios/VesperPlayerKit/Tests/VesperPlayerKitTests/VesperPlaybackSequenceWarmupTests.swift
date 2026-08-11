import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperPlaybackSequenceWarmupTests: XCTestCase {
    @MainActor
    func testProgressiveRequestOverridesRangeAndStoresOnly64KiB() async throws {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(
            responses: [
                VesperSequenceWarmupHTTPResponse(
                    statusCode: 206,
                    data: Data(repeating: 0x41, count: 128 * 1024)
                )
            ]
        )
        let terminal = expectation(description: "warmup completed")
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: { report in
                reports.append(report)
                if report.status == "completed" { terminal.fulfill() }
            },
            cache: cache,
            loader: loader
        )
        let source = progressiveSource(
            revision: 1,
            headers: [
                "Authorization": "Bearer private-token",
                "Referer": "https://example.invalid/watch",
                "rAnGe": "bytes=99-100",
            ]
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, _ in source }
        await fulfillment(of: [terminal], timeout: 2)

        let capturedRequests = await loader.requests()
        let byteLimits = await loader.maximumByteLimits()
        let stored = await cache.storedData(for: cacheKey(revision: 1))
        let request = try XCTUnwrap(capturedRequests.first)
        XCTAssertEqual(request.value(forHTTPHeaderField: "Range"), "bytes=0-65535")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer private-token")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Referer"), "https://example.invalid/watch")
        XCTAssertEqual(byteLimits, [64 * 1024])
        XCTAssertEqual(stored?.count, 64 * 1024)
        XCTAssertEqual(reports.last?.status, "completed")
        XCTAssertEqual(reports.last?.actualBytes, 64 * 1024)
        XCTAssertEqual(reports.last?.cacheHit, false)
        XCTAssertEqual(executor.snapshot.completedJobs, 1)
        XCTAssertEqual(executor.snapshot.actualBytes, 64 * 1024)
        executor.close()
    }

    @MainActor
    func testSecondExecutorHitsCacheWithoutCallingLoaderAgain() async {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(
            responses: [
                VesperSequenceWarmupHTTPResponse(
                    statusCode: 206,
                    data: Data(repeating: 0x42, count: 64 * 1024)
                )
            ]
        )
        let firstTerminal = expectation(description: "first warmup completed")
        let first = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in },
            onReport: { if $0.status == "completed" { firstTerminal.fulfill() } },
            cache: cache,
            loader: loader
        )
        first.reconcile(intents: [intent(revision: 1)]) { _, _, _ in self.progressiveSource(revision: 1) }
        await fulfillment(of: [firstTerminal], timeout: 2)
        first.close()

        let secondTerminal = expectation(description: "second warmup completed")
        var secondReport: VesperSequenceWarmupReport?
        let second = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in },
            onReport: {
                if $0.status == "completed" {
                    secondReport = $0
                    secondTerminal.fulfill()
                }
            },
            cache: cache,
            loader: loader
        )
        second.reconcile(intents: [intent(revision: 1)]) { _, _, _ in self.progressiveSource(revision: 1) }
        await fulfillment(of: [secondTerminal], timeout: 2)

        let loaderCallCount = await loader.callCount()
        XCTAssertEqual(loaderCallCount, 1)
        XCTAssertEqual(secondReport?.cacheHit, true)
        XCTAssertEqual(secondReport?.cacheEntries, 1)
        XCTAssertEqual(secondReport?.cacheBytes, 64 * 1024)
        XCTAssertEqual(second.snapshot.cacheHits, 1)
        XCTAssertEqual(second.snapshot.cacheEntries, 1)
        XCTAssertEqual(second.snapshot.cacheBytes, 64 * 1024)
        second.close()
    }

    @MainActor
    func testStableTaskDoesNotRestartAfterWindowExitOrPriorityChange() async {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(
            responses: [
                VesperSequenceWarmupHTTPResponse(
                    statusCode: 206,
                    data: Data(repeating: 0x42, count: 64 * 1024)
                ),
                VesperSequenceWarmupHTTPResponse(
                    statusCode: 206,
                    data: Data(repeating: 0x43, count: 64 * 1024)
                ),
            ]
        )
        let firstTerminal = expectation(description: "first warmup completed")
        var sourceLookups = 0
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: { if $0.status == "completed" { firstTerminal.fulfill() } },
            cache: cache,
            loader: loader
        )
        let nextIntent = intent(revision: 1)

        executor.reconcile(intents: [nextIntent]) { _, _, revision in
            sourceLookups += 1
            return self.progressiveSource(revision: revision)
        }
        await fulfillment(of: [firstTerminal], timeout: 2)
        executor.reconcile(intents: []) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        var currentIntent = nextIntent
        currentIntent["priority"] = "current"
        executor.reconcile(intents: [currentIntent]) { _, _, revision in
            sourceLookups += 1
            return self.progressiveSource(revision: revision)
        }

        XCTAssertEqual(sourceLookups, 1)
        XCTAssertEqual(executor.snapshot.completedJobs, 1)
        executor.close()
    }

    @MainActor
    func testCloseDuringSourceLookupCannotInsertJob() async {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(responses: [])
        var reports: [VesperSequenceWarmupReport] = []
        var executor: VesperPlaybackSequenceWarmupExecutor!
        executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: { reports.append($0) },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, revision in
            executor.close()
            return self.progressiveSource(revision: revision)
        }

        let loaderCallCount = await loader.callCount()
        XCTAssertTrue(reports.isEmpty)
        XCTAssertEqual(executor.snapshot.activeJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 0)
        XCTAssertEqual(loaderCallCount, 0)
    }

    @MainActor
    func testCloseDuringSourceLookupCannotRecordMissingSourceFailure() {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(responses: [])
        var reports: [VesperSequenceWarmupReport] = []
        var executor: VesperPlaybackSequenceWarmupExecutor!
        executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: { reports.append($0) },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, _ in
            executor.close()
            return nil
        }

        XCTAssertTrue(reports.isEmpty)
        XCTAssertEqual(executor.snapshot.activeJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 0)
    }

    @MainActor
    func testTerminalCommitDuringSourceLookupPreventsSameKeyInsertion() {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(responses: [])
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: { reports.append($0) },
            cache: cache,
            loader: loader
        )
        let taskIntent = intent(revision: 1)

        executor.reconcile(intents: [taskIntent]) { _, _, revision in
            executor.reconcile(intents: [taskIntent]) { _, _, _ in nil }
            return self.progressiveSource(revision: revision)
        }

        XCTAssertEqual(reports.map(\.status), ["failed"])
        XCTAssertEqual(reports.map(\.reasonCode), ["source_reference_missing"])
        XCTAssertEqual(executor.snapshot.activeJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 1)
        executor.close()
    }

    @MainActor
    func testActiveJobInsertedDuringSourceLookupCannotRecordMissingSourceFailure() async {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(
            responses: [
                VesperSequenceWarmupHTTPResponse(
                    statusCode: 206,
                    data: Data(repeating: 0x44, count: 64 * 1024)
                )
            ]
        )
        let terminal = expectation(description: "nested warmup completed")
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: {
                reports.append($0)
                if $0.status == "completed" { terminal.fulfill() }
            },
            cache: cache,
            loader: loader
        )
        let taskIntent = intent(revision: 1)

        executor.reconcile(intents: [taskIntent]) { _, _, _ in
            executor.reconcile(intents: [taskIntent]) { _, _, nestedRevision in
                self.progressiveSource(revision: nestedRevision)
            }
            return nil
        }
        await fulfillment(of: [terminal], timeout: 2)

        let loaderCallCount = await loader.callCount()
        XCTAssertEqual(loaderCallCount, 1)
        XCTAssertFalse(reports.contains { $0.status == "failed" })
        XCTAssertEqual(reports.filter { $0.status == "completed" }.count, 1)
        XCTAssertEqual(executor.snapshot.activeJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 0)
        XCTAssertEqual(executor.snapshot.completedJobs, 1)
        executor.close()
    }

    @MainActor
    func testCacheStoreFailureCannotReportCompleted() async {
        let cache = TestWarmupCache(failStore: true)
        let loader = ImmediateWarmupLoader(
            responses: [
                VesperSequenceWarmupHTTPResponse(
                    statusCode: 206,
                    data: Data(repeating: 0x43, count: 64 * 1024)
                )
            ]
        )
        let terminal = expectation(description: "warmup failed")
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in },
            onReport: {
                reports.append($0)
                if $0.status == "failed" { terminal.fulfill() }
            },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, _ in self.progressiveSource(revision: 1) }
        await fulfillment(of: [terminal], timeout: 2)

        XCTAssertEqual(reports.last?.reasonCode, "cache_store_failed")
        XCTAssertFalse(reports.contains { $0.status == "completed" })
        XCTAssertEqual(executor.snapshot.completedJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 1)
        executor.close()
    }

    @MainActor
    func testCacheHitInventoryFailureCannotReportCompletedWithZeroInventory() async throws {
        let cache = TestWarmupCache(failInventory: true)
        _ = try await cache.store(
            key: cacheKey(revision: 1),
            data: Data(repeating: 0x43, count: 64 * 1024)
        )
        let loader = ImmediateWarmupLoader(responses: [])
        let terminal = expectation(description: "warmup inventory failed")
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in },
            onReport: {
                reports.append($0)
                if $0.status == "failed" { terminal.fulfill() }
            },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, _ in
            self.progressiveSource(revision: 1)
        }
        await fulfillment(of: [terminal], timeout: 2)

        XCTAssertEqual(reports.last?.reasonCode, "cache_inventory_failed")
        XCTAssertEqual(reports.last?.actualBytes, 64 * 1024)
        XCTAssertFalse(reports.contains { $0.status == "completed" })
        XCTAssertEqual(executor.snapshot.completedJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 1)
        executor.close()
    }

    @MainActor
    func testExpiredStatusesNotifyOnlyAfterCurrentTerminalCommit() async {
        for status in [401, 403, 410, 404] {
            let cache = TestWarmupCache()
            let loader = ImmediateWarmupLoader(
                responses: [VesperSequenceWarmupHTTPResponse(statusCode: status, data: Data())]
            )
            let terminal = expectation(description: "status \(status) failed")
            var expiries: [(String, UInt64)] = []
            var terminalReport: VesperSequenceWarmupReport?
            let executor = VesperPlaybackSequenceWarmupExecutor(
                onSourceExpired: { expiries.append(($0, $1)) },
                onReport: {
                    if $0.status == "failed" {
                        terminalReport = $0
                        terminal.fulfill()
                    }
                },
                cache: cache,
                loader: loader
            )

            executor.reconcile(intents: [intent(revision: UInt64(status))]) { _, _, revision in
                self.progressiveSource(revision: revision)
            }
            await fulfillment(of: [terminal], timeout: 2)

            if status == 404 {
                XCTAssertTrue(expiries.isEmpty)
                XCTAssertEqual(terminalReport?.reasonCode, "http_failure")
            } else {
                XCTAssertEqual(expiries.count, 1)
                XCTAssertEqual(expiries.first?.0, "item-\(status)")
                XCTAssertEqual(expiries.first?.1, UInt64(status))
                XCTAssertEqual(terminalReport?.reasonCode, "source_expired")
            }
            executor.close()
        }
    }

    @MainActor
    func testRevisionReplacementFencesLateCompletion() async throws {
        let cache = TestWarmupCache()
        let loader = SuspendedWarmupLoader()
        let terminals = expectation(description: "both revisions terminate")
        terminals.expectedFulfillmentCount = 2
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: {
                reports.append($0)
                if $0.status != "started" { terminals.fulfill() }
            },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        try await waitForPendingRequests(loader, count: 1)
        executor.reconcile(intents: [intent(revision: 2)]) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        try await waitForPendingRequests(loader, count: 2)

        await loader.resume(revision: 2, statusCode: 206, byteCount: 64 * 1024)
        await loader.resume(revision: 1, statusCode: 206, byteCount: 64 * 1024)
        await fulfillment(of: [terminals], timeout: 2)

        XCTAssertEqual(reports.filter { $0.status == "completed" }.map(\.sourceRevision), [2])
        XCTAssertEqual(reports.filter { $0.status == "cancelled" }.map(\.sourceRevision), [1])
        let staleData = await cache.storedData(for: cacheKey(revision: 1))
        let currentData = await cache.storedData(for: cacheKey(revision: 2))
        XCTAssertNil(staleData)
        XCTAssertEqual(currentData?.count, 64 * 1024)
        executor.close()
    }

    @MainActor
    func testRevisionCancellationReturnedAsURLErrorCannotReportFailed() async throws {
        let cache = TestWarmupCache()
        let loader = SuspendedWarmupLoader()
        let terminals = expectation(description: "both revisions terminate")
        terminals.expectedFulfillmentCount = 2
        var reports: [VesperSequenceWarmupReport] = []
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in XCTFail("source must not expire") },
            onReport: {
                reports.append($0)
                if $0.status != "started" { terminals.fulfill() }
            },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        try await waitForPendingRequests(loader, count: 1)
        executor.reconcile(intents: [intent(revision: 2)]) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        try await waitForPendingRequests(loader, count: 2)

        await loader.fail(revision: 1, error: URLError(.cancelled))
        await loader.resume(revision: 2, statusCode: 206, byteCount: 64 * 1024)
        await fulfillment(of: [terminals], timeout: 2)

        XCTAssertEqual(reports.filter { $0.status == "cancelled" }.map(\.sourceRevision), [1])
        XCTAssertEqual(reports.filter { $0.status == "completed" }.map(\.sourceRevision), [2])
        XCTAssertFalse(reports.contains { $0.status == "failed" && $0.sourceRevision == 1 })
        XCTAssertEqual(executor.snapshot.failedJobs, 0)
        XCTAssertEqual(executor.snapshot.cancelledJobs, 1)
        executor.close()
    }

    @MainActor
    func testCloseFencesLateExpiryAndTerminalCallbacks() async throws {
        let cache = TestWarmupCache()
        let loader = SuspendedWarmupLoader()
        let unexpectedTerminal = expectation(description: "no terminal callback after close")
        unexpectedTerminal.isInverted = true
        var expiryCount = 0
        let executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in expiryCount += 1 },
            onReport: {
                if $0.status != "started" { unexpectedTerminal.fulfill() }
            },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        try await waitForPendingRequests(loader, count: 1)
        executor.close()
        await loader.resume(revision: 1, statusCode: 401, byteCount: 0)
        await fulfillment(of: [unexpectedTerminal], timeout: 0.2)

        XCTAssertEqual(expiryCount, 0)
        XCTAssertEqual(executor.snapshot.activeJobs, 0)
        XCTAssertEqual(executor.snapshot.failedJobs, 0)
    }

    @MainActor
    func testCloseInsideExpiryCallbackSuppressesTerminalReport() async {
        let cache = TestWarmupCache()
        let loader = ImmediateWarmupLoader(
            responses: [VesperSequenceWarmupHTTPResponse(statusCode: 403, data: Data())]
        )
        let expired = expectation(description: "source expiry callback")
        var reports: [VesperSequenceWarmupReport] = []
        var expiryCount = 0
        var executor: VesperPlaybackSequenceWarmupExecutor!
        executor = VesperPlaybackSequenceWarmupExecutor(
            onSourceExpired: { _, _ in
                expiryCount += 1
                executor.close()
                expired.fulfill()
            },
            onReport: { reports.append($0) },
            cache: cache,
            loader: loader
        )

        executor.reconcile(intents: [intent(revision: 1)]) { _, _, revision in
            self.progressiveSource(revision: revision)
        }
        await fulfillment(of: [expired], timeout: 2)

        XCTAssertEqual(expiryCount, 1)
        XCTAssertFalse(reports.contains { $0.status == "failed" && $0.reasonCode == "source_expired" })
        XCTAssertEqual(executor.snapshot.activeJobs, 0)
    }

    func testFileCacheEnforcesPhysicalEntryAndByteCaps() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-sequence-cache-test-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let cache = VesperSequenceFileCache(
            maxBytes: 12,
            directory: directory,
            maxEntries: 2
        )

        _ = try await cache.store(key: "first", data: Data(repeating: 1, count: 4))
        _ = try await cache.store(key: "second", data: Data(repeating: 2, count: 4))
        let afterEntryEviction = try await cache.store(
            key: "third",
            data: Data(repeating: 3, count: 4)
        )

        XCTAssertEqual(afterEntryEviction.entries, 2)
        XCTAssertEqual(afterEntryEviction.bytes, 8)
        XCTAssertEqual(try physicalCacheEntryCount(directory), 2)

        let afterByteEviction = try await cache.configure(maxBytes: 4)
        XCTAssertEqual(afterByteEviction.entries, 1)
        XCTAssertEqual(afterByteEviction.bytes, 4)
        XCTAssertEqual(try physicalCacheEntryCount(directory), 1)
    }

    func testSaturatingCounterDoesNotWrap() {
        XCTAssertEqual(vesperSequenceSaturatingAdd(UInt64.max, 1), UInt64.max)
        XCTAssertEqual(vesperSequenceSaturatingAdd(UInt64.max - 1, 1), UInt64.max)
        XCTAssertEqual(vesperSequenceSaturatingAdd(1, 2), 3)
    }

    private func intent(revision: UInt64) -> [String: Any] {
        [
            "sessionGeneration": UInt64(7),
            "itemId": "item-\(revision)",
            "sourceReference": "source-\(revision)",
            "sourceRevision": revision,
            "warmupTaskId": UInt64(100) + revision,
            "warmupGoal": "progressiveRange",
            "priority": "next",
            "cacheIdentity": ["canonicalKey": cacheKey(revision: revision)],
            "profile": [
                "expectedMemoryBytes": UInt64.max,
                "warmupWindowMs": UInt64(1_000),
            ],
        ]
    }

    private func cacheKey(revision: UInt64) -> String {
        "provider:content:rendition:resource:partition:\(revision)"
    }

    private func progressiveSource(
        revision: UInt64,
        headers: [String: String] = [:]
    ) -> VesperPlayerSource {
        VesperPlayerSource(
            uri: "https://example.invalid/video.mp4?revision=\(revision)",
            label: "fixture",
            kind: .remote,
            protocol: .progressive,
            headers: headers
        )
    }

    private func waitForPendingRequests(
        _ loader: SuspendedWarmupLoader,
        count: Int
    ) async throws {
        for _ in 0..<1_000 {
            if await loader.pendingCount() == count { return }
            try await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTFail("Timed out waiting for \(count) suspended warmup requests")
    }

    private func physicalCacheEntryCount(_ directory: URL) throws -> Int {
        try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "bin" }
            .count
    }
}

private actor TestWarmupCache: VesperSequenceWarmupCaching {
    private var values: [String: Data] = [:]
    private let failStore: Bool
    private let failInventory: Bool

    init(failStore: Bool = false, failInventory: Bool = false) {
        self.failStore = failStore
        self.failInventory = failInventory
    }

    func configure(maxBytes _: UInt64) -> VesperSequenceCacheInventory {
        currentInventory()
    }

    func read(key: String, length: Int) -> Data? {
        guard let value = values[key], value.count >= length else { return nil }
        return Data(value.prefix(length))
    }

    func store(key: String, data: Data) throws -> VesperSequenceCacheInventory {
        if failStore { throw VesperSequenceCacheError.storeFailed }
        values[key] = data
        return currentInventory()
    }

    func inventory() throws -> VesperSequenceCacheInventory {
        if failInventory { throw VesperSequenceCacheError.inventoryFailed }
        return currentInventory()
    }

    func storedData(for key: String) -> Data? {
        values[key]
    }

    private func currentInventory() -> VesperSequenceCacheInventory {
        VesperSequenceCacheInventory(
            evicted: 0,
            entries: values.count,
            bytes: values.values.reduce(UInt64(0)) {
                vesperSequenceSaturatingAdd($0, UInt64($1.count))
            }
        )
    }
}

private actor ImmediateWarmupLoader: VesperSequenceWarmupLoading {
    private var pendingResponses: [VesperSequenceWarmupHTTPResponse]
    private var capturedRequests: [URLRequest] = []
    private var limits: [Int] = []

    init(responses: [VesperSequenceWarmupHTTPResponse]) {
        pendingResponses = responses
    }

    func load(
        request: URLRequest,
        maximumBytes: Int
    ) throws -> VesperSequenceWarmupHTTPResponse {
        capturedRequests.append(request)
        limits.append(maximumBytes)
        guard !pendingResponses.isEmpty else {
            throw VesperSequenceWarmupLoadingError.nonHTTPResponse
        }
        return pendingResponses.removeFirst()
    }

    func requests() -> [URLRequest] { capturedRequests }
    func maximumByteLimits() -> [Int] { limits }
    func callCount() -> Int { capturedRequests.count }
}

private actor SuspendedWarmupLoader: VesperSequenceWarmupLoading {
    private var continuations: [UInt64: CheckedContinuation<VesperSequenceWarmupHTTPResponse, Error>] = [:]

    func load(
        request: URLRequest,
        maximumBytes _: Int
    ) async throws -> VesperSequenceWarmupHTTPResponse {
        let revision = UInt64(URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?
            .queryItems?.first(where: { $0.name == "revision" })?.value ?? "") ?? 0
        return try await withCheckedThrowingContinuation { continuation in
            continuations[revision] = continuation
        }
    }

    func pendingCount() -> Int { continuations.count }

    func resume(revision: UInt64, statusCode: Int, byteCount: Int) {
        continuations.removeValue(forKey: revision)?.resume(
            returning: VesperSequenceWarmupHTTPResponse(
                statusCode: statusCode,
                data: Data(repeating: 0x44, count: byteCount)
            )
        )
    }

    func fail(revision: UInt64, error: Error) {
        continuations.removeValue(forKey: revision)?.resume(throwing: error)
    }
}
