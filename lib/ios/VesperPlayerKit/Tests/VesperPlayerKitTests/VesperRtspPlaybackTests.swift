import AVFoundation
import Foundation
import XCTest
@_spi(VesperFlutter) @testable import VesperPlayerKit

final class VesperRtspPlaybackTests: XCTestCase {
    @MainActor
    func testRtspUsesLiveNativeItemAndDisposesSession() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let input = environment["VESPER_RTSP_TEST_URI"], let url = URL(string: input) else {
            throw XCTSkip("Set VESPER_RTSP_TEST_URI to a controlled RTSP origin.")
        }
        let source = VesperPlayerSource.remoteUrl(url, label: "RTSP fixture", protocol: .rtsp)
        let bridge = VesperNativePlayerBridge(initialSource: source, systemPlayerIsMuted: true)
        let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
        defer { controller.dispose() }
        if environment["VESPER_RTSP_EXPECT_BUNDLED"] == "0" {
            do {
                try await controller.initializeAsync()
                XCTFail("An iOS build without the RTSP feature accepted the source.")
            } catch let error as VesperPlayerError {
                XCTAssertEqual(error.code, .unsupported)
                XCTAssertEqual(error.details["reason"], "rtsp_bridge_not_bundled")
            }
            XCTAssertNil(bridge.currentRtspSession)
            return
        }
        try await controller.initializeAsync()
        XCTAssertEqual(bridge.currentSource, source)
        XCTAssertEqual(bridge.currentTimelineKind(durationMs: nil, seekableRange: nil), .live)
        let player = try XCTUnwrap(bridge.player)
        let item = try XCTUnwrap(player.currentItem)
        let asset = try XCTUnwrap(item.asset as? AVURLAsset)
        XCTAssertEqual(asset.url.host, "127.0.0.1")
        XCTAssertEqual(asset.url.lastPathComponent, "index.m3u8")
        do {
            try await controller.seekAsync(by: 1_000)
            XCTFail("RTSP does not expose a seekable timeline.")
        } catch let error as VesperPlayerError {
            XCTAssertEqual(error.code, .unsupported)
            XCTAssertEqual(error.category, .capability)
            XCTAssertEqual(error.details["reason"], "rtsp_seek_unsupported")
        }
        do {
            try await controller.seekToLiveEdgeAsync()
            XCTFail("RTSP does not expose a seek-to-live-edge command.")
        } catch let error as VesperPlayerError {
            XCTAssertEqual(error.details["reason"], "rtsp_seek_unsupported")
        }
        bridge.seek(toRatio: 0.5)
        XCTAssertEqual(bridge.lastError?.details["reason"], "rtsp_seek_unsupported")
        XCTAssertNil(bridge.activeSeekCommand)
        player.play()
        let deadline = ContinuousClock().now + .seconds(12)
        let start = player.currentTime().seconds
        while player.currentTime().seconds - start < 1.5, ContinuousClock().now < deadline {
            try await Task.sleep(for: .milliseconds(100))
        }
        XCTAssertGreaterThanOrEqual(player.currentTime().seconds - start, 1.5)
        if environment["VESPER_RTSP_EXPECT_AUDIO"] == "1" {
            let group = try await asset.loadMediaSelectionGroup(for: .audible)
            let selectedOption = group.flatMap { item.currentMediaSelection.selectedMediaOption(in: $0) }
            let enabledTrack = item.tracks.contains { $0.isEnabled && $0.assetTrack?.mediaType == .audio }
            XCTAssertTrue(selectedOption != nil || enabledTrack, "AVPlayer must select the muxed audio.")
        }
        controller.dispose()
        XCTAssertNil(bridge.currentRtspSession)
        XCTAssertNil(player.currentItem)
    }

    @MainActor
    func testPlaybackRetryReopensAnRtspSlotWhileAnotherControllerPlays() async throws {
        let policy = VesperRetryPolicy(maxAttempts: 1, baseDelayMs: 50)
        try await withTwoPlayingRtspControllers(retryPolicy: policy) { first, second in
            let oldPlayer = try XCTUnwrap(first.player)
            let oldHealthTask = try XCTUnwrap(first.rtspHealthTask)
            let otherPlayer = try XCTUnwrap(second.player)
            let otherItem = try XCTUnwrap(otherPlayer.currentItem)
            let otherHealthTask = try XCTUnwrap(second.rtspHealthTask)
            let otherPosition = otherPlayer.currentTime().seconds

            first.handlePlaybackFailureForTesting(error: originTimeout, fallbackMessage: originTimeout.message)

            XCTAssertNil(first.currentRtspSession)
            XCTAssertNil(oldPlayer.currentItem)
            XCTAssertTrue(oldHealthTask.isCancelled)
            XCTAssertEqual(first.retryAttemptCount, 1)
            XCTAssertNil(first.lastError)

            let deadline = ContinuousClock().now + .seconds(15)
            while ContinuousClock().now < deadline {
                let didReopen = first.player !== oldPlayer && first.currentRtspSession != nil
                    && first.activeSourceCommand == nil
                if first.lastError != nil || didReopen { break }
                try await Task.sleep(for: .milliseconds(50))
            }
            XCTAssertNil(first.lastError, "Retry should not report a busy RTSP bridge.")
            XCTAssertNotNil(first.currentRtspSession)
            let newPlayer = try XCTUnwrap(first.player)
            XCTAssertFalse(newPlayer === oldPlayer)
            try await assertPlaybackAdvances(newPlayer)
            try await assertPlaybackAdvances(otherPlayer, from: otherPosition)
            XCTAssertTrue(second.player === otherPlayer)
            XCTAssertTrue(otherPlayer.currentItem === otherItem)
            XCTAssertFalse(otherHealthTask.isCancelled)
            XCTAssertNil(second.lastError)
        }
    }

    @MainActor
    func testInitializingHealthyRtspTwiceKeepsBothPlayersRunning() async throws {
        try await withTwoPlayingRtspControllers(retryPolicy: VesperRetryPolicy(maxAttempts: 0)) { first, second in
            let firstPlayer = try XCTUnwrap(first.player)
            let firstItem = try XCTUnwrap(firstPlayer.currentItem)
            let firstSession = try XCTUnwrap(first.currentRtspSession)
            let secondPlayer = try XCTUnwrap(second.player)
            let secondItem = try XCTUnwrap(secondPlayer.currentItem)
            let firstPosition = firstPlayer.currentTime().seconds
            let secondPosition = secondPlayer.currentTime().seconds

            try await first.initializeAsync()

            XCTAssertTrue(first.currentRtspSession === firstSession)
            XCTAssertTrue(first.player?.currentItem === firstItem)
            XCTAssertTrue(second.player?.currentItem === secondItem)
            try await assertPlaybackAdvances(firstPlayer, from: firstPosition)
            try await assertPlaybackAdvances(secondPlayer, from: secondPosition)
            XCTAssertNil(first.lastError)
            XCTAssertNil(second.lastError)
        }
    }

    @MainActor
    func testRtspPlaylistStartupFailureRetiresItsHandle() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["VESPER_RTSP_EXPECT_BUNDLED"] != "0",
              let input = environment["VESPER_RTSP_TEST_URI"], let origin = URL(string: input) else {
            throw XCTSkip("Use an RTSP-enabled kit and set VESPER_RTSP_TEST_URI to a controlled origin.")
        }
        let missing = origin.deletingLastPathComponent().appendingPathComponent("unpublished-stream")
        let bridge = VesperNativePlayerBridge(
            initialSource: .remoteUrl(missing, label: "Unpublished fixture", protocol: .rtsp),
            resiliencePolicy: VesperPlaybackResiliencePolicy(retry: VesperRetryPolicy(maxAttempts: 0)),
            systemPlayerIsMuted: true
        )
        defer { bridge.dispose() }
        do {
            try await bridge.initializeAsync()
            XCTFail("An unpublished stream must fail before an AVPlayerItem is installed.")
        } catch let error as VesperPlayerError {
            XCTAssertEqual(error.details["protocol"], "rtsp")
        }
        XCTAssertNil(bridge.player)
        XCTAssertNil(bridge.currentRtspSession)
        let handle = try XCTUnwrap(bridge.retiringRtspHandle)
        try await VesperRtspSession.waitForRetirement(handle, deadline: ContinuousClock().now + .seconds(5))
    }

    @MainActor
    func testTerminalPlaybackFailureReleasesOnlyItsRtspSlot() async throws {
        try await withTwoPlayingRtspControllers(retryPolicy: VesperRetryPolicy(maxAttempts: 0)) { first, second in
            let oldPlayer = try XCTUnwrap(first.player)
            let oldHealthTask = try XCTUnwrap(first.rtspHealthTask)
            let otherPlayer = try XCTUnwrap(second.player)
            let otherItem = try XCTUnwrap(otherPlayer.currentItem)
            let otherHealthTask = try XCTUnwrap(second.rtspHealthTask)
            let otherPosition = otherPlayer.currentTime().seconds

            first.handlePlaybackFailureForTesting(error: originTimeout, fallbackMessage: originTimeout.message)

            XCTAssertNil(first.currentRtspSession)
            XCTAssertNil(oldPlayer.currentItem)
            XCTAssertTrue(oldHealthTask.isCancelled)
            XCTAssertNil(first.retryTask)
            XCTAssertEqual(first.uiState.playbackState, .paused)
            XCTAssertEqual(first.lastError?.code, .timeout)
            XCTAssertEqual(first.lastError?.category, .network)
            XCTAssertEqual(first.lastError?.retriable, true)
            XCTAssertEqual(first.lastError?.details["reason"], "rtsp_origin_timeout")
            XCTAssertEqual(first.lastError?.details["attemptsExhausted"], "true")
            let source = try XCTUnwrap(first.currentSource)
            let handle = try XCTUnwrap(first.retiringRtspHandle)
            try await VesperRtspSession.waitForRetirement(handle, deadline: ContinuousClock().now + .seconds(5))
            let replacement = try VesperRtspSession(source: source)
            let replacementHandle = try XCTUnwrap(replacement.close())
            try await VesperRtspSession.waitForRetirement(replacementHandle, deadline: ContinuousClock().now + .seconds(5))

            try await assertPlaybackAdvances(otherPlayer, from: otherPosition)
            XCTAssertTrue(second.player === otherPlayer)
            XCTAssertTrue(otherPlayer.currentItem === otherItem)
            XCTAssertFalse(otherHealthTask.isCancelled)
            XCTAssertNil(second.lastError)
        }
    }

    @MainActor
    func testSupersededRtspStartupCannotCloseItsReplacement() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["VESPER_RTSP_EXPECT_BUNDLED"] != "0",
              let input = environment["VESPER_RTSP_TEST_URI"], let url = URL(string: input) else {
            throw XCTSkip("Use an RTSP-enabled kit and set VESPER_RTSP_TEST_URI to a controlled origin.")
        }
        let source = VesperPlayerSource.remoteUrl(url, label: "RTSP fixture", protocol: .rtsp)
        let bridge = VesperNativePlayerBridge(initialSource: source,
            resiliencePolicy: VesperPlaybackResiliencePolicy(retry: VesperRetryPolicy(maxAttempts: 0)),
            systemPlayerIsMuted: true)
        defer { bridge.dispose() }
        let original = bridge.startSourceSelection(source)
        let deadline = ContinuousClock().now + .seconds(3)
        while bridge.currentRtspSession == nil, ContinuousClock().now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        let originalSession = try XCTUnwrap(bridge.currentRtspSession)
        XCTAssertNil(bridge.player)
        let replacement = bridge.startSourceSelection(source)
        do {
            try await original.value
            XCTFail("The superseded activation must finish as obsolete.")
        } catch let error as VesperPlayerError {
            XCTAssertEqual(error.details["obsolete"], "true")
        }
        try await replacement.value
        let currentSession = try XCTUnwrap(bridge.currentRtspSession)
        XCTAssertFalse(currentSession === originalSession)
        try await assertPlaybackAdvances(XCTUnwrap(bridge.player))
        bridge.dispose()
        let handle = try XCTUnwrap(bridge.retiringRtspHandle)
        try await VesperRtspSession.waitForRetirement(handle, deadline: ContinuousClock().now + .seconds(5))
    }

    private var originTimeout: VesperPlayerError {
        VesperPlayerError(
            message: "Injected RTSP origin timeout.",
            code: .timeout,
            category: .network,
            retriable: true,
            details: ["reason": "rtsp_origin_timeout", "route": "rtspBridge", "protocol": "rtsp"]
        )
    }

    @MainActor
    private func withTwoPlayingRtspControllers(
        retryPolicy: VesperRetryPolicy,
        body: (VesperNativePlayerBridge, VesperNativePlayerBridge) async throws -> Void
    ) async throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["VESPER_RTSP_EXPECT_BUNDLED"] != "0",
              let input = environment["VESPER_RTSP_TEST_URI"], let url = URL(string: input) else {
            throw XCTSkip("Use an RTSP-enabled kit and set VESPER_RTSP_TEST_URI to a controlled origin.")
        }
        let source = VesperPlayerSource.remoteUrl(url, label: "RTSP fixture", protocol: .rtsp)
        let first = VesperNativePlayerBridge(
            initialSource: source,
            resiliencePolicy: VesperPlaybackResiliencePolicy(retry: retryPolicy),
            systemPlayerIsMuted: true
        )
        let second = VesperNativePlayerBridge(initialSource: source, systemPlayerIsMuted: true)
        let firstController = VesperPlayerController(first, keepScreenOnDuringPlayback: false)
        let secondController = VesperPlayerController(second, keepScreenOnDuringPlayback: false)
        defer {
            firstController.dispose()
            secondController.dispose()
        }
        try await firstController.initializeAsync()
        try await secondController.initializeAsync()
        XCTAssertThrowsError(try VesperRtspSession(source: source)) { error in
            XCTAssertEqual((error as? VesperPlayerError)?.details["reason"], "rtsp_bridge_busy")
        }
        let firstPlayer = try XCTUnwrap(first.player)
        let secondPlayer = try XCTUnwrap(second.player)
        firstPlayer.play()
        secondPlayer.play()
        try await assertPlaybackAdvances(firstPlayer)
        try await assertPlaybackAdvances(secondPlayer)
        try await body(first, second)
        firstController.dispose()
        secondController.dispose()
        for handle in [first.retiringRtspHandle, second.retiringRtspHandle].compactMap({ $0 }) {
            try await VesperRtspSession.waitForRetirement(handle, deadline: ContinuousClock().now + .seconds(5))
        }
    }

    @MainActor
    private func assertPlaybackAdvances(_ player: AVPlayer, from position: Double? = nil) async throws {
        var start = position
        let deadline = ContinuousClock().now + .seconds(8)
        while ContinuousClock().now < deadline {
            let current = player.currentTime().seconds
            if current.isFinite {
                if let start, current - start >= 0.25 { return }
                start = start ?? current
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTFail("Expected playback to advance by at least 250 ms.")
    }
}
