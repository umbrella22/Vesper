import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperGeneratedDownloadSizeOverflowTests: XCTestCase {
    @MainActor
    func testResourceOverflowReportsPreparationFailureWithoutTrapping() async throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, resources: [
            generatedManifest(),
            VesperDownloadResourceRecord(resourceId: "media", uri: "https://example.invalid/media", sizeBytes: .max)
        ])
        try await assertOverflow(index, directory: directory)
    }

    @MainActor
    func testSegmentOverflowReportsPreparationFailureWithoutTrapping() async throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, resources: [generatedManifest()], segments: [
            VesperDownloadSegmentRecord(segmentId: "segment", uri: "https://example.invalid/segment", sizeBytes: .max)
        ])
        try await assertOverflow(index, directory: directory)
    }

    @MainActor
    func testGeneratedResourcesPreserveStreamsAndRecomputeRepresentableTotal() async throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let resource = VesperDownloadResourceRecord(resourceId: "media", uri: "https://example.invalid/media", sizeBytes: 2)
        let segment = VesperDownloadSegmentRecord(segmentId: "segment", uri: "https://example.invalid/segment", sizeBytes: 3)
        let stream = VesperDownloadAssetStream(streamId: "video", kind: .video, resourceIds: ["manifest", "media"], segmentIds: ["segment"])
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, version: "v1", etag: "etag", checksum: "checksum",
            totalSizeBytes: 99, resources: [generatedManifest(), resource], segments: [segment], streams: [stream], completedPath: "completed")
        let reporter = await prepare(index, directory: directory)
        XCTAssertNil(reporter.failure)
        let prepared = try XCTUnwrap(reporter.preparedIndex)
        XCTAssertEqual(prepared.totalSizeBytes, 6)
        XCTAssertEqual(prepared.resources[1], resource)
        XCTAssertEqual(prepared.segments, [segment])
        XCTAssertEqual(prepared.streams, [stream])
        XCTAssertEqual(prepared.version, index.version)
        XCTAssertEqual(prepared.etag, index.etag)
        XCTAssertEqual(prepared.checksum, index.checksum)
        XCTAssertEqual(prepared.completedPath, index.completedPath)
        let manifest = prepared.resources[0]
        XCTAssertNil(manifest.generatedText)
        XCTAssertEqual(manifest.sizeBytes, 1)
        let url = try XCTUnwrap(URL(string: manifest.uri))
        XCTAssertTrue(url.isFileURL)
        XCTAssertEqual(try Data(contentsOf: url), Data("m".utf8))
        XCTAssertEqual(reporter.preparationCount, 1)
        XCTAssertEqual(reporter.failureCount, 0)
    }

    @MainActor
    func testRepresentableUInt64MaxResourceTotalRemainsAccepted() async throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, resources: [generatedManifest(),
            VesperDownloadResourceRecord(resourceId: "media", uri: "https://example.invalid/media", sizeBytes: UInt64.max - 1)
        ])
        let reporter = await prepare(index, directory: directory)
        XCTAssertNil(reporter.failure)
        XCTAssertEqual(try XCTUnwrap(reporter.preparedIndex).totalSizeBytes, UInt64.max)
    }

    @MainActor
    func testRepresentableUInt64MaxSegmentTotalRemainsAccepted() async throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, resources: [generatedManifest()], segments: [
            VesperDownloadSegmentRecord(segmentId: "segment", uri: "https://example.invalid/segment", sizeBytes: UInt64.max - 1)
        ])
        let reporter = await prepare(index, directory: directory)
        XCTAssertNil(reporter.failure)
        XCTAssertEqual(try XCTUnwrap(reporter.preparedIndex).totalSizeBytes, UInt64.max)
    }

    func testUnknownResourceSizeRetainsOriginalFallback() throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        for original in [nil, UInt64(77)] {
            let index = VesperDownloadAssetIndex(totalSizeBytes: original, resources: [generatedManifest(),
                VesperDownloadResourceRecord(resourceId: "media", uri: "https://example.invalid/media", sizeBytes: .max),
                VesperDownloadResourceRecord(resourceId: "unknown", uri: "https://example.invalid/unknown")
            ])
            let prepared = try materialize(index, directory: directory)
            XCTAssertEqual(prepared.totalSizeBytes, original)
            XCTAssertNil(prepared.resources[2].sizeBytes)
        }
    }

    func testUnknownSegmentSizeRetainsOriginalFallback() throws {
        let directory = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        for original in [nil, UInt64(77)] {
            let index = VesperDownloadAssetIndex(totalSizeBytes: original, resources: [generatedManifest(),
                VesperDownloadResourceRecord(resourceId: "media", uri: "https://example.invalid/media", sizeBytes: .max)
            ], segments: [VesperDownloadSegmentRecord(segmentId: "unknown", uri: "https://example.invalid/unknown")])
            let prepared = try materialize(index, directory: directory)
            XCTAssertEqual(prepared.totalSizeBytes, original)
            XCTAssertNil(prepared.segments[0].sizeBytes)
        }
    }

    private func generatedManifest() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(resourceId: "manifest", uri: "vesper-generated://index.m3u8", relativePath: "index.m3u8", generatedText: "m")
    }

    private func temporaryDirectory() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    }

    private func materialize(_ index: VesperDownloadAssetIndex, directory: URL) throws -> VesperDownloadAssetIndex {
        try VesperGeneratedDownloadResourceMaterializer(baseDirectory: directory).materialize(
            assetId: "generated-size", taskId: 1, profile: VesperDownloadProfile(), assetIndex: index)
    }

    @MainActor
    private func assertOverflow(_ index: VesperDownloadAssetIndex, directory: URL) async throws {
        let reporter = await prepare(index, directory: directory)
        XCTAssertNil(reporter.preparedIndex)
        XCTAssertEqual(reporter.preparationCount, 0)
        XCTAssertEqual(reporter.failureCount, 1)
        let failure = try XCTUnwrap(reporter.failure)
        XCTAssertEqual(failure.code, .backendFailure)
        XCTAssertEqual(failure.category, .network)
        XCTAssertFalse(failure.retriable)
        XCTAssertEqual(failure.message, "download generated byte total overflows")
        XCTAssertThrowsError(try materialize(index, directory: directory)) { error in
            guard let preparationError = error as? VesperForegroundDownloadPreparationError,
                  case let .invalidSource(message) = preparationError else {
                return XCTFail("expected the existing typed invalidSource error, got \(error)")
            }
            XCTAssertEqual(message, failure.message)
        }
    }

    @MainActor
    private func prepare(_ index: VesperDownloadAssetIndex, directory: URL) async -> GeneratedSizePreparationReporter {
        let terminal = expectation(description: "public preparation reports one terminal outcome")
        let reporter = GeneratedSizePreparationReporter(terminal: terminal)
        let executor = VesperForegroundDownloadExecutor(baseDirectory: directory)
        defer { executor.dispose() }
        let task = VesperDownloadTaskSnapshot(taskId: 1, assetId: "generated-size",
            source: VesperDownloadSource(source: .hls(url: URL(string: "https://example.invalid/index.m3u8")!)),
            profile: VesperDownloadProfile(), state: .preparing, progress: VesperDownloadProgressSnapshot(), assetIndex: index)
        executor.prepare(task: task, reporter: reporter)
        await fulfillment(of: [terminal], timeout: 3)
        return reporter
    }
}

@MainActor
private final class GeneratedSizePreparationReporter: VesperDownloadExecutionReporter {
    let terminal: XCTestExpectation
    var preparedIndex: VesperDownloadAssetIndex?
    var failure: VesperDownloadError?
    var preparationCount = 0
    var failureCount = 0

    init(terminal: XCTestExpectation) { self.terminal = terminal }

    func completePreparation(taskId: VesperDownloadTaskId, assetIndex: VesperDownloadAssetIndex) {
        preparedIndex = assetIndex
        preparationCount += 1
        terminal.fulfill()
    }

    func fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
        failure = error
        failureCount += 1
        terminal.fulfill()
    }

    func updateProgress(taskId: VesperDownloadTaskId, receivedBytes: UInt64, receivedSegments: UInt32) {}
    func complete(taskId: VesperDownloadTaskId, completedPath: String?) { XCTFail("prepare must not report download completion") }
}
