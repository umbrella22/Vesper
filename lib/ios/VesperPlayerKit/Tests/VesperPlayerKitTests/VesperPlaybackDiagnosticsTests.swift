import AVFoundation
import XCTest
@testable import VesperPlayerKit

final class VesperPlaybackDiagnosticsTests: XCTestCase {
    @MainActor
    func testSameURICommandsBeginFreshNativeAttempts() async throws {
        var tokens: [VesperPlaybackObservationToken] = []
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { bridge, _, _, _ in
            let token = bridge.diagnosticsTracker.capture()
            tokens.append(token)
            XCTAssertNil(bridge.diagnosticsTracker.snapshot.firstFrame)
            bridge.diagnosticsTracker.firstFrame(token, mediaPositionMs: 1000)
        })
        defer { bridge.dispose() }
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://example.invalid/video.mp4")!, label: "Same")
        try await bridge.selectSourceAsync(source)
        try await bridge.selectSourceAsync(source)
        XCTAssertEqual(tokens.count, 2)
        XCTAssertGreaterThan(tokens[1].epoch, tokens[0].epoch)
        XCTAssertFalse(bridge.diagnosticsTracker.firstFrame(tokens[0], mediaPositionMs: 2000))
        XCTAssertEqual(bridge.diagnosticsTracker.snapshot.firstFrame?.playbackEpoch, tokens[1].epoch)
    }

    @MainActor
    func testSourceSelectionFromDiagnosticsSubscriberWinsOverOuterCommand() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        let sourceA = VesperPlayerSource.remoteUrl(URL(string: "https://example.invalid/video.mp4")!, label: "A")
        let sourceB = VesperPlayerSource.remoteUrl(URL(string: "https://example.invalid/video.mp4")!, label: "B")
        var replaceOnce = true
        let subscription = bridge.diagnosticsTracker.publisher.dropFirst().sink { _ in
            guard replaceOnce else { return }
            replaceOnce = false
            _ = bridge.startSourceSelection(sourceB)
        }
        let oldTask = bridge.startSourceSelection(sourceA)
        XCTAssertTrue(oldTask.isCancelled)
        XCTAssertEqual(bridge.currentSource, sourceB)
        XCTAssertEqual(bridge.activeSourceCommand?.source, sourceB)
        XCTAssertEqual(bridge.publishedUiState.sourceLabel, "B")
        XCTAssertFalse(bridge.sourceLoadTask?.isCancelled ?? true)
        subscription.cancel()
    }

    @MainActor
    func testNewSourceStateNeverCarriesPriorFirstFrame() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://example.invalid/video.mp4")!, label: "Replacement")
        let old = bridge.diagnosticsTracker.beginAttempt()
        bridge.diagnosticsTracker.firstFrame(old, mediaPositionMs: 1000)
        var checkedNewState = false
        let subscription = bridge.$publishedUiState.sink { value in
            if value.sourceLabel == source.label {
                checkedNewState = true
                XCTAssertNil(bridge.diagnosticsTracker.snapshot.firstFrame)
                XCTAssertNotEqual(bridge.diagnosticsTracker.snapshot.playbackEpoch, old.epoch)
            }
        }
        _ = bridge.startSourceSelection(source)
        XCTAssertTrue(checkedNewState)
        subscription.cancel()
    }

    func testFutureNativeEvidenceSurvivesErrorSerialization() throws {
        let json = """
        {"playbackEpoch":1,"audio":{"evidence":"futureAudio","lastIssue":{"kind":"futureIssue","elapsedSinceLoadStartMs":50}},"firstFrame":{"playbackEpoch":1,"elapsedSinceLoadStartMs":100,"kind":"futureFrame"}}
        """
        let value = try JSONDecoder().decode(VesperPlaybackDiagnosticsSnapshot.self, from: Data(json.utf8))
        let error = VesperPlayerError(message: "failure", code: .backendFailure, category: .playback, retriable: false, details: value.errorDetails)
        XCTAssertEqual(error.playbackDiagnostics?.audio.evidence.rawValue, "futureAudio")
        XCTAssertEqual(error.playbackDiagnostics?.audio.lastIssue?.kind.rawValue, "futureIssue")
        XCTAssertEqual(error.playbackDiagnostics?.firstFrame?.kind.rawValue, "futureFrame")
    }
    @MainActor
    func testMonotonicStartupDurationIsIndependentOfPlaybackPosition() {
        var now: UInt64 = 100
        let tracker = VesperPlaybackDiagnosticsTracker(nowMs: { now })
        let token = tracker.beginAttempt()
        now = 225
        XCTAssertTrue(tracker.firstFrame(token, mediaPositionMs: 90_000))
        XCTAssertEqual(tracker.snapshot.firstFrame?.elapsedSinceLoadStartMs, 125)
        XCTAssertEqual(tracker.snapshot.firstFrame?.mediaPositionMs, 90_000)
        XCTAssertEqual(tracker.snapshot.firstFrame?.kind, .avPlayerLayerReadyForDisplay)
        XCTAssertFalse(tracker.firstFrame(token, mediaPositionMs: 90_100))
    }

    @MainActor
    func testReplacementAndDisposalRejectOldAndForeignObservations() {
        let tracker = VesperPlaybackDiagnosticsTracker(nowMs: { 100 })
        let old = tracker.beginAttempt()
        tracker.invalidate()
        XCTAssertFalse(tracker.firstFrame(old, mediaPositionMs: 0))
        let replacement = tracker.beginAttempt()
        var audio = VesperAudioPlaybackDiagnostics()
        audio.trackId = "old"
        tracker.audio(old, value: audio)
        XCTAssertNil(tracker.snapshot.audio.trackId)
        let foreign = VesperPlaybackDiagnosticsTracker(nowMs: { 100 }).beginAttempt()
        XCTAssertFalse(tracker.firstFrame(foreign, mediaPositionMs: 0))
        XCTAssertTrue(tracker.firstFrame(replacement, mediaPositionMs: 0))
        tracker.dispose()
        tracker.audio(replacement, value: audio)
        XCTAssertNil(tracker.snapshot.audio.trackId)
    }

    @MainActor
    func testQueuedSurfaceCallbackCannotConfirmReplacementLoad() async {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        let firstItem = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let firstPlayer = AVPlayer(playerItem: firstItem)
        bridge.player = firstPlayer
        bridge.activePlayerObservationToken = bridge.diagnosticsTracker.beginAttempt()
        let host = PlayerSurfaceView()
        bridge.attachSurfaceHost(host)
        host.onReadyForDisplay?(firstPlayer, firstItem)
        bridge.diagnosticsTracker.invalidate()
        let nextItem = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let nextPlayer = AVPlayer(playerItem: nextItem)
        bridge.player = nextPlayer
        bridge.activePlayerObservationToken = bridge.diagnosticsTracker.beginAttempt()
        await Task.yield()
        XCTAssertNil(bridge.diagnosticsTracker.snapshot.firstFrame)
        host.onReadyForDisplay?(nextPlayer, nextItem)
        await Task.yield()
        XCTAssertNotNil(bridge.diagnosticsTracker.snapshot.firstFrame)
    }

    @MainActor
    func testSynchronousReplacementFromPublisherDoesNotRestoreOldSnapshot() {
        let tracker = VesperPlaybackDiagnosticsTracker(nowMs: { 100 })
        let token = tracker.beginAttempt()
        let subscription = tracker.publisher.sink { value in
            if value.firstFrame != nil { tracker.invalidate() }
        }
        let secondSubscription = tracker.publisher.sink { value in
            XCTAssertEqual(value, tracker.snapshot, "Reentry must not replay superseded evidence to other subscribers")
        }
        XCTAssertTrue(tracker.firstFrame(token, mediaPositionMs: nil))
        XCTAssertFalse(tracker.isCurrent(token))
        XCTAssertNil(tracker.snapshot.firstFrame)
        subscription.cancel()
        secondSubscription.cancel()
    }

    @MainActor
    func testGenericPlaybackFailureRetainsUnknownAudioWithoutReclassification() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        bridge.diagnosticsTracker.beginAttempt()
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            video: .auto(), audio: .track("requested-but-unconfirmed"), subtitle: .disabled()
        )
        bridge.activePlayerObservationToken = bridge.diagnosticsTracker.capture()
        bridge.refreshAudioDiagnostics()
        let error = bridge.resolvedPlaybackFailure(error: nil, fallbackMessage: "failed").toPlayerError()
        XCTAssertNotEqual(error.category, .audioOutput)
        XCTAssertNil(error.playbackDiagnostics?.audio.trackId)
        XCTAssertNil(error.playbackDiagnostics?.audio.decoderName)
        XCTAssertEqual(error.playbackDiagnostics?.audio.evidence, .unknown)
        XCTAssertEqual(error.playbackDiagnostics?.playbackEpoch, bridge.diagnosticsTracker.snapshot.playbackEpoch)
    }

    @MainActor
    func testNativeControllerRetainsObservationForLateSubscribers() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        let token = bridge.diagnosticsTracker.beginAttempt()
        bridge.diagnosticsTracker.firstFrame(token, mediaPositionMs: 1000)
        var received: VesperPlaybackDiagnosticsSnapshot?
        let subscription = controller.playbackDiagnosticsPublisher.sink { received = $0 }
        XCTAssertEqual(received, controller.playbackDiagnostics)
        XCTAssertEqual(received?.firstFrame?.mediaPositionMs, 1000)
        subscription.cancel()
    }
}
