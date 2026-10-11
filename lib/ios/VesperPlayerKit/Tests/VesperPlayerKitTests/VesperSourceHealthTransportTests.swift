import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperSourceHealthTransportTests: XCTestCase {
    func testManifestRetainsFinalResponseUrl() async throws {
        let fixture = SourceHealthHTTPFixture { _ in
            .init(status: 200, finalURL: URL(string: "https://redirect.fixture.test/movie/index.mpd")!,
                  body: Data("<MPD/>".utf8))
        }
        defer { fixture.remove() }
        let client = SourceHealthDashClient(configuration: fixture.configuration)
        let (data, finalURL) = try await client.manifestData(for: fixture.url("manifest"))
        XCTAssertEqual(data, Data("<MPD/>".utf8))
        XCTAssertEqual(finalURL.absoluteString, "https://redirect.fixture.test/movie/index.mpd")
    }

    func testDashRangeRejectsIgnoredRangeAndAcceptsExact206() async throws {
        for status in [200, 206] {
            let fixture = SourceHealthHTTPFixture { request in
                XCTAssertEqual(request.value(forHTTPHeaderField: "Range"), "bytes=4-7")
                XCTAssertEqual(request.value(forHTTPHeaderField: "Accept-Encoding"), "identity")
                return .init(status: status, headers: ["Content-Range": "bytes 4-7/8", "Content-Length": "4"], body: Data([4, 5, 6, 7]))
            }
            defer { fixture.remove() }
            let client = SourceHealthDashClient(configuration: fixture.configuration)
            do {
                let data = try await client.data(for: fixture.url("media"), byteRange: .init(start: 4, end: 7))
                XCTAssertEqual(status, 206)
                XCTAssertEqual(data, Data([4, 5, 6, 7]))
            } catch {
                XCTAssertEqual(status, 200)
            }
        }
    }

    func testDashRangeRejectsMetadataLengthAndEncodingMismatches() throws {
        let client = VesperDashNetworkClient()
        let url = URL(string: "https://fixture.test/media")!
        let valid = ["Content-Range": "bytes 4-7/8", "Content-Length": "4"]
        for headers in [[:], ["Content-Range": "bytes 0-3/8"], ["Content-Range": "bytes 4-7/7"],
                        ["Content-Range": "bytes 4-7/8", "Content-Length": "5"],
                        ["Content-Range": "bytes 4-7/8", "Content-Encoding": "gzip"]] {
            let response = HTTPURLResponse(url: url, statusCode: 206, httpVersion: nil, headerFields: headers)!
            XCTAssertThrowsError(try client.validateRangeResponse(response, byteRange: .init(start: 4, end: 7), bodyLength: 4, sourceURL: url))
        }
        let response = HTTPURLResponse(url: url, statusCode: 206, httpVersion: nil, headerFields: valid)!
        XCTAssertThrowsError(try client.validateRangeResponse(response, byteRange: .init(start: 4, end: 7), bodyLength: 3, sourceURL: url))
        XCTAssertNoThrow(try client.validateRangeResponse(response, byteRange: .init(start: 4, end: 7), bodyLength: 4, sourceURL: url))
    }

    func testSizeProbeRejectsHeadErrorLengthAndReturnsBeforeBodyEof() async throws {
        let stopped = expectation(description: "both response streams cancelled")
        stopped.expectedFulfillmentCount = 2
        let fixture = SourceHealthHTTPFixture(onStop: { stopped.fulfill() }) { request in
            if request.httpMethod == "HEAD" {
                return .init(status: 405, headers: ["Content-Length": "17"])
            }
            XCTAssertEqual(request.value(forHTTPHeaderField: "Range"), "bytes=0-0")
            return .init(status: 206, headers: ["Content-Range": "bytes 0-0/1024", "Content-Length": "1"], body: Data([0]), finishes: false)
        }
        defer { fixture.remove() }
        let executor = VesperForegroundDownloadExecutor(stalledTransferTimeoutMs: 2_000)
        executor.httpSessionConfiguration = fixture.configuration
        defer { executor.dispose() }
        let size = try await executor.probeContentLength(fixture.url("probe"), requestHeaders: [:])
        XCTAssertEqual(size, 1024)
        await fulfillment(of: [stopped], timeout: 2)
        XCTAssertEqual(fixture.requests.map { $0.httpMethod }, ["HEAD", "GET"])
    }

    func testSizeProbeAcceptsIgnoredRange200HeadersWithoutReadingItsBody() async throws {
        let stopped = expectation(description: "both response streams cancelled")
        stopped.expectedFulfillmentCount = 2
        let fixture = SourceHealthHTTPFixture(onStop: { stopped.fulfill() }) { request in
            request.httpMethod == "HEAD" ? .init(status: 501)
                : .init(status: 200, headers: ["Content-Length": "65536"], body: Data([0]), finishes: false)
        }
        defer { fixture.remove() }
        let executor = VesperForegroundDownloadExecutor(stalledTransferTimeoutMs: 2_000)
        executor.httpSessionConfiguration = fixture.configuration
        defer { executor.dispose() }
        let size = try await executor.probeContentLength(fixture.url("probe"), requestHeaders: [:])
        XCTAssertEqual(size, 65536)
        await fulfillment(of: [stopped], timeout: 2)
    }

    func testSizeProbeRejectsInvalid206AndUntrustedStatus() async throws {
        let invalid: [SourceHealthHTTPReply] = [
            .init(status: 206, headers: [:]),
            .init(status: 206, headers: ["Content-Range": "bytes 1-1/9", "Content-Length": "1"]),
            .init(status: 206, headers: ["Content-Range": "bytes 0-0/0", "Content-Length": "1"]),
            .init(status: 206, headers: ["Content-Range": "bytes 0-0/9", "Content-Length": "2"]),
            .init(status: 206, headers: ["Content-Range": "bytes 0-0/9", "Content-Encoding": "gzip"]),
            .init(status: 500, headers: ["Content-Length": "9"])
        ]
        for reply in invalid {
            let fixture = SourceHealthHTTPFixture { request in
                request.httpMethod == "HEAD" ? .init(status: 405) : reply
            }
            defer { fixture.remove() }
            let executor = VesperForegroundDownloadExecutor(stalledTransferTimeoutMs: 2_000)
            executor.httpSessionConfiguration = fixture.configuration
            defer { executor.dispose() }
            do {
                _ = try await executor.probeContentLength(fixture.url("probe"), requestHeaders: [:])
                XCTFail("invalid response was accepted: \(reply.status) \(reply.headers)")
            } catch {}
        }
    }

    func testWarmupRetainsOnlyBudgetedPrefixAndCancelsUnderlyingTaskBeforeEof() async throws {
        for status in [200, 206] {
            let stopped = expectation(description: "warmup transport cancelled")
            let fixture = SourceHealthHTTPFixture(onStop: { path in if path == "/large" { stopped.fulfill() } }) { _ in
                .init(status: status, body: Data((0..<8192).map { UInt8($0 % 251) }), finishes: false)
            }
            defer { fixture.remove() }
            let session = URLSession(configuration: fixture.configuration)
            defer { session.invalidateAndCancel() }
            let result = try await VesperSequenceURLSessionWarmupLoader(session: session).load(request: URLRequest(url: fixture.url("large")), maximumBytes: 127)
            XCTAssertEqual(result.statusCode, status)
            XCTAssertEqual(result.data, Data((0..<127).map { UInt8($0 % 251) }))
            await fulfillment(of: [stopped], timeout: 2)
            // The shared/injected session itself remains usable after per-request cancellation.
            let next = try await VesperSequenceURLSessionWarmupLoader(session: session).load(request: URLRequest(url: fixture.url("next")), maximumBytes: 8)
            XCTAssertEqual(next.data.count, 8)
        }
    }

    func testWarmupCancellationCancelsStalledUnderlyingTask() async throws {
        let began = expectation(description: "transport began")
        let stopped = expectation(description: "cancelled transport stopped")
        let fixture = SourceHealthHTTPFixture(onStop: { stopped.fulfill() }) { _ in
            began.fulfill()
            return .init(status: 200, finishes: false)
        }
        defer { fixture.remove() }
        let session = URLSession(configuration: fixture.configuration)
        defer { session.invalidateAndCancel() }
        let loader = VesperSequenceURLSessionWarmupLoader(session: session)
        let work = Task { try await loader.load(request: URLRequest(url: fixture.url("blocked")), maximumBytes: 127) }
        await fulfillment(of: [began], timeout: 2)
        work.cancel()
        do { _ = try await work.value; XCTFail("cancelled warmup should not return data") } catch {}
        await fulfillment(of: [stopped], timeout: 2)
    }

    @MainActor
    func testStaleRecoveryPreparesRemoteEmptyIndexWithoutSelfCancellation() async throws {
        let prepared = expectation(description: "recovered remote manifest prepared")
        let fixture = SourceHealthHTTPFixture { request in
            switch request.url!.path {
            case "/stale": return .init(status: 403)
            case "/fresh.m3u8": return .init(status: 200, body: Data("#EXTM3U\n#EXTINF:1,\nseg.ts\n#EXT-X-ENDLIST\n".utf8))
            default: return .init(status: 200, headers: ["Content-Length": "4"])
            }
        }
        defer { fixture.remove() }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let fresh = VesperDownloadSource(source: .hls(url: fixture.url("fresh.m3u8")))
        let executor = VesperForegroundDownloadExecutor(baseDirectory: directory, staleResourceRecoveryHandler: { _, _ in fresh })
        executor.httpSessionConfiguration = fixture.configuration
        defer { executor.dispose() }
        let reporter = SourceHealthRecoveryReporter(prepared: prepared)
        executor.start(task: healthStaleTask(fixture.url("stale")), reporter: reporter)
        await fulfillment(of: [prepared], timeout: 3)
        XCTAssertNil(reporter.failure)
        XCTAssertEqual(reporter.replacedCount, 1)
        XCTAssertEqual(reporter.preparedIndex?.segments.count, 1)
        XCTAssertFalse(reporter.preparedIndex?.resources.isEmpty ?? true)
    }

    @MainActor
    func testRemoveDuringPrepareRecoveryDoesNotRecreateGeneratedResources() async throws {
        for shouldRemove in [true, false] {
            let entered = expectation(description: "prepare recovery is held")
            let recovery = SourceHealthHeldRecovery()
            let fixture = SourceHealthHTTPFixture { _ in .init(status: 403) }
            defer { fixture.remove() }
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let assetId = "source-health-prepare-recovery"
            let assetDirectory = directory.appendingPathComponent(assetId, isDirectory: true)
            try FileManager.default.createDirectory(at: assetDirectory, withIntermediateDirectories: true)
            try Data("existing task".utf8).write(to: assetDirectory.appendingPathComponent("marker"))
            let task = VesperDownloadTaskSnapshot(
                taskId: 92, assetId: assetId,
                source: VesperDownloadSource(source: .hls(url: fixture.url("expired.m3u8"))),
                profile: VesperDownloadProfile(), state: .preparing,
                progress: VesperDownloadProgressSnapshot(), assetIndex: VesperDownloadAssetIndex()
            )
            let executor = VesperForegroundDownloadExecutor(
                baseDirectory: directory,
                staleResourcePlanRecoveryHandler: { _, stale in
                    XCTAssertEqual(stale.phase, .prepare)
                    return await recovery.wait(entered: entered)
                }
            )
            executor.httpSessionConfiguration = fixture.configuration
            defer { executor.dispose() }
            let reporter = SourceHealthRecoveryReporter()
            executor.prepare(task: task, reporter: reporter)
            await fulfillment(of: [entered], timeout: 3)
            let work = try XCTUnwrap(executor.lock.withLock { executor.tasks[task.taskId]?.work })
            if shouldRemove {
                executor.remove(task: task)
                XCTAssertTrue(work.isCancelled)
                XCTAssertFalse(FileManager.default.fileExists(atPath: assetDirectory.path))
            }
            let manifest = "#EXTM3U\n#EXT-X-ENDLIST\n"
            await recovery.release(plan: VesperDownloadRecoveredTaskPlan(
                source: VesperDownloadSource(source: .hls(url: fixture.url("fresh.m3u8"))),
                profile: task.profile,
                assetIndex: VesperDownloadAssetIndex(contentFormat: .hlsSegments, resources: [
                    VesperDownloadResourceRecord(resourceId: "manifest", uri: "vesper-generated://index.m3u8",
                        relativePath: "index.m3u8", generatedText: manifest)
                ])
            ))
            await work.value
            let generated = assetDirectory.appendingPathComponent(".generated/index.m3u8")
            XCTAssertNil(reporter.failure)
            XCTAssertEqual(reporter.completedCount, 0)
            XCTAssertTrue(executor.lock.withLock { executor.tasks.isEmpty })
            if shouldRemove {
                XCTAssertFalse(FileManager.default.fileExists(atPath: assetDirectory.path), "removed task directory must stay absent")
                XCTAssertFalse(FileManager.default.fileExists(atPath: generated.path), "cancelled recovery must not write generated resources")
                XCTAssertEqual(reporter.replacedCount, 0, "cancelled recovery must not publish a replacement plan")
                XCTAssertNil(reporter.preparedIndex)
            } else {
                XCTAssertEqual(reporter.replacedCount, 1)
                XCTAssertNotNil(reporter.preparedIndex)
                XCTAssertEqual(try String(contentsOf: generated, encoding: .utf8), manifest)
            }
        }
    }

    @MainActor
    func testPausedAndResumedDownloadRecoveryReturningNilDoesNotReportOldFailure() async throws {
        let entered = expectation(description: "download recovery is held")
        let resumedRequest = expectation(description: "replacement transfer is active")
        let stopped = expectation(description: "replacement transport cancelled")
        let recovery = SourceHealthHeldRecovery()
        let fixture = SourceHealthHTTPFixture(onStop: { path in
            if path == "/resumed" { stopped.fulfill() }
        }) { request in
            if request.url!.path == "/stale" { return .init(status: 403) }
            resumedRequest.fulfill()
            return .init(status: 200, sendsResponse: false, finishes: false)
        }
        defer { fixture.remove() }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let executor = VesperForegroundDownloadExecutor(
            baseDirectory: directory,
            staleResourcePlanRecoveryHandler: { _, stale in
                XCTAssertEqual(stale.phase, .download)
                XCTAssertEqual(stale.statusCode, 403)
                // Keep the old callback in flight after cancellation until the replacement is active.
                return await recovery.wait(entered: entered)
            }
        )
        executor.httpSessionConfiguration = fixture.configuration
        defer {
            executor.dispose()
            Task { await recovery.release(plan: nil) }
        }
        let reporter = SourceHealthRecoveryReporter()
        let originalTask = healthStaleTask(fixture.url("stale"))
        executor.start(task: originalTask, reporter: reporter)
        await fulfillment(of: [entered], timeout: 3)
        let oldWork = try XCTUnwrap(executor.lock.withLock { executor.tasks[originalTask.taskId]?.work })

        executor.pause(taskId: originalTask.taskId)
        XCTAssertTrue(oldWork.isCancelled)
        let resumedTask = healthStaleTask(fixture.url("resumed"))
        executor.resume(task: resumedTask, reporter: reporter)
        let resumedWork = try XCTUnwrap(executor.lock.withLock { executor.tasks[resumedTask.taskId]?.work })
        await fulfillment(of: [resumedRequest], timeout: 3)
        XCTAssertFalse(resumedWork.isCancelled)
        XCTAssertEqual(reporter.completedCount, 0)
        XCTAssertNil(reporter.failure)

        // Model the nonthrowing Flutter recovery adapter returning nil for cancellation.
        await recovery.release(plan: nil)
        await oldWork.value
        XCTAssertNil(reporter.failure, "the cancelled execution must not report a stale failure after resume")
        XCTAssertEqual(reporter.completedCount, 0)
        XCTAssertEqual(reporter.replacedCount, 0)
        XCTAssertNil(reporter.preparedIndex)
        XCTAssertEqual(fixture.requests.map { $0.url!.path }, ["/stale", "/resumed"])
        XCTAssertNotNil(executor.lock.withLock { executor.tasks[resumedTask.taskId] })
        XCTAssertFalse(resumedWork.isCancelled)
        executor.pause(taskId: resumedTask.taskId)
        XCTAssertTrue(resumedWork.isCancelled)
        await fulfillment(of: [stopped], timeout: 2)
        await resumedWork.value
        XCTAssertNil(reporter.failure)
        XCTAssertTrue(executor.lock.withLock { executor.tasks.isEmpty })
    }

    @MainActor
    func testDeclinedDownloadRecoveryReturningNilReportsOriginalStaleFailure() async throws {
        let fixture = SourceHealthHTTPFixture { _ in .init(status: 403) }
        defer { fixture.remove() }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let executor = VesperForegroundDownloadExecutor(
            baseDirectory: directory,
            staleResourcePlanRecoveryHandler: { _, stale in
                XCTAssertEqual(stale.phase, .download)
                XCTAssertEqual(stale.statusCode, 403)
                return nil
            }
        )
        executor.httpSessionConfiguration = fixture.configuration
        defer { executor.dispose() }
        let reporter = SourceHealthRecoveryReporter()
        let task = healthStaleTask(fixture.url("stale"))
        executor.start(task: task, reporter: reporter)
        let work = try XCTUnwrap(executor.lock.withLock { executor.tasks[task.taskId]?.work })
        await work.value

        XCTAssertFalse(work.isCancelled)
        XCTAssertEqual(reporter.failure?.code, .backendFailure)
        XCTAssertEqual(reporter.failure?.category, .network)
        XCTAssertTrue(reporter.failure?.message.contains("HTTP 403") == true)
        XCTAssertEqual(reporter.completedCount, 0)
        XCTAssertEqual(reporter.replacedCount, 0)
        XCTAssertNil(reporter.preparedIndex)
        XCTAssertEqual(fixture.requests.map { $0.url!.path }, ["/stale"])
    }

    @MainActor
    func testHostPauseDuringRecoveryCancelsManifestWithoutReportingFailure() async throws {
        let began = expectation(description: "recovery manifest request began")
        let stopped = expectation(description: "recovery request cancelled")
        let fixture = SourceHealthHTTPFixture(onStop: { path in if path == "/fresh.m3u8" { stopped.fulfill() } }) { request in
            if request.url!.path == "/stale" { return .init(status: 403) }
            began.fulfill()
            return .init(status: 200, sendsResponse: false, finishes: false)
        }
        defer { fixture.remove() }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let fresh = VesperDownloadSource(source: .hls(url: fixture.url("fresh.m3u8")))
        let executor = VesperForegroundDownloadExecutor(baseDirectory: directory, staleResourceRecoveryHandler: { _, _ in fresh })
        executor.httpSessionConfiguration = fixture.configuration
        defer { executor.dispose() }
        let reporter = SourceHealthRecoveryReporter()
        executor.start(task: healthStaleTask(fixture.url("stale")), reporter: reporter)
        await fulfillment(of: [began], timeout: 3)
        let work = executor.lock.withLock { executor.tasks[91]?.work }
        executor.pause(taskId: 91)
        await fulfillment(of: [stopped], timeout: 2)
        await work?.value
        XCTAssertNil(reporter.failure)
        XCTAssertNil(reporter.preparedIndex)
        XCTAssertTrue(executor.lock.withLock { executor.tasks.isEmpty })
    }
}

