import AVFoundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class PlayerErrorStateTests: XCTestCase {
    func testNativeBridgeReportsUnsupportedVideoTrackSelection() {
        let bridge = VesperNativePlayerBridge()
        let missingTrackId = "video:missing"

        bridge.setVideoTrackSelection(.track(missingTrackId))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setVideoTrackSelection is not implemented on iOS AVPlayer (mode=track, trackId=\(missingTrackId))"
        )
    }

    func testNativeBridgeReportsUnsupportedFixedTrackAbrWithoutCurrentCatalog() {
        let bridge = VesperNativePlayerBridge()
        let missingTrackId = "video:missing"

        bridge.setAbrPolicy(.fixedTrack(missingTrackId))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy fixedTrack requires a video variant from the current iOS track catalog (trackId=\(missingTrackId))"
        )
    }

    func testNativeBridgeReportsUnsupportedSingleAxisConstrainedAbrWithoutCurrentCatalog() {
        let bridge = VesperNativePlayerBridge()

        bridge.setAbrPolicy(.constrained(maxHeight: 720))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy constrained mode requires a loaded HLS variant catalog to infer a single-axis maxWidth/maxHeight limit on iOS"
        )
    }

    func testSelectingUnsupportedSourceClearsPreviousSurfaceAttachmentAndTrackState() throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: tempUrl, label: "Local"))
        let surface = PlayerSurfaceView(frame: .zero)
        bridge.attachSurfaceHost(surface)
        bridge.initialize()

        XCTAssertNotNil(attachedPlayer(in: surface))

        bridge.selectSource(
            .dash(
                url: URL(string: "https://example.com/playlist.mpd")!,
                label: "Unsupported DASH"
            )
        )

        XCTAssertNil(attachedPlayer(in: surface))
        XCTAssertEqual(bridge.trackCatalog, .empty)
        XCTAssertEqual(bridge.trackSelection, VesperTrackSelectionSnapshot())
        XCTAssertNil(bridge.effectiveVideoTrackId)
        XCTAssertNil(bridge.videoVariantObservation)
        XCTAssertNil(bridge.fixedTrackStatus)
        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.message, VesperPlayerI18n.dashUnsupportedOnIos)
    }

    func testStaleStopSeekCompletionDoesNotOverwriteNewSourceStopSeekState() throws {
        let firstUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        let secondUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: firstUrl.path, contents: Data(), attributes: nil)
        FileManager.default.createFile(atPath: secondUrl.path, contents: Data(), attributes: nil)
        defer {
            try? FileManager.default.removeItem(at: firstUrl)
            try? FileManager.default.removeItem(at: secondUrl)
        }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: firstUrl, label: "First"))
        bridge.initialize()
        let staleEpoch = bridge.playbackEpochSnapshot()

        bridge.selectSource(.localFile(url: secondUrl, label: "Second"))
        bridge.stop()
        bridge.play()

        XCTAssertEqual(bridge.uiState.sourceLabel, "Second")
        XCTAssertEqual(
            bridge.stopSeekStateSnapshot(),
            StopSeekStateSnapshot(
                isSeekingToStartAfterStop: true,
                pendingPlayAfterStopSeek: true
            )
        )

        bridge.handleStopSeekCompletion(playbackEpoch: staleEpoch)

        XCTAssertEqual(bridge.uiState.sourceLabel, "Second")
        XCTAssertEqual(
            bridge.stopSeekStateSnapshot(),
            StopSeekStateSnapshot(
                isSeekingToStartAfterStop: true,
                pendingPlayAfterStopSeek: true
            )
        )
        XCTAssertNil(bridge.lastError)
    }

    func testStaleRetryTaskDoesNotReinitializeSameUriAfterPolicyReinit() throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: tempUrl, label: "Local"))
        bridge.initialize()
        let staleEpoch = bridge.playbackEpochSnapshot()

        bridge.setResiliencePolicy(.resilient())
        let currentEpoch = bridge.playbackEpochSnapshot()
        XCTAssertNotEqual(currentEpoch, staleEpoch)

        bridge.handleScheduledRetryFire(
            expectedUri: tempUrl.absoluteString,
            playbackEpoch: staleEpoch,
            attempt: 1,
            delayMs: 500
        )

        XCTAssertEqual(bridge.playbackEpochSnapshot(), currentEpoch)
        XCTAssertEqual(bridge.uiState.sourceLabel, "Local")
        XCTAssertNil(bridge.lastError)
    }

    func testStaleRetryTaskAfterDisposeDoesNotReinitializeBridge() throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: tempUrl, label: "Local"))
        bridge.initialize()
        let staleEpoch = bridge.playbackEpochSnapshot()

        bridge.dispose()
        let disposedEpoch = bridge.playbackEpochSnapshot()

        bridge.handleScheduledRetryFire(
            expectedUri: tempUrl.absoluteString,
            playbackEpoch: staleEpoch,
            attempt: 1,
            delayMs: 500
        )

        XCTAssertEqual(bridge.playbackEpochSnapshot(), disposedEpoch)
        XCTAssertEqual(bridge.uiState.sourceLabel, "Local")
        XCTAssertNil(bridge.lastError)
    }
}

@MainActor
private func attachedPlayer(in surface: PlayerSurfaceView) -> AVPlayer? {
    surface.layer.sublayers?
        .compactMap { $0 as? AVPlayerLayer }
        .first?
        .player
}
