import AVFoundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperCommandContractTests: XCTestCase {
    func testSourceTimelineReadinessRequiresExplicitTimelineEvidence() {
        XCTAssertEqual(
            sourceTimelineReadiness(
                durationMs: 4_000,
                hasIndefiniteDuration: false,
                seekableRange: nil,
                isConfirmedLive: false
            ),
            .ready(.vod)
        )
        XCTAssertEqual(
            sourceTimelineReadiness(
                durationMs: nil,
                hasIndefiniteDuration: true,
                seekableRange: SeekableRangeUi(startMs: 10_000, endMs: 30_000),
                isConfirmedLive: true
            ),
            .ready(.liveDvr)
        )
        XCTAssertEqual(
            sourceTimelineReadiness(
                durationMs: nil,
                hasIndefiniteDuration: true,
                seekableRange: nil,
                isConfirmedLive: true
            ),
            .ready(.live)
        )
        XCTAssertEqual(
            sourceTimelineReadiness(
                durationMs: nil,
                hasIndefiniteDuration: true,
                seekableRange: nil,
                isConfirmedLive: false
            ),
            .waiting
        )
    }

    func testSourceCommandRetriesThenSucceeds() async throws {
        var attempts = 0
        let bridge = VesperNativePlayerBridge(
            resiliencePolicy: retryPolicy(maxAttempts: 2, delayMs: 1),
            sourceLoadAttemptOverride: { _, _, _, _ in
                attempts += 1
                if attempts == 1 {
                    throw fixtureNetworkError()
                }
            }
        )
        defer { bridge.dispose() }

        try await bridge.selectSourceAsync(remoteSource("retry-success"))

        XCTAssertEqual(attempts, 2)
        XCTAssertNil(bridge.lastError)
        XCTAssertNil(bridge.activeSourceCommand)
    }

    func testSourceCommandRetryExhaustionPreservesDomainReason() async {
        var attempts = 0
        let bridge = VesperNativePlayerBridge(
            resiliencePolicy: retryPolicy(maxAttempts: 1, delayMs: 1),
            sourceLoadAttemptOverride: { _, _, _, _ in
                attempts += 1
                throw fixtureNetworkError()
            }
        )
        defer { bridge.dispose() }

        let error = await capturePlayerError {
            try await bridge.selectSourceAsync(remoteSource("retry-exhausted"))
        }

        XCTAssertEqual(attempts, 2)
        XCTAssertEqual(error.details["reason"], "fixtureNetworkFailure")
        XCTAssertEqual(error.details["commandReason"], "sourceCommandRetryExhausted")
        XCTAssertEqual(error.details["retryAttempts"], "1")
        XCTAssertEqual(error.details["attemptsExhausted"], "true")
        XCTAssertEqual(error.details["commandId"], "1")
        XCTAssertEqual(error.details["sourceEpoch"], "1")
        XCTAssertNil(error.details["obsolete"])
        XCTAssertEqual(bridge.lastError, error)
    }

    func testSourceRetriesShareOneTotalDeadline() async {
        var attempts = 0
        let clock = ContinuousClock()
        let startedAt = clock.now
        let bridge = VesperNativePlayerBridge(
            resiliencePolicy: retryPolicy(maxAttempts: 50, delayMs: 1_000),
            sourceReadinessWaitPolicy: VesperSourceReadinessWaitPolicy(
                timeout: .milliseconds(80),
                pollInterval: .milliseconds(1)
            ),
            sourceLoadAttemptOverride: { _, _, _, _ in
                attempts += 1
                throw fixtureNetworkError()
            }
        )
        defer { bridge.dispose() }

        let error = await capturePlayerError {
            try await bridge.selectSourceAsync(remoteSource("shared-deadline"))
        }
        let elapsed = startedAt.duration(to: clock.now)

        XCTAssertLessThanOrEqual(attempts, 2)
        XCTAssertEqual(error.code, .timeout)
        XCTAssertEqual(error.details["reason"], "sourceCommandTimeout")
        XCTAssertEqual(error.details["commandReason"], "sourceCommandTimeout")
        XCTAssertLessThan(elapsed, .milliseconds(500))
    }

    func testPauseDuringSourceRetryClearsPendingAutoplay() async throws {
        var attempts = 0
        let bridge = VesperNativePlayerBridge(
            resiliencePolicy: retryPolicy(maxAttempts: 2, delayMs: 50),
            sourceLoadAttemptOverride: { _, _, _, _ in
                attempts += 1
                if attempts == 1 {
                    throw fixtureNetworkError()
                }
            }
        )
        defer { bridge.dispose() }

        let command = Task { @MainActor in
            try await bridge.selectSourceAsync(remoteSource("pause-retry"))
        }
        let retryStarted = await waitUntil { bridge.retryAttemptCount == 1 }
        XCTAssertTrue(retryStarted)
        XCTAssertTrue(bridge.pendingAutoPlay)

        bridge.pause()

        XCTAssertFalse(bridge.pendingAutoPlay)
        XCTAssertEqual(bridge.uiState.playbackState, .paused)
        try await command.value
        XCTAssertEqual(attempts, 2)
        XCTAssertFalse(bridge.pendingAutoPlay)
        XCTAssertEqual(bridge.uiState.playbackState, .paused)
        XCTAssertNil(bridge.lastError)
    }

    func testPausedPlaybackAtZeroRemainsPausedAcrossRefreshDerivation() {
        let player = AVPlayer()

        XCTAssertEqual(
            derivePlaybackState(
                currentState: .paused,
                player: player,
                durationMs: 5_000,
                positionMs: 0
            ),
            .paused
        )
        XCTAssertEqual(
            derivePlaybackState(
                currentState: .ready,
                player: player,
                durationMs: 5_000,
                positionMs: 0
            ),
            .ready
        )
    }

    func testPauseAtZeroClearsEveryDeferredPlaybackStart() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        bridge.player = AVPlayer()
        bridge.pendingAutoPlay = true
        bridge.pendingPlaybackStart = true
        bridge.pendingPlayAfterStopSeek = true

        bridge.pause()

        XCTAssertFalse(bridge.pendingAutoPlay)
        XCTAssertFalse(bridge.pendingPlaybackStart)
        XCTAssertFalse(bridge.pendingPlayAfterStopSeek)
        XCTAssertEqual(bridge.uiState.playbackState, .paused)
    }

    func testInitializeAsyncWaitsAcrossDeferredNativeFrameSurfaceAttachment() async {
        let source = VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginReferences: [VesperBundledPluginReferences.sourceNormalizerFfmpeg]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginReferences: [VesperBundledPluginReferences.decoderVideoToolbox]
            )
        )
        defer { bridge.dispose() }

        var didSettle = false
        let command = Task { @MainActor in
            defer { didSettle = true }
            try await bridge.initializeAsync()
        }
        let waitingForSurface = await waitUntil { bridge.pendingNativeFrameSurfaceLoad }

        XCTAssertTrue(waitingForSurface)
        XCTAssertFalse(didSettle)
        XCTAssertNil(bridge.lastError)

        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)
        let error = await capturePlayerError {
            try await command.value
        }

        XCTAssertTrue(didSettle)
        XCTAssertEqual(error.code, .unsupported)
        XCTAssertEqual(error.category, .capability)
        XCTAssertTrue(error.message.contains("SourceNormalizer"))
    }

    func testSupersededSourceFutureGetsTypedObsoleteErrorOnly() async throws {
        var startedSources: [String] = []
        let first = remoteSource("source-one")
        let second = remoteSource("source-two")
        let bridge = VesperNativePlayerBridge(
            sourceLoadAttemptOverride: { _, source, _, _ in
                startedSources.append(source.label)
                if source == first {
                    try await Task.sleep(for: .seconds(30))
                }
            }
        )
        defer { bridge.dispose() }

        let firstCommand = Task { @MainActor in
            try await bridge.selectSourceAsync(first)
        }
        let firstStarted = await waitUntil { startedSources.contains(first.label) }
        XCTAssertTrue(firstStarted)

        try await bridge.selectSourceAsync(second)
        let firstError = await capturePlayerError {
            try await firstCommand.value
        }

        XCTAssertEqual(firstError.code, .cancelled)
        XCTAssertEqual(firstError.details["reason"], "sourceCommandSuperseded")
        XCTAssertEqual(firstError.details["commandReason"], "sourceCommandSuperseded")
        XCTAssertEqual(firstError.details["commandId"], "1")
        XCTAssertEqual(firstError.details["sourceEpoch"], "1")
        XCTAssertEqual(firstError.details["obsolete"], "true")
        XCTAssertEqual(bridge.currentSource, second)
        XCTAssertNil(bridge.lastError)
    }

    func testAsyncSeekSubmitsExactRequestAndWaitsForCompletion() async throws {
        let probe = VesperSeekSubmissionProbe()
        let bridge = makeSeekBridge(probe: probe)
        defer { bridge.dispose() }

        let command = Task { @MainActor in
            try await bridge.seekAsync(by: 1_250)
        }
        let firstSubmission = await waitUntil { probe.submissions.count == 1 }
        XCTAssertTrue(firstSubmission)
        XCTAssertNotNil(bridge.activeSeekCommand)
        XCTAssertEqual(probe.submissions[0].target.milliseconds, 1_250)
        XCTAssertEqual(CMTimeCompare(probe.submissions[0].toleranceBefore, .zero), 0)
        XCTAssertEqual(CMTimeCompare(probe.submissions[0].toleranceAfter, .zero), 0)

        probe.complete(at: 0, finished: true)
        try await command.value

        XCTAssertNil(bridge.activeSeekCommand)
        XCTAssertNil(bridge.lastError)
    }

    func testAsyncSeekRejectsFinishedFalse() async {
        let probe = VesperSeekSubmissionProbe()
        let bridge = makeSeekBridge(probe: probe)
        defer { bridge.dispose() }

        let command = Task { @MainActor in
            try await bridge.seekAsync(by: 2_000)
        }
        let submissionStarted = await waitUntil { probe.submissions.count == 1 }
        XCTAssertTrue(submissionStarted)
        probe.complete(at: 0, finished: false)

        let error = await capturePlayerError {
            try await command.value
        }
        XCTAssertEqual(error.code, .seekFailure)
        XCTAssertEqual(error.details["reason"], "seekCommandInterrupted")
        XCTAssertEqual(error.details["commandReason"], "seekCommandInterrupted")
        XCTAssertNil(error.details["obsolete"])
        XCTAssertEqual(bridge.lastError, error)
    }

    func testNewSeekSupersedesOldFutureWithoutPublishingObsoleteError() async throws {
        let probe = VesperSeekSubmissionProbe()
        let bridge = makeSeekBridge(probe: probe)
        defer { bridge.dispose() }

        let first = Task { @MainActor in
            try await bridge.seekAsync(by: 1_000)
        }
        let firstSubmission = await waitUntil { probe.submissions.count == 1 }
        XCTAssertTrue(firstSubmission)
        let second = Task { @MainActor in
            try await bridge.seekAsync(by: 2_000)
        }
        let secondSubmission = await waitUntil { probe.submissions.count == 2 }
        XCTAssertTrue(secondSubmission)

        let firstError = await capturePlayerError {
            try await first.value
        }
        XCTAssertEqual(firstError.details["commandReason"], "seekCommandSuperseded")
        XCTAssertEqual(firstError.details["obsolete"], "true")
        XCTAssertNil(bridge.lastError)

        probe.complete(at: 0, finished: true)
        probe.complete(at: 1, finished: true)
        try await second.value
        XCTAssertNil(bridge.lastError)
    }

    func testSourceReplacementCancelsPendingSeekFuture() async throws {
        let probe = VesperSeekSubmissionProbe()
        let bridge = makeSeekBridge(
            probe: probe,
            sourceLoadAttemptOverride: { _, _, _, _ in }
        )
        defer { bridge.dispose() }

        let seek = Task { @MainActor in
            try await bridge.seekAsync(by: 3_000)
        }
        let submissionStarted = await waitUntil { probe.submissions.count == 1 }
        XCTAssertTrue(submissionStarted)

        try await bridge.selectSourceAsync(remoteSource("replacement"))
        let error = await capturePlayerError {
            try await seek.value
        }

        XCTAssertEqual(error.details["commandReason"], "seekSourceChanged")
        XCTAssertEqual(error.details["obsolete"], "true")
        XCTAssertNil(bridge.lastError)
        probe.complete(at: 0, finished: true)
    }

    func testDisposeSettlesPendingSeekExactlyOnceWithoutPublishingError() async {
        let probe = VesperSeekSubmissionProbe()
        let bridge = makeSeekBridge(probe: probe)

        let seek = Task { @MainActor in
            try await bridge.seekAsync(by: 4_000)
        }
        let submissionStarted = await waitUntil { probe.submissions.count == 1 }
        XCTAssertTrue(submissionStarted)

        bridge.dispose()
        let error = await capturePlayerError {
            try await seek.value
        }

        XCTAssertEqual(error.details["commandReason"], "seekPlaybackTornDown")
        XCTAssertEqual(error.details["obsolete"], "true")
        XCTAssertNil(bridge.activeSeekCommand)
        XCTAssertNil(bridge.lastError)
        probe.complete(at: 0, finished: true)
        await Task.yield()
        XCTAssertNil(bridge.lastError)
    }

    private func makeSeekBridge(
        probe: VesperSeekSubmissionProbe,
        sourceLoadAttemptOverride: VesperSourceLoadAttemptOverride? = nil
    ) -> VesperNativePlayerBridge {
        let bridge = VesperNativePlayerBridge(
            sourceLoadAttemptOverride: sourceLoadAttemptOverride,
            systemPlayerSeekSubmitter: { player, target, before, after, completion in
                probe.record(
                    player: player,
                    target: target,
                    toleranceBefore: before,
                    toleranceAfter: after,
                    completion: completion
                )
            }
        )
        bridge.currentSource = remoteSource("seek-source")
        bridge.player = AVPlayer()
        bridge.publishedUiState = PlayerHostUiState(
            title: "Test",
            subtitle: "Test",
            sourceLabel: "seek-source",
            playbackState: .paused,
            playbackRate: 0,
            isBuffering: false,
            isInterrupted: false,
            timeline: TimelineUiState(
                kind: .vod,
                isSeekable: true,
                seekableRange: SeekableRangeUi(startMs: 0, endMs: 5_000),
                liveEdgeMs: nil,
                positionMs: 0,
                durationMs: 5_000
            )
        )
        return bridge
    }

    private func capturePlayerError(
        _ operation: () async throws -> Void,
        file: StaticString = #filePath,
        line: UInt = #line
    ) async -> VesperPlayerError {
        do {
            try await operation()
            XCTFail("operation should fail", file: file, line: line)
        } catch let error as VesperPlayerError {
            return error
        } catch {
            XCTFail("expected VesperPlayerError, got \(error)", file: file, line: line)
        }
        return VesperPlayerError(
            message: "missing test error",
            code: .backendFailure,
            category: .platform,
            retriable: false
        )
    }

    private func waitUntil(
        timeout: Duration = .seconds(1),
        _ condition: () -> Bool
    ) async -> Bool {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: timeout)
        while clock.now < deadline {
            if condition() {
                return true
            }
            try? await Task.sleep(for: .milliseconds(5))
        }
        return condition()
    }
}

