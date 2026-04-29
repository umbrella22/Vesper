import UIKit
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperPlayerControllerStateTests: XCTestCase {
    func testControllerMirrorsBridgeFixedTrackStatusAndResiliencePolicy() async {
        let bridge = TestObservablePlayerBridge()
        let controller = VesperPlayerController(bridge)

        let updatedPolicy = VesperPlaybackResiliencePolicy.resilient()
        bridge.publishedTrackCatalog = sampleTrackCatalog
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )
        bridge.publishedEffectiveVideoTrackId = "video:hls:cavc1:b1500000:w1280:h720:f3000"
        bridge.publishedVideoVariantObservation = VesperVideoVariantObservation(
            bitRate: 1_500_000,
            width: 1280,
            height: 720
        )
        bridge.publishedFixedTrackStatus = .locked
        bridge.publishedResiliencePolicy = updatedPolicy
        bridge.publishedLastError = VesperPlayerError(
            message: "temporary network hiccup",
            category: .network,
            retriable: true
        )
        await settleControllerObservation()

        XCTAssertEqual(controller.trackCatalog, sampleTrackCatalog)
        XCTAssertEqual(
            controller.trackSelection.abrPolicy,
            .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )
        XCTAssertEqual(
            controller.effectiveVideoTrackId,
            "video:hls:cavc1:b1500000:w1280:h720:f3000"
        )
        XCTAssertEqual(
            controller.videoVariantObservation,
            VesperVideoVariantObservation(
                bitRate: 1_500_000,
                width: 1280,
                height: 720
            )
        )
        XCTAssertEqual(controller.fixedTrackStatus, .locked)
        XCTAssertEqual(controller.resiliencePolicy, updatedPolicy)
        XCTAssertEqual(controller.lastError?.category, .network)
        XCTAssertEqual(controller.lastError?.message, "temporary network hiccup")
    }

    func testControllerClearsStaleEffectiveTrackStateAfterSourceReset() async {
        let bridge = TestObservablePlayerBridge()
        let controller = VesperPlayerController(bridge)

        bridge.publishedTrackCatalog = sampleTrackCatalog
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )
        bridge.publishedEffectiveVideoTrackId = "video:hls:cavc1:b1500000:w1280:h720:f3000"
        bridge.publishedVideoVariantObservation = VesperVideoVariantObservation(
            bitRate: 1_500_000,
            width: 1280,
            height: 720
        )
        bridge.publishedFixedTrackStatus = .locked
        await settleControllerObservation()

        bridge.publishedTrackCatalog = .empty
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot()
        bridge.publishedEffectiveVideoTrackId = nil
        bridge.publishedVideoVariantObservation = nil
        bridge.publishedFixedTrackStatus = nil
        bridge.publishedLastError = nil
        await settleControllerObservation()

        XCTAssertEqual(controller.trackCatalog, .empty)
        XCTAssertEqual(controller.trackSelection, VesperTrackSelectionSnapshot())
        XCTAssertNil(controller.effectiveVideoTrackId)
        XCTAssertNil(controller.videoVariantObservation)
        XCTAssertNil(controller.fixedTrackStatus)
        XCTAssertNil(controller.lastError)
    }

    private func settleControllerObservation() async {
        await Task.yield()
        await Task.yield()
    }
}

@MainActor
private final class TestObservablePlayerBridge: ObservableObject, ObservablePlayerBridge {
    @Published var publishedUiState = PlayerHostUiState(
        title: "Test Player",
        subtitle: "Ready",
        sourceLabel: "Test Source",
        playbackState: .ready,
        playbackRate: 1.0,
        isBuffering: false,
        isInterrupted: false,
        timeline: TimelineUiState(
            kind: .vod,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 0, endMs: 60_000),
            liveEdgeMs: nil,
            positionMs: 0,
            durationMs: 60_000
        )
    )
    @Published var publishedTrackCatalog: VesperTrackCatalog = .empty
    @Published var publishedTrackSelection = VesperTrackSelectionSnapshot()
    @Published var publishedEffectiveVideoTrackId: String?
    @Published var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published var publishedResiliencePolicy = VesperPlaybackResiliencePolicy()
    @Published var publishedLastError: VesperPlayerError?

    let backend: PlayerBridgeBackend = .fakeDemo

    var uiState: PlayerHostUiState { publishedUiState }
    var trackCatalog: VesperTrackCatalog { publishedTrackCatalog }
    var trackSelection: VesperTrackSelectionSnapshot { publishedTrackSelection }
    var effectiveVideoTrackId: String? { publishedEffectiveVideoTrackId }
    var videoVariantObservation: VesperVideoVariantObservation? { publishedVideoVariantObservation }
    var fixedTrackStatus: VesperFixedTrackStatus? { publishedFixedTrackStatus }
    var resiliencePolicy: VesperPlaybackResiliencePolicy { publishedResiliencePolicy }
    var lastError: VesperPlayerError? { publishedLastError }

    func initialize() {}
    func dispose() {}
    func refresh() {}
    func selectSource(_ source: VesperPlayerSource) {}
    func attachSurfaceHost(_ host: UIView) {}
    func detachSurfaceHost() {}
    func play() {}
    func pause() {}
    func togglePause() {}
    func stop() {}
    func seek(by deltaMs: Int64) {}
    func seek(toRatio ratio: Double) {}
    func seekToLiveEdge() {}
    func setPlaybackRate(_ rate: Float) {}
    func setVideoTrackSelection(_ selection: VesperTrackSelection) {}
    func setAudioTrackSelection(_ selection: VesperTrackSelection) {}
    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) {}
    func setAbrPolicy(_ policy: VesperAbrPolicy) {}
    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {}
}

private let sampleTrackCatalog = VesperTrackCatalog(
    tracks: [
        VesperMediaTrack(
            id: "video:hls:cavc1:b854000:w854:h480:f3000",
            kind: .video,
            label: "480p",
            codec: "avc1",
            bitRate: 854_000,
            width: 854,
            height: 480,
            frameRate: 30
        ),
        VesperMediaTrack(
            id: "video:hls:cavc1:b1500000:w1280:h720:f3000",
            kind: .video,
            label: "720p",
            codec: "avc1",
            bitRate: 1_500_000,
            width: 1280,
            height: 720,
            frameRate: 30
        ),
    ],
    adaptiveVideo: true,
    adaptiveAudio: false
)