private func healthStaleTask(_ url: URL) -> VesperDownloadTaskSnapshot {
    .init(taskId: 91, assetId: "source-health-recovery", source: VesperDownloadSource(source: .remoteUrl(url)),
          profile: VesperDownloadProfile(), state: .downloading, progress: VesperDownloadProgressSnapshot(),
          assetIndex: VesperDownloadAssetIndex(contentFormat: .singleFile, resources: [.init(resourceId: "old", uri: url.absoluteString, relativePath: "old.mp4", sizeBytes: 4)]))
}

private actor SourceHealthHeldRecovery {
    private var continuation: CheckedContinuation<VesperDownloadRecoveredTaskPlan?, Never>?

    func wait(entered: XCTestExpectation) async -> VesperDownloadRecoveredTaskPlan? {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            entered.fulfill()
        }
    }

    func release(plan: VesperDownloadRecoveredTaskPlan?) {
        let pending = continuation
        continuation = nil
        pending?.resume(returning: plan)
    }
}

@MainActor
private final class SourceHealthRecoveryReporter: VesperDownloadExecutionReporter {
    let prepared: XCTestExpectation?
    var replacedCount = 0
    var completedCount = 0
    var preparedIndex: VesperDownloadAssetIndex?
    var failure: VesperDownloadError?
    init(prepared: XCTestExpectation? = nil) { self.prepared = prepared }
    func replaceTaskPlan(taskId: VesperDownloadTaskId, source: VesperDownloadSource, profile: VesperDownloadProfile, assetIndex: VesperDownloadAssetIndex) { replacedCount += 1 }
    func completePreparation(taskId: VesperDownloadTaskId, assetIndex: VesperDownloadAssetIndex) { preparedIndex = assetIndex; prepared?.fulfill() }
    func updateProgress(taskId: VesperDownloadTaskId, receivedBytes: UInt64, receivedSegments: UInt32) {}
    func complete(taskId: VesperDownloadTaskId, completedPath: String?) { completedCount += 1 }
    func fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) { failure = error }
}

