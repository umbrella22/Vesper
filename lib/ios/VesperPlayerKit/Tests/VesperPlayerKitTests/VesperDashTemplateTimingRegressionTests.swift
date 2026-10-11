import XCTest
@testable import VesperPlayerKit

final class VesperDashTemplateTimingRegressionTests: XCTestCase {
    func testFixedTemplateExactDurationPreservesNumberCountAcrossRustBridge() throws {
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: 6_003,
            segmentTemplate: template()
        )

        XCTAssertEqual(segments.map(\.number), [1, 2, 3])
        XCTAssertTrue(segments.allSatisfy { $0.time == nil })
        for segment in segments {
            XCTAssertEqual(segment.duration, 2.001, accuracy: 0.000_001)
        }
    }

    func testFixedTemplatePartialTailPreservesNumberIdentityAcrossRustBridge() throws {
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: 6_004,
            segmentTemplate: template()
        )

        XCTAssertEqual(segments.map(\.number), [1, 2, 3, 4])
        XCTAssertTrue(segments.allSatisfy { $0.time == nil })
        for segment in segments.prefix(3) {
            XCTAssertEqual(segment.duration, 2.001, accuracy: 0.000_001)
        }
        XCTAssertEqual(try XCTUnwrap(segments.last).duration, 0.001, accuracy: 0.000_001)
    }

    private func template() -> VesperDashSegmentTemplate {
        VesperDashSegmentTemplate(
            timescale: 1_000,
            duration: 2_001,
            startNumber: 1,
            presentationTimeOffset: 0,
            initialization: "init.mp4",
            media: "segment-$Number$.m4s",
            timeline: []
        )
    }
}
