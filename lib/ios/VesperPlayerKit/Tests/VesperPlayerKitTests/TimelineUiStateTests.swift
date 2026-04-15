import XCTest
@testable import VesperPlayerKit

final class TimelineUiStateTests: XCTestCase {
    func testLiveDvrGoLiveFallsBackToSeekableWindowEnd() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 30_000, endMs: 120_000),
            liveEdgeMs: nil,
            positionMs: 90_000,
            durationMs: nil
        )

        XCTAssertEqual(timeline.goLivePositionMs, 120_000)
        XCTAssertEqual(timeline.liveOffsetMs, 30_000)
        XCTAssertEqual(timeline.displayedRatio ?? 0.0, 2.0 / 3.0, accuracy: 0.0001)
    }

    func testLiveDvrOffsetTracksLiveEdgeTolerance() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 30_000, endMs: 120_000),
            liveEdgeMs: 120_000,
            positionMs: 118_800,
            durationMs: nil
        )

        XCTAssertEqual(timeline.liveOffsetMs, 1_200)
        XCTAssertTrue(timeline.isAtLiveEdge())
        XCTAssertFalse(timeline.isAtLiveEdge(toleranceMs: 500))
    }

    func testLiveDvrSliderDragClampsToWindowBounds() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 30_000, endMs: 120_000),
            liveEdgeMs: 120_000,
            positionMs: 90_000,
            durationMs: nil
        )

        XCTAssertEqual(timeline.position(forRatio: -0.25), 30_000)
        XCTAssertEqual(timeline.position(forRatio: 0.5), 75_000)
        XCTAssertEqual(timeline.position(forRatio: 1.5), 120_000)
    }

    func testLiveDvrWindowShrinkClampsStalePositionToNewWindow() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 60_000, endMs: 100_000),
            liveEdgeMs: 100_000,
            positionMs: 120_000,
            durationMs: nil
        )

        XCTAssertEqual(timeline.clampedPosition(timeline.positionMs), 100_000)
        XCTAssertEqual(timeline.liveOffsetMs, 0)
        XCTAssertEqual(timeline.displayedRatio ?? 0.0, 1.0, accuracy: 0.0001)
        XCTAssertTrue(timeline.isAtLiveEdge())
    }
}
