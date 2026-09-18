import AVFoundation
import XCTest
@testable import VesperPlayerKit

final class VesperVideoPresentationTests: XCTestCase {
    func testNativeDisplayDimensionsAndUnknownValues() {
        XCTAssertEqual(VesperVideoPresentation(size: CGSize(width: 1080, height: 1920))?.displayAspectRatio, 9.0 / 16)
        XCTAssertEqual(VesperVideoPresentation(size: CGSize(width: 768, height: 576))?.displayAspectRatio, 4.0 / 3)
        XCTAssertNil(VesperVideoPresentation(size: .zero))
        XCTAssertNil(VesperVideoPresentation(size: CGSize(width: CGFloat.infinity, height: 10)))
    }

    @MainActor
    func testControllerPublishesDimensionsAndClearsThemOnSourceTeardown() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        var values: [VesperVideoPresentation?] = []
        var changes = 0
        let dimensions = controller.videoPresentationPublisher.sink { values.append($0) }
        let change = controller.objectWillChange.sink { changes += 1 }
        let portrait = VesperVideoPresentation(size: CGSize(width: 1080, height: 1920))
        bridge.presentationState.update(portrait)
        bridge.presentationState.update(portrait)
        XCTAssertEqual(values, [nil, portrait])
        XCTAssertEqual(changes, 1)
        XCTAssertEqual(controller.videoPresentation, portrait)
        bridge.tearDownActivePlayback()
        XCTAssertNil(controller.videoPresentation)
        XCTAssertNil(values.last!)
        dimensions.cancel()
        change.cancel()
    }

    @MainActor
    func testRebindingDetachesThePreviousLayerWithoutDetachingTheReplacement() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        bridge.player = AVPlayer()
        let first = PlayerSurfaceView()
        let second = PlayerSurfaceView()
        controller.attachSurfaceHost(first)
        XCTAssertNotNil(first.pictureInPicturePlayerLayer)
        controller.attachSurfaceHost(second)
        XCTAssertNil(first.pictureInPicturePlayerLayer)
        XCTAssertNil(first.geometry)
        XCTAssertNotNil(second.pictureInPicturePlayerLayer)
        controller.detachSurfaceHost(first)
        XCTAssertNotNil(second.pictureInPicturePlayerLayer)
    }

    @MainActor
    func testClosingFullscreenRestoresTheRetainedSurfaceContainer() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        bridge.player = AVPlayer()
        let inline = PlayerSurfaceView()
        let fullscreen = PlayerSurfaceView()
        let inlineCoordinator = PlayerSurfaceContainer(controller: controller).makeCoordinator()
        let fullscreenCoordinator = PlayerSurfaceContainer(controller: controller).makeCoordinator()

        inlineCoordinator.attach(controller: controller, view: inline)
        fullscreenCoordinator.attach(controller: controller, view: fullscreen)
        XCTAssertNil(inline.pictureInPicturePlayerLayer)
        XCTAssertTrue(bridge.surfaceHost === fullscreen)

        fullscreenCoordinator.detach(view: fullscreen)

        XCTAssertTrue(bridge.surfaceHost === inline)
        XCTAssertTrue(inline.pictureInPicturePlayerLayer?.player === bridge.player)
        XCTAssertNil(fullscreen.pictureInPicturePlayerLayer)
    }

    @MainActor
    func testRemovingSuspendedContainerDoesNotStealPlaybackOrRestoreItLater() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        bridge.player = AVPlayer()
        let inline = PlayerSurfaceView()
        let fullscreen = PlayerSurfaceView()
        let inlineCoordinator = PlayerSurfaceContainer(controller: controller).makeCoordinator()
        let fullscreenCoordinator = PlayerSurfaceContainer(controller: controller).makeCoordinator()
        inlineCoordinator.attach(controller: controller, view: inline)
        fullscreenCoordinator.attach(controller: controller, view: fullscreen)

        XCTAssertTrue(inlineCoordinator.isRegistered(controller: controller, view: inline))
        XCTAssertTrue(bridge.surfaceHost === fullscreen)
        inlineCoordinator.detach(view: inline)
        XCTAssertTrue(bridge.surfaceHost === fullscreen)
        XCTAssertNotNil(fullscreen.pictureInPicturePlayerLayer)

        fullscreenCoordinator.detach(view: fullscreen)
        XCTAssertNil(bridge.surfaceHost)
        XCTAssertNil(inline.pictureInPicturePlayerLayer)
    }

    @MainActor
    func testContainerDisposalDoesNotReplaceAnExplicitlyAttachedHost() {
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose() }
        bridge.player = AVPlayer()
        let inline = PlayerSurfaceView()
        let explicitHost = PlayerSurfaceView()
        let coordinator = PlayerSurfaceContainer(controller: controller).makeCoordinator()
        coordinator.attach(controller: controller, view: inline)
        controller.attachSurfaceHost(explicitHost)

        coordinator.detach(view: inline)

        XCTAssertTrue(bridge.surfaceHost === explicitHost)
        XCTAssertNotNil(explicitHost.pictureInPicturePlayerLayer)
    }
}
