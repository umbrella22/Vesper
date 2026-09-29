import AVFoundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperSourceActivationTests: XCTestCase {
    private func source(_ label: String) -> VesperPlayerSource {
        .remoteUrl(URL(string: "https://fixture.test/\(label).mp4")!, label: label)
    }

    func testActivationReturnsMatchingEpochAndRetainsScopedLease() async throws {
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession()
        defer { controller.dispose(); session.close() }
        let handle = try session.register(.dash(url: URL(string: "https://fixture.test/manifest.mpd")!))
        let expected = try session.acquire(handle).sourceForActivation().dashStartupScope
        let result = try await controller.activate(handle, options: .init(playWhenReady: false, playbackRate: 1.5))
        XCTAssertEqual(result.sourceId, handle.id)
        XCTAssertEqual(result.sessionId, session.id)
        XCTAssertEqual(result.playbackEpoch, bridge.playbackEpochSnapshot())
        XCTAssertEqual(bridge.currentSource?.dashStartupScope, expected)
        XCTAssertFalse(bridge.pendingAutoPlay)
        session.close()
        XCTAssertNotNil(try controller.activeSourceLease?.sourceForActivation())
        session.invalidate()
        XCTAssertThrowsError(try controller.activeSourceLease?.sourceForActivation())
    }

    func testSupersessionSettlesBeforeUncooperativeLoadExits() async throws {
        let gate = ActivationTestGate()
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, source, _, _ in
            if source.label == "first" { await gate.wait() }
        })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession()
        defer { controller.dispose(); session.close() }
        let a = try session.register(source("first")), b = try session.register(source("second"))
        let first = Task { try await controller.activate(a, options: .init(playWhenReady: false)) }
        try await gate.started()
        let second = try await controller.activate(b, options: .init(playWhenReady: false))
        do { _ = try await first.value; XCTFail("Superseded activation succeeded") }
        catch { XCTAssertEqual(error as? VesperSourceActivationError, .superseded) }
        XCTAssertEqual(second.sourceId, b.id)
        await gate.open()
        await Task.yield()
        XCTAssertEqual(bridge.currentSource?.label, "second")
    }

    func testActivationAwaitsInitialSeekBeforeApplyingRateAndPause() async throws {
        var completion: (@Sendable (Bool) -> Void)?
        var requestedPosition: Double?
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { bridge, _, _, _ in
            bridge.player = AVPlayer()
            bridge.publishedUiState = PlayerHostUiState(title: "test", subtitle: "test", sourceLabel: "seek",
                playbackState: .paused, playbackRate: 1, isBuffering: false, isInterrupted: false,
                timeline: TimelineUiState(kind: .vod, isSeekable: true,
                    seekableRange: .init(startMs: 0, endMs: 5000), liveEdgeMs: nil, positionMs: 0, durationMs: 5000))
        }, systemPlayerSeekSubmitter: { _, target, _, _, callback in
            requestedPosition = target.seconds
            completion = callback
        })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession()
        defer { controller.dispose(); session.close() }
        let handle = try session.register(source("seek"))
        var settled = false
        let work = Task {
            let result = try await controller.activate(handle, options: .init(playWhenReady: false, startPositionMs: 1250, playbackRate: 1.5))
            settled = true
            return result
        }
        for _ in 0..<1000 { if completion != nil { break }; await Task.yield() }
        XCTAssertEqual(requestedPosition, 1.25)
        XCTAssertFalse(settled)
        let callback = try XCTUnwrap(completion)
        callback(true)
        _ = try await work.value
        XCTAssertTrue(settled)
        XCTAssertFalse(bridge.pendingAutoPlay)
    }

    func testTimeoutAndDisposeSettlePendingActivation() async throws {
        for dispose in [false, true] {
            let gate = ActivationTestGate()
            let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in await gate.wait() })
            let controller = VesperPlayerController(bridge)
            let session = try VesperSourceSession()
            let handle = try session.register(source("waiting"))
            let work = Task { try await controller.activate(handle, options: .init(timeoutMs: dispose ? 5000 : 20)) }
            if dispose {
                try await gate.started()
                controller.dispose()
            }
            do { _ = try await work.value; XCTFail("Obsolete activation succeeded") }
            catch { XCTAssertEqual(error as? VesperSourceActivationError, dispose ? .disposed : .timeout) }
            await gate.open()
            controller.dispose(); session.close()
        }
    }

    func testCloseDuringAcquiredActivationIsAllowedButInvalidationIsNot() async throws {
        for invalidate in [false, true] {
            let gate = ActivationTestGate()
            let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in await gate.wait() })
            let controller = VesperPlayerController(bridge)
            let session = try VesperSourceSession()
            let handle = try session.register(source("waiting"))
            let work = Task { try await controller.activate(handle, options: .init(playWhenReady: false)) }
            try await gate.started()
            if invalidate { session.invalidate() } else { session.close() }
            await gate.open()
            do {
                let result = try await work.value
                XCTAssertFalse(invalidate)
                XCTAssertEqual(result.sourceId, handle.id)
            } catch {
                XCTAssertTrue(invalidate)
                XCTAssertEqual(error as? VesperSourceSessionError, .invalidated)
            }
            controller.dispose()
        }
    }

    func testSequenceMutationDoesNotActivateAndReorderRetainsScope() async throws {
        var loads = 0
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in loads += 1 })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession(configuration: .init(maxMemoryBytes: 0))
        let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: UUID().uuidString))
        defer { sequence.dispose(); controller.dispose(); session.close() }
        let a = try session.register(source("a")), b = try session.register(source("b"))
        func item(_ id: String, _ handle: VesperSourceHandle) -> VesperPlaybackSequenceItem {
            .init(itemId: id, contentIdentity: .init(providerNamespace: "test", value: id), source: handle)
        }
        try sequence.replace([item("a", a), item("b", b)])
        try sequence.attach(to: controller)
        XCTAssertNil(sequence.snapshot.activeItemId)
        XCTAssertEqual(loads, 0)
        let activation = try await sequence.activate("a", options: .init(playWhenReady: false))
        XCTAssertEqual(loads, 1)
        a.close()
        try sequence.replace([item("b", b), item("a", a)])
        XCTAssertEqual(loads, 1)
        XCTAssertEqual(bridge.playbackEpochSnapshot(), activation.playbackEpoch)
        _ = try await sequence.activate("a", options: .init(playWhenReady: false))
        XCTAssertEqual(loads, 2, "Retained sequence lease may activate after handle close")
        _ = try sequence.remove(itemId: "a")
        XCTAssertEqual(loads, 2)
    }

    func testOnlyExplicitUnresolvedActivationConsumesProviderResponse() async throws {
        var loads = 0
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in loads += 1 })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession(configuration: .init(maxMemoryBytes: 0))
        let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: UUID().uuidString))
        defer { sequence.dispose(); controller.dispose(); session.close() }
        try sequence.replace([.init(itemId: "a", contentIdentity: .init(providerNamespace: "test", value: "a"))])
        try sequence.attach(to: controller)
        let activation = Task { try await sequence.activate("a", options: .init(playWhenReady: false)) }
        for _ in 0..<1000 { if !sequence.snapshot.sourceRequests.isEmpty { break }; await Task.yield() }
        let request = try XCTUnwrap(sequence.snapshot.sourceRequests.first)
        let handle = try session.register(source("a"))
        try sequence.submitResolvedSource(request: request, source: handle)
        _ = try await activation.value
        XCTAssertEqual(loads, 1)
        let revision = try XCTUnwrap(sequence.snapshot.items.first?.sourceRevision)
        try sequence.markSourceExpired(itemId: "a", sourceRevision: revision)
        let refresh = try XCTUnwrap(sequence.snapshot.sourceRequests.first)
        try sequence.submitResolvedSource(request: refresh, source: session.register(source("replacement")))
        await Task.yield()
        XCTAssertEqual(loads, 1, "Accepting a replacement must not replace active playback")
        XCTAssertEqual(bridge.currentSource?.label, "a")
    }

    func testPagingBoundaryAndAppendingItemsNeverActivate() async throws {
        var loads = 0
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in loads += 1 })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession(configuration: .init(maxMemoryBytes: 0))
        let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: UUID().uuidString, mode: .replenishable))
        defer { sequence.dispose(); controller.dispose(); session.close() }
        let first = VesperPlaybackSequenceItem(itemId: "first", contentIdentity: .init(providerNamespace: "test", value: "first"),
                                              source: try session.register(source("first")))
        try sequence.replace([first])
        try sequence.attach(to: controller)
        _ = try await sequence.activate("first", options: .init(playWhenReady: false))
        let boundary = try await sequence.next(options: .init(timeoutMs: 100))
        XCTAssertNil(boundary)
        let envelope = try XCTUnwrap(sequence.snapshot.pendingRequests.first)
        let request = try XCTUnwrap(envelope["request"] as? [String: Any])
        let requestId = try XCTUnwrap(request["requestId"] as? UInt64)
        let item = VesperPlaybackSequenceItem(itemId: "a", contentIdentity: .init(providerNamespace: "test", value: "a"),
                                             source: try session.register(source("a")))
        _ = try sequence.append(sessionGeneration: sequence.snapshot.sessionGeneration, requestId: requestId,
                                anchorItemId: "first", items: [item], endReached: true)
        XCTAssertEqual(sequence.snapshot.activeItemId, "first")
        XCTAssertEqual(loads, 1)
        _ = try await sequence.next(options: .init(playWhenReady: false))
        XCTAssertEqual(loads, 2)
    }

    func testRemovingTargetSettlesAnActivationAlreadyLoading() async throws {
        let gate = ActivationTestGate()
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in await gate.wait() })
        let controller = VesperPlayerController(bridge)
        let session = try VesperSourceSession(configuration: .init(maxMemoryBytes: 0))
        let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: UUID().uuidString))
        defer { sequence.dispose(); controller.dispose(); session.close() }
        try sequence.replace([.init(itemId: "a", contentIdentity: .init(providerNamespace: "test", value: "a"),
                                    source: try session.register(source("a")))])
        try sequence.attach(to: controller)
        let work = Task { try await sequence.activate("a") }
        try await gate.started()
        _ = try sequence.remove(itemId: "a")
        do { _ = try await work.value; XCTFail("Removed activation succeeded") }
        catch { XCTAssertEqual(error as? VesperSourceActivationError, .superseded) }
        await gate.open()
        await Task.yield()
        XCTAssertNil(controller.activeSourceLease)
    }

    func testDetachAndDisposeSettleUnresolvedNavigation() async throws {
        for dispose in [false, true] {
            let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { _, _, _, _ in })
            let controller = VesperPlayerController(bridge)
            let sequence = try VesperPlaybackSequence(configuration: .init(sequenceId: UUID().uuidString))
            try sequence.replace([.init(itemId: "pending", contentIdentity: .init(providerNamespace: "test", value: "pending"))])
            try sequence.attach(to: controller)
            let work = Task { try await sequence.activate("pending") }
            for _ in 0..<1000 { if !sequence.snapshot.sourceRequests.isEmpty { break }; await Task.yield() }
            XCTAssertFalse(sequence.snapshot.sourceRequests.isEmpty)
            if dispose { sequence.dispose() } else { sequence.detach() }
            do { _ = try await work.value; XCTFail("Detached navigation succeeded") }
            catch { XCTAssertEqual(error as? VesperSourceActivationError, dispose ? .disposed : .detached) }
            sequence.dispose(); controller.dispose()
        }
    }
}

private actor ActivationTestGate {
    private var didStart = false
    private var opened = false
    private var waiter: CheckedContinuation<Void, Never>?
    func started() async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(10))
        while !didStart, ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        if !didStart { throw VesperSourceActivationError.timeout }
    }
    func wait() async {
        didStart = true
        if !opened { await withCheckedContinuation { waiter = $0 } }
    }
    func open() { opened = true; waiter?.resume(); waiter = nil }
}
