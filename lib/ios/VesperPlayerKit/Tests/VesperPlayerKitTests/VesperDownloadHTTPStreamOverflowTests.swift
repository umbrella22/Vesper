import Foundation
import XCTest
@testable import VesperPlayerKit

/// Regression tests for the download HTTP stream back-pressure boundary.
///
/// `VesperURLSessionDataStreamDelegate` carries a lossless byte payload via an
/// `AsyncThrowingStream` with `bufferingNewest(256)`. If the consumer falls
/// behind and a chunk is dropped, the downloaded file would be silently
/// corrupted (a hole in the middle of the byte stream). These tests verify that
/// a dropped chunk is detected and surfaces as a failure instead of silent
/// corruption.
final class VesperDownloadHTTPStreamOverflowTests: XCTestCase {
    /// When the consumer is slow enough to overflow the 256-chunk buffer, the
    /// next yielded chunk is reported as `.dropped` and the delegate must finish
    /// the stream with an error rather than losing the byte silently.
    func testDroppedChunkSurfacesAsStreamFailure() async {
        let delegate = VesperURLSessionDataStreamDelegate(
            stalledTransferTimeoutMs: 0,
            sourceDescription: "test://overflow"
        )
        // A stand-in task is required by the delegate signature but the method
        // never inspects it; the session is only used for invalidation.
        let session = URLSession(configuration: .ephemeral)
        let task = session.dataTask(with: URL(string: "https://example.com")!)

        // Fill the 256-slot buffer without draining. The 257th yield overflows
        // `bufferingNewest(256)`, so the oldest chunk is dropped.
        let chunk = Data(repeating: 0x41, count: 1)
        for _ in 0..<256 {
            delegate.urlSession(session, dataTask: task, didReceive: chunk)
        }
        // This 257th deliver triggers the dropped-detection path.
        delegate.urlSession(session, dataTask: task, didReceive: chunk)

        // The stream must terminate with an error, not deliver a truncated
        // payload. Drain whatever the consumer would see.
        do {
            for try await _ in delegate.chunks {
                // Discard; we only care that iteration ends with a throw.
            }
            XCTFail("Stream completed normally; expected an overflow failure after a dropped chunk")
        } catch {
            let message = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            XCTAssertTrue(
                message.contains("overflowed"),
                "Expected overflow error message, got: \(message)"
            )
        }

        task.cancel()
        session.invalidateAndCancel()
    }

    /// When the consumer keeps up, all chunks are delivered in order and the
    /// stream completes normally. Guards against an overly-aggressive failure
    /// path that would reject valid throughput.
    func testSteadyDeliveryDoesNotOverflow() async throws {
        let delegate = VesperURLSessionDataStreamDelegate(
            stalledTransferTimeoutMs: 0,
            sourceDescription: "test://steady"
        )
        let session = URLSession(configuration: .ephemeral)
        let task = session.dataTask(with: URL(string: "https://example.com")!)

        // Produce and consume concurrently so the buffer never fills.
        async let consumed: [Data] = consume(delegate.chunks, expected: 32)
        for index in 0..<32 {
            delegate.urlSession(session, dataTask: task, didReceive: Data([UInt8(index)]))
        }
        // Signal end-of-stream by simulating a clean task completion.
        delegate.urlSession(session, task: task, didCompleteWithError: nil)

        let received = try await consumed
        XCTAssertEqual(received.count, 32)
        XCTAssertEqual(received.enumerated().map { $0.element.first }, Array(0..<32).map { UInt8($0) })

        session.invalidateAndCancel()
    }

    private func consume(
        _ stream: AsyncThrowingStream<Data, Error>,
        expected: Int
    ) async throws -> [Data] {
        var collected: [Data] = []
        collected.reserveCapacity(expected)
        for try await data in stream {
            collected.append(data)
        }
        return collected
    }
}