@MainActor
private final class VesperSeekSubmissionProbe {
    struct Submission {
        let player: AVPlayer
        let target: CMTime
        let toleranceBefore: CMTime
        let toleranceAfter: CMTime
        let completion: @Sendable (Bool) -> Void
    }

    private(set) var submissions: [Submission] = []

    func record(
        player: AVPlayer,
        target: CMTime,
        toleranceBefore: CMTime,
        toleranceAfter: CMTime,
        completion: @escaping @Sendable (Bool) -> Void
    ) {
        submissions.append(
            Submission(
                player: player,
                target: target,
                toleranceBefore: toleranceBefore,
                toleranceAfter: toleranceAfter,
                completion: completion
            )
        )
    }

    func complete(at index: Int, finished: Bool) {
        submissions[index].completion(finished)
    }
}

private func remoteSource(_ label: String) -> VesperPlayerSource {
    VesperPlayerSource.remoteUrl(
        URL(string: "https://example.com/\(label).mp4")!,
        label: label,
        protocol: .progressive
    )
}

private func retryPolicy(maxAttempts: Int, delayMs: UInt64) -> VesperPlaybackResiliencePolicy {
    VesperPlaybackResiliencePolicy(
        retry: VesperRetryPolicy(
            maxAttempts: maxAttempts,
            baseDelayMs: delayMs,
            maxDelayMs: delayMs,
            backoff: .fixed
        )
    )
}

private func fixtureNetworkError() -> VesperPlayerError {
    VesperPlayerError(
        message: "fixture network failure",
        code: .backendFailure,
        category: .network,
        retriable: true,
        details: ["reason": "fixtureNetworkFailure"]
    )
}
