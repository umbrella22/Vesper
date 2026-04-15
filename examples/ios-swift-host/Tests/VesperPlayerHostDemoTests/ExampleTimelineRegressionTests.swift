import XCTest
@testable import VesperPlayerHostDemo
import VesperPlayerKit

final class ExampleTimelineRegressionTests: XCTestCase {
    func testGoLiveFallsBackToSeekableEndForLiveDvr() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 10_000, endMs: 60_000),
            liveEdgeMs: nil,
            positionMs: 55_000,
            durationMs: 60_000
        )

        XCTAssertEqual(liveButtonState(timeline), .liveBehind(5_000))
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: nil),
            .window(positionMs: 55_000, endMs: 60_000)
        )
    }

    func testLiveEdgeToleranceKeepsLiveBadgeActive() {
        let timeline = TimelineUiState(
            kind: .live,
            isSeekable: false,
            seekableRange: nil,
            liveEdgeMs: 120_000,
            positionMs: 119_100,
            durationMs: nil
        )

        XCTAssertEqual(liveButtonState(timeline), .live)
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: nil),
            .liveEdge(120_000)
        )
    }

    func testPendingRatioIsClampedToSeekableRange() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 30_000, endMs: 90_000),
            liveEdgeMs: 90_000,
            positionMs: 48_000,
            durationMs: 90_000
        )

        XCTAssertEqual(displayedTimelinePositionMs(timeline, pendingSeekRatio: 1.4), 90_000)
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: 1.4),
            .window(positionMs: 90_000, endMs: 90_000)
        )
    }

    func testWindowShrinkClampsStalePositionBeforeRendering() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 40_000, endMs: 70_000),
            liveEdgeMs: nil,
            positionMs: 82_000,
            durationMs: 120_000
        )

        XCTAssertEqual(displayedTimelinePositionMs(timeline, pendingSeekRatio: nil), 70_000)
        XCTAssertEqual(liveButtonState(timeline), .live)
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: nil),
            .window(positionMs: 70_000, endMs: 70_000)
        )
    }
}
