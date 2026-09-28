import XCTest
@testable import VesperPlayerKit

final class VesperDashStartupPlaybackDeviceTests: XCTestCase {
    @MainActor
    func testAVPlayerConsumesWarmSegmentsThenContinuesWithUpstreamRanges() async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("The production DASH route requires device hardware decode capabilities")
#else
        let result = try await runDashStartupPlaybackScenario()
        XCTAssertTrue(result.firstFrame, "AVPlayer did not display the warmed fMP4 bytes")
        XCTAssertGreaterThan(result.position, 3, "Playback did not continue into upstream ranges: \(result.state)")
        XCTAssertEqual(result.warmRequests, 4)
        XCTAssertGreaterThan(result.originRequests, 0)
        let evidence = XCTAttachment(data: try JSONEncoder().encode(result), uniformTypeIdentifier: "public.json")
        evidence.lifetime = .keepAlways
        add(evidence)
#endif
    }
}
