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

    func testQualityHelpersExposeFixedTrackStateAndObservation() {
        let trackCatalog = VesperTrackCatalog(
            tracks: [
                VesperMediaTrack(
                    id: "video:hls:cavc1:b854000:w854:h480:f3000",
                    kind: .video,
                    bitRate: 854_000,
                    width: 854,
                    height: 480,
                    frameRate: 30
                ),
                VesperMediaTrack(
                    id: "video:hls:cavc1:b1500000:w1280:h720:f3000",
                    kind: .video,
                    bitRate: 1_500_000,
                    width: 1280,
                    height: 720,
                    frameRate: 30
                ),
            ],
            adaptiveVideo: true,
            adaptiveAudio: false
        )
        let trackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )

        XCTAssertEqual(
            currentFixedTrackStatus(
                trackCatalog,
                trackSelection,
                effectiveVideoTrackId: "video:hls:cavc1:b854000:w854:h480:f3000",
                fixedTrackStatus: .fallback
            ),
            .fallback
        )
        XCTAssertEqual(
            qualityOptionBadgeLabel(
                trackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
                trackCatalog: trackCatalog,
                trackSelection: trackSelection,
                effectiveVideoTrackId: "video:hls:cavc1:b854000:w854:h480:f3000",
                fixedTrackStatus: .fallback
            ),
            ExampleI18n.qualityStatusFallback
        )
        XCTAssertEqual(
            videoVariantObservationSummary(
                VesperVideoVariantObservation(
                    bitRate: 854_000,
                    width: 854,
                    height: 480
                )
            ),
            "854x480 · 854 kbps"
        )
    }

    func testQualityHelpersKeepFixedTrackPendingWhileRuntimeEvidenceSettles() {
        let requestedTrackId = "video:hls:cavc1:b1500000:w1280:h720:f3000"
        let observedTrackId = "video:hls:cavc1:b854000:w854:h480:f3000"
        let trackCatalog = VesperTrackCatalog(
            tracks: [
                VesperMediaTrack(
                    id: observedTrackId,
                    kind: .video,
                    bitRate: 854_000,
                    width: 854,
                    height: 480,
                    frameRate: 30
                ),
                VesperMediaTrack(
                    id: requestedTrackId,
                    kind: .video,
                    bitRate: 1_500_000,
                    width: 1280,
                    height: 720,
                    frameRate: 30
                ),
            ],
            adaptiveVideo: true,
            adaptiveAudio: false
        )
        let trackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack(requestedTrackId)
        )

        XCTAssertEqual(
            currentFixedTrackStatus(
                trackCatalog,
                trackSelection,
                effectiveVideoTrackId: observedTrackId,
                fixedTrackStatus: .pending
            ),
            .pending
        )
        XCTAssertEqual(
            qualityOptionBadgeLabel(
                trackId: requestedTrackId,
                trackCatalog: trackCatalog,
                trackSelection: trackSelection,
                effectiveVideoTrackId: observedTrackId,
                fixedTrackStatus: .pending
            ),
            ExampleI18n.qualityStatusPending
        )
        XCTAssertEqual(
            qualityAutoRowSubtitle(
                trackCatalog,
                trackSelection,
                effectiveVideoTrackId: observedTrackId,
                fixedTrackStatus: .pending,
                videoVariantObservation: nil
            ),
            ExampleI18n.qualityFixedSubtitlePending("720p")
        )
    }
}
