import AVFoundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperSurfaceFirstFrameRegressionTests: XCTestCase {
    func testReplacementRetainsPrimaryLayerAndRejectsOldInitialReadiness() throws {
        let layer = ControlledReadyPlayerLayer()
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        let firstItem = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let firstPlayer = AVPlayer(playerItem: firstItem)
        var observed: [AVPlayerItem] = []
        surface.onReadyForDisplay = { _, item in observed.append(item) }
        surface.attach(player: firstPlayer)
        layer.setReady(true)
        XCTAssertEqual(observed.count, 1)
        XCTAssertTrue(observed.last === firstItem)
        let primaryLayer = try XCTUnwrap(surface.pictureInPicturePlayerLayer)

        let nextItem = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let nextPlayer = AVPlayer(playerItem: nextItem)
        surface.attach(player: nil)
        surface.attach(player: nextPlayer)
        XCTAssertTrue(surface.pictureInPicturePlayerLayer === primaryLayer,
                      "System PiP retains the primary layer across sources")
        XCTAssertTrue(layer.isReadyForDisplay, "Simulate the retained old picture")
        XCTAssertFalse(surface.isReadyForDisplay)
        XCTAssertNil(surface.geometry)
        XCTAssertEqual(observed.count, 1, "Old initial readiness cannot confirm a replacement")
        layer.setReady(false)
        XCTAssertEqual(observed.count, 1)
        layer.setReady(true)
        XCTAssertEqual(observed.count, 2)
        XCTAssertTrue(observed.last === nextItem)
        XCTAssertTrue(surface.isReadyForDisplay)
        surface.detachBridgeIfNeeded()
        layer.setReady(false)
        layer.setReady(true)
        XCTAssertEqual(observed.count, 2, "A detached surface must stop delivering callbacks")
    }

    func testFirstBindingStillReportsInitialReadinessOnLateAttach() {
        let layer = ControlledReadyPlayerLayer()
        layer.setReady(true)
        let item = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let player = AVPlayer(playerItem: item)
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        var observed: AVPlayerItem?
        surface.onReadyForDisplay = { _, value in observed = value }
        surface.attach(player: player)
        XCTAssertTrue(observed === item)
        XCTAssertTrue(surface.isReadyForDisplay)
        surface.detachBridgeIfNeeded()
    }

    func testReplacingItemOnSamePlayerRequiresNewReadiness() {
        let layer = ControlledReadyPlayerLayer()
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        let oldItem = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let player = AVPlayer(playerItem: oldItem)
        var observed: [AVPlayerItem] = []
        surface.onReadyForDisplay = { _, value in observed.append(value) }
        surface.attach(player: player)
        layer.setReady(true)
        let replacement = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        player.replaceCurrentItem(with: replacement)
        XCTAssertFalse(surface.isReadyForDisplay, "Item identity fences the old binding")
        surface.attach(player: player)
        XCTAssertTrue(surface.pictureInPicturePlayerLayer === layer)
        XCTAssertEqual(observed.count, 1)
        layer.setReady(false)
        layer.setReady(true)
        XCTAssertEqual(observed.count, 2)
        XCTAssertTrue(observed.last === replacement)
        surface.detachBridgeIfNeeded()
    }

    func testInstallingCallbackAfterSameBindingBecameReadyStillDelivers() {
        let layer = ControlledReadyPlayerLayer()
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        let item = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let player = AVPlayer(playerItem: item)
        surface.attach(player: player)
        layer.setReady(true)
        var observed: AVPlayerItem?
        surface.onReadyForDisplay = { _, value in observed = value }
        surface.attach(player: player)
        XCTAssertTrue(observed === item)
        XCTAssertTrue(surface.pictureInPicturePlayerLayer === layer)
        surface.detachBridgeIfNeeded()
    }

    func testReattachingConfirmedSameItemAllowsRetainedReadyPicture() {
        let layer = ControlledReadyPlayerLayer()
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        let item = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let player = AVPlayer(playerItem: item)
        surface.attach(player: player)
        layer.setReady(true)
        surface.attach(player: nil)
        var observed: AVPlayerItem?
        surface.onReadyForDisplay = { _, value in observed = value }
        surface.attach(player: player)
        XCTAssertTrue(observed === item)
        XCTAssertTrue(surface.pictureInPicturePlayerLayer === layer)
        surface.detachBridgeIfNeeded()
    }

    func testReattachingUnconfirmedReplacementCannotReuseOldReadyPicture() {
        let layer = ControlledReadyPlayerLayer()
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        let oldPlayer = AVPlayer(playerItem: AVPlayerItem(url: URL(fileURLWithPath: "/dev/null")))
        surface.attach(player: oldPlayer)
        layer.setReady(true)
        let item = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let player = AVPlayer(playerItem: item)
        var observed: AVPlayerItem?
        surface.onReadyForDisplay = { _, value in observed = value }
        surface.attach(player: player)
        surface.attach(player: nil)
        surface.attach(player: player)
        XCTAssertNil(observed)
        XCTAssertFalse(surface.isReadyForDisplay)
        layer.setReady(false)
        layer.setReady(true)
        XCTAssertTrue(observed === item)
        surface.detachBridgeIfNeeded()
    }

    func testReadinessResetWhileDetachedAllowsLaterCurrentBinding() {
        let layer = ControlledReadyPlayerLayer()
        let surface = PlayerSurfaceView(frame: .zero, playerLayerFactory: { layer })
        let firstPlayer = AVPlayer(playerItem: AVPlayerItem(url: URL(fileURLWithPath: "/dev/null")))
        surface.attach(player: firstPlayer)
        layer.setReady(true)
        surface.attach(player: nil)
        layer.setReady(false)
        let nextItem = AVPlayerItem(url: URL(fileURLWithPath: "/dev/null"))
        let nextPlayer = AVPlayer(playerItem: nextItem)
        var observed: AVPlayerItem?
        surface.onReadyForDisplay = { _, value in observed = value }
        surface.attach(player: nextPlayer)
        layer.setReady(true)
        XCTAssertTrue(observed === nextItem)
        surface.detachBridgeIfNeeded()
    }
}

private final class ControlledReadyPlayerLayer: AVPlayerLayer {
    private var ready = false
    override var isReadyForDisplay: Bool { ready }
    func setReady(_ value: Bool) {
        willChangeValue(forKey: #keyPath(AVPlayerLayer.isReadyForDisplay))
        ready = value
        didChangeValue(forKey: #keyPath(AVPlayerLayer.isReadyForDisplay))
    }
}
