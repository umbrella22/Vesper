import AVFoundation
import XCTest
@testable import VesperPlayerKit

final class VesperLocalResourceIOTests: XCTestCase {
    func testDataBodyReadsRequestedOffsetAndLength() throws {
        let body = VesperLocalResourceBody.data(
            Data("0123456789".utf8),
            contentType: "public.data"
        )

        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 2,
            requestedLength: 4
        )

        XCTAssertEqual(String(data: data, encoding: .utf8), "2345")
    }

    func testDataBodyUsesCurrentOffsetWhenPresent() throws {
        let body = VesperLocalResourceBody.data(
            Data("0123456789".utf8),
            contentType: "public.data"
        )

        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 0,
            currentOffset: 5,
            requestedLength: 2
        )

        XCTAssertEqual(String(data: data, encoding: .utf8), "56")
    }

    func testOffsetOutOfRangeCanFinishEmpty() throws {
        let body = VesperLocalResourceBody.data(
            Data("0123".utf8),
            contentType: "public.data"
        )

        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 8,
            requestedLength: 4,
            readPolicy: VesperLocalResourceReadPolicy(outOfRangeBehavior: .finishEmpty)
        )

        XCTAssertTrue(data.isEmpty)
    }

    func testOffsetOutOfRangeFailsByDefault() throws {
        let body = VesperLocalResourceBody.data(
            Data("0123".utf8),
            contentType: "public.data"
        )

        XCTAssertThrowsError(
            try VesperLocalResourceResponder.readDataForTesting(
                body: body,
                requestedOffset: 8,
                requestedLength: 4
            )
        )
    }

    func testZeroLengthRequestReadsRemainingBytes() throws {
        let body = VesperLocalResourceBody.data(
            Data("012345".utf8),
            contentType: "public.data"
        )

        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 3,
            requestedLength: 0
        )

        XCTAssertEqual(String(data: data, encoding: .utf8), "345")
    }

    func testFileBodyReadsRequestedByteRange() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("resource.bin")
        try Data("0123456789".utf8).write(to: file)

        let body = VesperLocalResourceBody.file(
            url: file,
            offset: 2,
            length: 6,
            contentType: "public.data",
            removeAfterServing: false,
            growingPolicy: nil
        )
        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 1,
            requestedLength: 3,
            readPolicy: VesperLocalResourceReadPolicy(bufferBytes: 2)
        )

        XCTAssertEqual(String(data: data, encoding: .utf8), "345")
    }

    func testReadPolicyAllowsFourMiBProfileBuffer() {
        let policy = VesperLocalResourceReadPolicy(bufferBytes: 4 * 1024 * 1024)

        XCTAssertEqual(policy.bufferBytes, 4 * 1024 * 1024)
    }

    func testReadPolicyCapsOversizedBufferAtFourMiB() {
        let policy = VesperLocalResourceReadPolicy(bufferBytes: 16 * 1024 * 1024)

        XCTAssertEqual(policy.bufferBytes, 4 * 1024 * 1024)
    }

    func testTemporaryFileCleanupRemovesFile() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("temporary.bin")
        try Data("temporary".utf8).write(to: file)

        let body = VesperLocalResourceBody.file(
            url: file,
            offset: 0,
            length: 9,
            contentType: "public.data",
            removeAfterServing: true,
            growingPolicy: nil
        )

        body.cleanupIfNeeded()

        XCTAssertFalse(FileManager.default.fileExists(atPath: file.path))
    }

    func testGrowingFileWaitsUntilRequestedBytesAppear() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("growing.bin")
        try Data("0123".utf8).write(to: file)

        let body = VesperLocalResourceBody.file(
            url: file,
            offset: 0,
            length: 4,
            contentType: "public.data",
            removeAfterServing: false,
            growingPolicy: VesperGrowingFileReadPolicy(timeoutSeconds: 1, pollSeconds: 0.01)
        )
        let writerFinished = expectation(description: "growing writer finished")
        DispatchQueue.global(qos: .userInitiated).async {
            Thread.sleep(forTimeInterval: 0.05)
            do {
                var data = try Data(contentsOf: file)
                data.append(Data("4567".utf8))
                try data.write(to: file)
            } catch {
                XCTFail("Failed to append growing bytes: \(error)")
            }
            writerFinished.fulfill()
        }

        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 4,
            requestedLength: 4
        )
        wait(for: [writerFinished], timeout: 1)

        XCTAssertEqual(String(data: data, encoding: .utf8), "4567")
    }

    func testGrowingFileReadsRangeBeyondInitialContentLength() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("growing-range.bin")
        try Data("0123".utf8).write(to: file)

        let body = VesperLocalResourceBody.file(
            url: file,
            offset: 0,
            length: 4,
            contentType: "public.data",
            removeAfterServing: false,
            growingPolicy: VesperGrowingFileReadPolicy(timeoutSeconds: 1, pollSeconds: 0.01)
        )
        let writerFinished = expectation(description: "growing range writer finished")
        DispatchQueue.global(qos: .userInitiated).async {
            Thread.sleep(forTimeInterval: 0.05)
            do {
                var data = try Data(contentsOf: file)
                data.append(Data("456789".utf8))
                try data.write(to: file)
            } catch {
                XCTFail("Failed to append growing range bytes: \(error)")
            }
            writerFinished.fulfill()
        }

        let data = try VesperLocalResourceResponder.readDataForTesting(
            body: body,
            requestedOffset: 6,
            requestedLength: 4
        )
        wait(for: [writerFinished], timeout: 1)

        XCTAssertEqual(String(data: data, encoding: .utf8), "6789")
    }

    func testPathContainmentRejectsSiblingDirectoryPrefix() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let sibling = directory
            .deletingLastPathComponent()
            .appendingPathComponent(directory.lastPathComponent + "-sibling")
            .appendingPathComponent("file.bin")

        XCTAssertFalse(vesperLocalResourceIsContained(sibling, in: directory))
    }

    private func makeTemporaryDirectory() throws -> URL {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }
}
