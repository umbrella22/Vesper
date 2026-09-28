import AVFoundation
import XCTest
@testable import VesperPlayerKit

final class VesperPlaybackStallTests: XCTestCase {
    func testRequiresProgressAndReportsOnlyOncePerAttempt() {
        var detector = VesperPlaybackStallDetector()
        for time in stride(from: UInt64(0), through: 10_000, by: 1_000) {
            XCTAssertNil(detector.sample(nowMs: time, positionMs: 0, eligible: true, buffering: false))
        }
        XCTAssertNil(detector.sample(nowMs: 11_000, positionMs: 100, eligible: true, buffering: false))
        for time in stride(from: UInt64(12_000), through: 15_000, by: 1_000) {
            XCTAssertNil(detector.sample(nowMs: time, positionMs: 100, eligible: true, buffering: false))
        }
        let stall = detector.sample(nowMs: 16_000, positionMs: 100, eligible: true, buffering: false)
        XCTAssertEqual(stall?.kind, .positionNotAdvancing)
        XCTAssertEqual(stall?.durationMs, 5_000)
        detector.resetWindow()
        for time in stride(from: UInt64(17_000), through: 28_000, by: 1_000) {
            XCTAssertNil(detector.sample(nowMs: time, positionMs: time == 17_000 ? 100 : 200, eligible: true, buffering: false))
        }
    }

    func testPauseSeekSuspensionAndBufferingUseFreshWindows() {
        var detector = VesperPlaybackStallDetector()
        detector.policy = .init(positionThresholdMs: 2_000, bufferingThresholdMs: 4_000)
        _ = detector.sample(nowMs: 0, positionMs: 0, eligible: true, buffering: false)
        _ = detector.sample(nowMs: 1_000, positionMs: 100, eligible: true, buffering: false)
        detector.resetWindow()
        for time in stride(from: UInt64(2_000), through: 8_000, by: 1_000) {
            XCTAssertNil(detector.sample(nowMs: time, positionMs: 100, eligible: true, buffering: false))
        }
        _ = detector.sample(nowMs: 9_000, positionMs: 200, eligible: true, buffering: false)
        XCTAssertNil(detector.sample(nowMs: 15_000, positionMs: 200, eligible: true, buffering: false))
        XCTAssertNil(detector.sample(nowMs: 16_000, positionMs: 200, eligible: false, buffering: false))
        _ = detector.sample(nowMs: 17_000, positionMs: 200, eligible: true, buffering: false)
        _ = detector.sample(nowMs: 18_000, positionMs: 300, eligible: true, buffering: false)
        for time in stride(from: UInt64(19_000), through: 22_000, by: 1_000) {
            XCTAssertNil(detector.sample(nowMs: time, positionMs: 300, eligible: true, buffering: true))
        }
        XCTAssertEqual(detector.sample(nowMs: 23_000, positionMs: 300, eligible: true, buffering: true)?.kind, .bufferingTimeout)
    }

    @MainActor func testRetainedStallRejectsOldTicksAndDisposal() {
        var now: UInt64 = 0
        let tracker = VesperPlaybackDiagnosticsTracker(nowMs: { now })
        let token = tracker.beginAttempt()
        var audio = VesperAudioPlaybackDiagnostics()
        audio.codec = "ec-3"
        tracker.audio(token, value: audio)
        tracker.sampleStall(token, positionMs: 0, eligible: true, buffering: false)
        now = 1_000
        tracker.sampleStall(token, positionMs: 100, eligible: true, buffering: false)
        for time in stride(from: UInt64(2_000), through: 6_000, by: 1_000) {
            now = time
            tracker.sampleStall(token, positionMs: 100, eligible: true, buffering: false)
        }
        XCTAssertEqual(tracker.snapshot.lastStall?.audio.codec, "ec-3")
        let current = tracker.beginAttempt()
        tracker.sampleStall(token, positionMs: 100, eligible: true, buffering: false)
        XCTAssertNil(tracker.snapshot.lastStall)
        tracker.dispose()
        tracker.sampleStall(current, positionMs: 100, eligible: true, buffering: false)
        XCTAssertNil(tracker.snapshot.lastStall)
    }

    @MainActor func testLegacySeekCompletionCannotClearReplacementSeekAndTeardownCancelsTimer() async {
        var completions: [@Sendable (Bool) -> Void] = []
        let bridge = VesperNativePlayerBridge(systemPlayerSeekSubmitter: { _, _, _, _, completion in
            completions.append(completion)
        })
        defer { bridge.dispose() }
        let item = AVPlayerItem(url: URL(string: "https://example.invalid/video.mp4")!)
        let player = AVPlayer(playerItem: item)
        bridge.player = player
        bridge.activePlayerObservationToken = bridge.diagnosticsTracker.beginAttempt()
        bridge.installPlaybackStallObserver(player: player, item: item)
        XCTAssertNotNil(bridge.playbackStallTask)
        bridge.seekToPosition(100)
        let old = bridge.activeLegacySeekId
        bridge.seekToPosition(200)
        let current = bridge.activeLegacySeekId
        XCTAssertNotEqual(old, current)
        completions[0](true)
        await Task.yield()
        XCTAssertEqual(bridge.activeLegacySeekId, current)
        completions[1](false)
        await Task.yield()
        XCTAssertNil(bridge.activeLegacySeekId)
        bridge.seekToPosition(300)
        bridge.tearDownActivePlayback()
        XCTAssertNil(bridge.activeLegacySeekId)
        XCTAssertNil(bridge.playbackStallTask)
        completions[2](true)
        await Task.yield()
        XCTAssertNil(bridge.activeLegacySeekId)
    }

    @MainActor func testStallPublicationReentryInvalidatesOuterSnapshot() {
        var now: UInt64 = 0
        let tracker = VesperPlaybackDiagnosticsTracker(nowMs: { now })
        let old = tracker.beginAttempt()
        let subscription = tracker.publisher.sink { value in
            if value.lastStall != nil { tracker.beginAttempt() }
        }
        defer { subscription.cancel() }
        tracker.sampleStall(old, positionMs: 0, eligible: true, buffering: false)
        for time in stride(from: UInt64(1_000), through: 6_000, by: 1_000) {
            now = time
            tracker.sampleStall(old, positionMs: 100, eligible: true, buffering: false)
        }
        XCTAssertFalse(tracker.isCurrent(old))
        XCTAssertNil(tracker.snapshot.lastStall)
    }

    @MainActor func testAVPlayerProbeDoesNotAssertAudioSupport() throws {
        let request = VesperAudioDecoderCapabilityRequest(codec: "mp4a.40.2", channels: 2, sampleRate: 48_000)
        let result = try VesperPlayerControllerFactory.probeAudioDecoderCapability(request)
        XCTAssertEqual(result.request, request)
        XCTAssertEqual(result.status, .unknown)
        XCTAssertEqual(result.reason, "avPlayerAudioDecoderQueryUnavailable")
        XCTAssertThrowsError(try VesperPlayerControllerFactory.probeAudioDecoderCapability(.init(channels: 0)))
    }
}
