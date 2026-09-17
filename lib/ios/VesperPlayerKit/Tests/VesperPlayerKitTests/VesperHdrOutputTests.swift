import XCTest
import AVFoundation
@_spi(VesperFlutter) @testable import VesperPlayerKit

final class VesperHdrOutputTests: XCTestCase {
    @MainActor
    func testDedicatedOutputUpdatesBypassGeneralSnapshotSubscription() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        var states: [VesperHdrOutputState] = []
        var generalSnapshots = 0
        var outputNotifications = 0
        let output = controller.hdrOutputPublisher.dropFirst().sink { states.append($0.state) }
        let general = controller.objectWillChange.sink {
            if controller.isPublishingHdrOutputUpdate {
                outputNotifications += 1
            } else {
                generalSnapshots += 1
            }
        }
        let hdr = VesperHdrOutputObservation(state: .hdr, evidence: "testOutputObserver")
        XCTAssertTrue(bridge.outputTracker.apply(bridge.outputTracker.capture(), observation: hdr))
        controller.invalidateHdrOutput()
        XCTAssertTrue(bridge.outputTracker.apply(bridge.outputTracker.capture(), observation: hdr))
        XCTAssertEqual(states, [.hdr, .unknown, .hdr])
        XCTAssertEqual(outputNotifications, 3)
        XCTAssertEqual(generalSnapshots, 0)
        XCTAssertFalse(controller.isPublishingHdrOutputUpdate)
        controller.setSubtitleStyle(.default)
        XCTAssertGreaterThan(generalSnapshots, 0)
        output.cancel()
        general.cancel()
    }

    @MainActor
    func testOutputPublisherPreservesUnknownBeforeImmediateReconfirmation() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        var states: [VesperHdrOutputState] = []
        let observation = controller.hdrOutputPublisher.sink { states.append($0.state) }
        let hdr = VesperHdrOutputObservation(state: .hdr, evidence: "testOutputObserver")
        XCTAssertTrue(bridge.outputTracker.apply(bridge.outputTracker.capture(), observation: hdr))
        controller.invalidateHdrOutput()
        XCTAssertTrue(bridge.outputTracker.apply(bridge.outputTracker.capture(), observation: hdr))
        XCTAssertEqual(states, [.unknown, .hdr, .unknown, .hdr])
        observation.cancel()
    }

    @MainActor
    func testNativeEventsReachControllerWithoutWaitingForSnapshotSampling() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        bridge.publishedEffectiveVideoTrackId = "A"
        let firstA = bridge.outputTracker.capture()
        bridge.publishedEffectiveVideoTrackId = "B"
        bridge.publishedEffectiveVideoTrackId = "A"
        XCTAssertGreaterThan(controller.hdrOutput!.outputGeneration, firstA.outputGeneration)
        XCTAssertFalse(bridge.outputTracker.apply(firstA, observation: VesperHdrOutputObservation(
            state: .hdr, evidence: "testOutputObserver"
        )))
        let beforePlayer = controller.hdrOutput!.outputGeneration
        bridge.player = AVPlayer()
        XCTAssertGreaterThan(controller.hdrOutput!.outputGeneration, beforePlayer)
        let surface = PlayerSurfaceView()
        controller.attachSurfaceHost(surface)
        let beforeSurface = controller.hdrOutput!.outputGeneration
        surface.detachBridgeIfNeeded()
        XCTAssertGreaterThan(controller.hdrOutput!.outputGeneration, beforeSurface)
        let beforePip = controller.hdrOutput!.outputGeneration
        controller.invalidateHdrOutput()
        XCTAssertGreaterThan(controller.hdrOutput!.outputGeneration, beforePip)
        XCTAssertEqual(controller.hdrOutput!.state, .unknown)
    }

    @MainActor
    func testSequenceActivationCountsRepeatedSources() throws {
        let source = VesperPlayerSource.remoteUrl(URL(string: "https://example.invalid/video.mp4")!, label: "Video")
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let controller = VesperPlayerController(bridge)
        let attachment = HdrTestSequenceAttachment()
        defer { controller.dispose() }
        try controller.attachPlaybackSequence(attachment)
        XCTAssertEqual(controller.hdrOutput?.sourceRevision, 1)
        try controller.activateSequenceSource(attachment, source: source)
        try controller.activateSequenceSource(attachment, source: source)
        XCTAssertEqual(controller.hdrOutput?.sourceRevision, 3)
        XCTAssertEqual(controller.hdrOutput?.state, .unknown)
    }

    @MainActor
    func testRecurringTrackAndSurfaceRejectOldEvidence() {
        let tracker = VesperHdrOutputTracker()
        tracker.sourceChanged()
        tracker.videoTrackChanged("A", catalogRevision: 1)
        let firstA = tracker.capture()
        let hdr = VesperHdrOutputObservation(state: .hdr, format: .hdr10, evidence: "testOutputObserver")
        XCTAssertTrue(tracker.apply(firstA, observation: hdr))
        tracker.videoTrackChanged("B", catalogRevision: 1)
        XCTAssertEqual(tracker.snapshot.state, .unknown)
        XCTAssertNil(tracker.snapshot.evidence)
        tracker.videoTrackChanged("A", catalogRevision: 1)
        XCTAssertFalse(tracker.apply(firstA, observation: hdr))
        let sameDisplay = tracker.capture()
        tracker.outputPathChanged()
        XCTAssertFalse(tracker.apply(sameDisplay, observation: hdr))
        XCTAssertEqual(tracker.snapshot.sourceRevision, 1)
    }

    @MainActor
    func testRepeatedSourceNewPlayerAndDisposalRejectOldResults() {
        let tracker = VesperHdrOutputTracker()
        tracker.sourceChanged()
        let pending = tracker.capture()
        let hdr = VesperHdrOutputObservation(state: .hdr, evidence: "testOutputObserver")
        let replacement = VesperHdrOutputTracker()
        replacement.sourceChanged()
        XCTAssertFalse(replacement.apply(pending, observation: hdr))
        tracker.sourceChanged()
        XCTAssertEqual(tracker.snapshot.sourceRevision, 2)
        XCTAssertFalse(tracker.apply(pending, observation: hdr))
        let atDispose = tracker.capture()
        tracker.dispose()
        XCTAssertFalse(tracker.apply(atDispose, observation: hdr))
        let disposedGeneration = tracker.snapshot.outputGeneration
        tracker.dispose()
        tracker.sourceChanged()
        XCTAssertEqual(tracker.snapshot.outputGeneration, disposedGeneration)
    }

    @MainActor
    func testUnknownAndSdrRequireTheirOwnOutputSemantics() {
        let tracker = VesperHdrOutputTracker()
        XCTAssertEqual(tracker.snapshot.reason, "outputObservationUnavailable")
        XCTAssertFalse(tracker.apply(tracker.capture(), observation: VesperHdrOutputObservation(state: .sdr)))
        XCTAssertTrue(tracker.apply(tracker.capture(), observation: VesperHdrOutputObservation(
            state: .hdr, format: .hdr10, evidence: "testHdrObserver"
        )))
        XCTAssertTrue(tracker.apply(tracker.capture(), observation: VesperHdrOutputObservation(
            state: .sdr, evidence: "testSdrObserver"
        )))
        XCTAssertEqual(tracker.snapshot.state, .sdr)
        XCTAssertEqual(tracker.snapshot.format, .unknown)
        tracker.outputPathChanged()
        XCTAssertEqual(tracker.snapshot.state, .unknown)
        XCTAssertNil(tracker.snapshot.evidence)
    }
}

@MainActor
private final class HdrTestSequenceAttachment: VesperPlaybackSequenceAttachment {
    func onControllerDisposed(_ controller: VesperPlayerController) {}
}
