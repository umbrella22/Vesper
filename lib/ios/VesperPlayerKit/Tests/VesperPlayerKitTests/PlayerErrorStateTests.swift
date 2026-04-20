import XCTest
@testable import VesperPlayerKit

@MainActor
final class PlayerErrorStateTests: XCTestCase {
    func testNativeBridgeReportsUnsupportedVideoTrackSelection() {
        let bridge = VesperNativePlayerBridge()

        bridge.setVideoTrackSelection(.track("video:0"))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setVideoTrackSelection is not implemented on iOS AVPlayer (mode=track, trackId=video:0)"
        )
    }

    func testNativeBridgeReportsUnsupportedFixedTrackAbrWithoutCurrentItem() {
        let bridge = VesperNativePlayerBridge()

        bridge.setAbrPolicy(.fixedTrack("video:0"))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy fixedTrack is not implemented on iOS AVPlayer"
        )
    }
}
