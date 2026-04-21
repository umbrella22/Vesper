import XCTest
@testable import VesperPlayerKit

@MainActor
final class PlayerErrorStateTests: XCTestCase {
    func testNativeBridgeReportsUnsupportedVideoTrackSelection() {
        let bridge = VesperNativePlayerBridge()
        let missingTrackId = "video:missing"

        bridge.setVideoTrackSelection(.track(missingTrackId))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setVideoTrackSelection is not implemented on iOS AVPlayer (mode=track, trackId=\(missingTrackId))"
        )
    }

    func testNativeBridgeReportsUnsupportedFixedTrackAbrWithoutCurrentCatalog() {
        let bridge = VesperNativePlayerBridge()
        let missingTrackId = "video:missing"

        bridge.setAbrPolicy(.fixedTrack(missingTrackId))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy fixedTrack requires a video variant from the current iOS track catalog (trackId=\(missingTrackId))"
        )
    }

    func testNativeBridgeReportsUnsupportedSingleAxisConstrainedAbrWithoutCurrentCatalog() {
        let bridge = VesperNativePlayerBridge()

        bridge.setAbrPolicy(.constrained(maxHeight: 720))

        XCTAssertEqual(bridge.lastError?.category, .unsupported)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy constrained mode requires a loaded HLS variant catalog to infer a single-axis maxWidth/maxHeight limit on iOS"
        )
    }
}
