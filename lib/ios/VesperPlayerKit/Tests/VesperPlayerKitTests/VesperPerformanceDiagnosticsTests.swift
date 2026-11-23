import Foundation
@testable import VesperPlayerKit
import XCTest

@MainActor
final class VesperPerformanceDiagnosticsTests: XCTestCase {
    func testRawStringValueObjectsEncodeAsSchemaStringsAndPreserveUnknownValues() throws {
        let probe = VesperPerformanceProbe(rawValue: "futureProbe")
        let encoded = try JSONEncoder().encode(probe)

        XCTAssertEqual(String(decoding: encoded, as: UTF8.self), "\"futureProbe\"")
        XCTAssertEqual(
            try JSONDecoder().decode(VesperPerformanceProbe.self, from: encoded),
            probe
        )
    }

    func testRetiredReportPluginKeepsLocalBenchmarkRecording() async {
        let bridge = FakePlayerBridge(benchmarkConfiguration: .init(enabled: true))

        do {
            _ = try await bridge.startPerformanceDiagnostics(
                configuration: .init(),
                probe: .flutterFrameTiming
            )
            XCTFail("Expected the retired plugin to be unavailable")
        } catch let error as VesperPerformanceDiagnosticsError {
            XCTAssertEqual(error.code, .artifactUnavailable)
            XCTAssertTrue(error.message.contains("retired"))
        } catch {
            XCTFail("Unexpected error: \(error)")
        }

        bridge.play()
        XCTAssertFalse(bridge.drainBenchmarkEvents().isEmpty)
    }
}