private final class SourceHealthDashClient: VesperDashNetworkClient {
    let configuration: URLSessionConfiguration
    init(configuration: URLSessionConfiguration) { self.configuration = configuration; super.init() }
    override func makeSession() -> URLSession { URLSession(configuration: configuration) }
}

private struct SourceHealthHTTPReply {
    var status: Int
    var headers: [String: String] = [:]
    var finalURL: URL? = nil
    var body: Data = Data()
    var sendsResponse: Bool = true
    var finishes: Bool = true
}

private final class SourceHealthHTTPFixture: @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [URLRequest] = []
    let host = UUID().uuidString.lowercased() + ".fixture.test"
    let handler: (URLRequest) -> SourceHealthHTTPReply
    let onStop: (String) -> Void
    init(onStop: @escaping () -> Void = {}, handler: @escaping (URLRequest) -> SourceHealthHTTPReply) {
        self.onStop = { _ in onStop() }; self.handler = handler
        SourceHealthHTTPProtocol.register(self)
    }
    init(onStop: @escaping (String) -> Void, handler: @escaping (URLRequest) -> SourceHealthHTTPReply) {
        self.onStop = onStop; self.handler = handler
        SourceHealthHTTPProtocol.register(self)
    }
    var configuration: URLSessionConfiguration {
        let value = URLSessionConfiguration.ephemeral
        value.protocolClasses = [SourceHealthHTTPProtocol.self]
        return value
    }
    var requests: [URLRequest] { lock.withLock { recorded } }
    func url(_ path: String) -> URL { URL(string: "https://\(host)/\(path)")! }
    func reply(for request: URLRequest) -> SourceHealthHTTPReply {
        lock.withLock { recorded.append(request) }
        return handler(request)
    }
    func remove() { SourceHealthHTTPProtocol.remove(host) }
}

