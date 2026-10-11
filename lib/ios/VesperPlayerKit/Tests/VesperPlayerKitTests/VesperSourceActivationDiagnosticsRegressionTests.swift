import Combine
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperSourceActivationDiagnosticsRegressionTests: XCTestCase {
    private func source(_ label: String) throws -> VesperPlayerSource {
        .dash(url: try XCTUnwrap(URL(string: "https://fixture.invalid/\(label).mpd")), label: label)
    }

    func testPublicActivationMatchesDiagnosticFirstFrameAcrossDivergentCounters() async throws {
        let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { bridge, _, _, _ in
            // Observer lifetime changes need not match diagnostic attempt changes.
            for _ in 0..<3 { _ = bridge.advancePlaybackEpoch() }
            let token = bridge.diagnosticsTracker.capture()
            XCTAssertTrue(bridge.diagnosticsTracker.firstFrame(token, mediaPositionMs: 250))
        })
        let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
        let session = try VesperSourceSession()
        defer { controller.dispose(); session.close() }
        var published: VesperPlaybackDiagnosticsSnapshot?
        let subscription = controller.playbackDiagnosticsPublisher.sink { published = $0 }
        defer { subscription.cancel() }
        var previous: UInt64 = 0
        for label in ["template", "timeline", "base", "progressive-replacement"] {
            let handle = try session.register(source(label))
            let result = try await controller.activate(handle, options: .init(playWhenReady: false, timeoutMs: 2000))
            let diagnostics = try XCTUnwrap(controller.playbackDiagnostics)
            XCTAssertGreaterThan(result.playbackEpoch, previous)
            XCTAssertEqual(result.playbackEpoch, diagnostics.playbackEpoch)
            XCTAssertEqual(result.playbackEpoch, diagnostics.firstFrame?.playbackEpoch)
            XCTAssertEqual(result.playbackEpoch, published?.playbackEpoch)
            XCTAssertNotEqual(bridge.currentPlaybackEpoch(), diagnostics.playbackEpoch)
            let internalFence = bridge.currentPlaybackEpoch()
            XCTAssertTrue(bridge.isPlaybackEpochCurrent(internalFence))
            XCTAssertFalse(bridge.isPlaybackEpochCurrent(internalFence &- 1))
            previous = result.playbackEpoch
        }
    }

    func testSupersededAndCancelledPrepareReturnReplacementDiagnosticEpoch() async throws {
        for explicitlyCancel in [false, true] {
            let gate = DiagnosticActivationGate(entered: expectation(description: "pending prepare began"))
            let bridge = VesperNativePlayerBridge(sourceLoadAttemptOverride: { bridge, source, _, _ in
                if source.label == "stall" { await gate.wait(); return }
                _ = bridge.advancePlaybackEpoch()
                let token = bridge.diagnosticsTracker.capture()
                bridge.diagnosticsTracker.firstFrame(token, mediaPositionMs: 500)
            })
            // Reproduce divergence in either direction, without numeric compensation.
            for _ in 0..<7 { _ = bridge.advancePlaybackEpoch() }
            let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
            let session = try VesperSourceSession()
            defer { gate.open(); controller.dispose(); session.close() }
            let stalled = try session.register(source("stall"))
            let recovery = try session.register(source("recovery"))
            let pending = Task { try await controller.activate(stalled, options: .init(timeoutMs: 2000)) }
            await fulfillment(of: [gate.entered], timeout: 1)
            if explicitlyCancel {
                pending.cancel()
                do { _ = try await pending.value; XCTFail("Cancelled activation succeeded") }
                catch { XCTAssertEqual(error as? VesperSourceActivationError, .cancelled) }
            }
            let activated = try await controller.activate(recovery, options: .init(playWhenReady: false, timeoutMs: 2000))
            if !explicitlyCancel {
                do { _ = try await pending.value; XCTFail("Superseded activation succeeded") }
                catch { XCTAssertEqual(error as? VesperSourceActivationError, .superseded) }
            }
            XCTAssertEqual(activated.sourceId, recovery.id)
            XCTAssertEqual(activated.playbackEpoch, controller.playbackDiagnostics?.playbackEpoch)
            XCTAssertEqual(activated.playbackEpoch, controller.playbackDiagnostics?.firstFrame?.playbackEpoch)
            gate.open()
            await Task.yield()
            XCTAssertEqual(activated.playbackEpoch, controller.playbackDiagnostics?.playbackEpoch)
        }
    }
}

@MainActor
private final class DiagnosticActivationGate {
    let entered: XCTestExpectation
    private var continuation: CheckedContinuation<Void, Never>?
    init(entered: XCTestExpectation) { self.entered = entered }
    func wait() async {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            entered.fulfill()
        }
    }
    func open() {
        let pending = continuation
        continuation = nil
        pending?.resume()
    }
}
