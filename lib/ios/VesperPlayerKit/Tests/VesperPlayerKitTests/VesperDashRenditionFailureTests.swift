import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperDashRenditionFailureTests: XCTestCase {
    func testFailedRenditionLookupPropagatesOriginalFailureAndSnapshotDoesNotRetry() async {
        let url = URL(string: "https://fixture.test/failed.mpd")!
        let client = CountingDashNetworkClient(dataByURL: [:])
        let session = VesperDashSession(sourceURL: url, networkClient: client,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider)
        do {
            _ = try await session.isSubtitleRendition(renditionId: "sub-en")
            XCTFail("A failed manifest lookup must not become a non-subtitle decision")
        } catch let error as VesperDashBridgeError {
            guard case .network = error else { return XCTFail("Unexpected failure: \(error)") }
        } catch { XCTFail("Unexpected failure: \(error)") }
        XCTAssertEqual(client.requestCount(for: url), 1)
        let subtitle = await session.isSubtitleRenditionSnapshot(renditionId: "sub-en")
        XCTAssertFalse(subtitle)
        XCTAssertEqual(client.requestCount(for: url), 1, "Error classification must not restart manifest loading")
    }

    func testCancelledRenditionLookupDoesNotStartManifestRequest() async {
        let url = URL(string: "https://fixture.test/cancelled.mpd")!
        let client = CountingDashNetworkClient(dataByURL: [:])
        let session = VesperDashSession(sourceURL: url, networkClient: client)
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            do {
                _ = try await session.isSubtitleRendition(renditionId: "sub-en")
                XCTFail("Cancellation must propagate")
            } catch is CancellationError {
                // Expected control flow.
            } catch { XCTFail("Unexpected failure: \(error)") }
            _ = await session.isSubtitleRenditionSnapshot(renditionId: "sub-en")
        }
        await task.value
        XCTAssertEqual(client.requestCount(for: url), 0)
    }

    func testKnownSubtitleSnapshotDoesNotStartAnotherManifestRequest() async throws {
        let url = URL(string: "https://fixture.test/subtitles.mpd")!
        let client = CountingDashNetworkClient(dataByURL: [url: Data(sampleWebVttSubtitleMpd.utf8)])
        let session = VesperDashSession(sourceURL: url, networkClient: client,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider)
        let manifest = try await session.loadManifest()
        let selected = try await session.selectedPlayableRepresentations(manifest: manifest, variantPolicy: .all)
        let rendition = try XCTUnwrap(selected.subtitles.first?.renditionId)
        let before = client.requestCount(for: url)
        let subtitle = await session.isSubtitleRenditionSnapshot(renditionId: rendition)
        XCTAssertTrue(subtitle)
        let unknown = await session.isSubtitleRenditionSnapshot(renditionId: "missing")
        XCTAssertFalse(unknown)
        XCTAssertEqual(client.requestCount(for: url), before)
    }
}