private final class SourceHealthHTTPProtocol: URLProtocol {
    static let lock = NSLock()
    static var fixtures: [String: SourceHealthHTTPFixture] = [:]
    private var fixture: SourceHealthHTTPFixture?
    private var stopped = false
    private let stopLock = NSLock()
    static func register(_ fixture: SourceHealthHTTPFixture) { lock.withLock { fixtures[fixture.host] = fixture } }
    static func remove(_ host: String) { lock.withLock { _ = fixtures.removeValue(forKey: host) } }
    override class func canInit(with request: URLRequest) -> Bool { request.url?.host?.hasSuffix(".fixture.test") == true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let fixture = Self.lock.withLock({ Self.fixtures[request.url?.host ?? ""] }) else { return }
        self.fixture = fixture
        var reply = fixture.reply(for: request)
        // A concrete MIME type prevents Foundation from waiting for a sniffing
        // prefix before forwarding headers from a deliberately unfinished body.
        if reply.headers["Content-Type"] == nil {
            reply.headers["Content-Type"] = "application/octet-stream"
        }
        if reply.sendsResponse {
            client?.urlProtocol(self, didReceive: HTTPURLResponse(url: reply.finalURL ?? request.url!, statusCode: reply.status, httpVersion: nil, headerFields: reply.headers)!, cacheStoragePolicy: .notAllowed)
        }
        if !reply.body.isEmpty { client?.urlProtocol(self, didLoad: reply.body) }
        if reply.finishes { client?.urlProtocolDidFinishLoading(self) }
    }
    override func stopLoading() {
        let first = stopLock.withLock { () -> Bool in
            guard !stopped else { return false }; stopped = true; return true
        }
        if first { fixture?.onStop(request.url?.path ?? "") }
    }
}
