import XCTest
@testable import VesperPlayerKit

final class VesperSystemPlaybackControlsTests: XCTestCase {
    func testVideoDefaultUsesTenSecondSeekButtons() {
        let controls = VesperSystemPlaybackControls.videoDefault().normalized()

        XCTAssertEqual(
            controls.compactButtons.map(\.kind),
            [.seekBack, .playPause, .seekForward]
        )
        XCTAssertEqual(controls.seekOffsetMs(for: .seekBack), 10_000)
        XCTAssertEqual(controls.seekOffsetMs(for: .seekForward), 10_000)
    }

    func testCompactButtonsClampOffsetsAndForceCenterPlayPause() {
        let controls = VesperSystemPlaybackControls(
            compactButtons: [
                .seekBack(500),
                .seekForward(15_000),
                .seekForward(90_000),
            ]
        ).normalized()

        XCTAssertEqual(controls.compactButtons[1].kind, .playPause)
        XCTAssertEqual(controls.seekOffsetMs(for: .seekBack), 1_000)
        XCTAssertEqual(controls.seekOffsetMs(for: .seekForward), 60_000)
    }

    func testDisabledSeekLeavesOnlyPlayPauseControl() {
        let controls = VesperSystemPlaybackControls.videoDefault().normalized(showSeekActions: false)

        XCTAssertEqual(controls.compactButtons.map(\.kind), [.playPause])
        XCTAssertNil(controls.seekOffsetMs(for: .seekBack))
        XCTAssertNil(controls.seekOffsetMs(for: .seekForward))
    }
}
