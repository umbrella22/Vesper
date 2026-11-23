@testable import VesperPlayerKit
import XCTest

@MainActor
final class VesperBenchmarkRecorderTests: XCTestCase {
    func testRecorderBoundsEventsAndRejectsRecordingAfterDispose() async throws {
        let recorder = VesperBenchmarkRecorder(configuration: .init(
            enabled: true,
            maxBufferedEvents: 1,
            pluginReferences: [try .init(pluginId: "dev.vesper.retired", transport: .native)]
        ))
        recorder.record("play_command", sourceProtocol: nil)
        recorder.record("play_command", sourceProtocol: nil)
        recorder.dispose()
        recorder.record("pause_command", sourceProtocol: nil)

        let summary = recorder.summary()
        XCTAssertEqual(recorder.drainEvents().map(\.eventName), ["play_command"])
        XCTAssertEqual(summary.acceptedEvents, 2)
        XCTAssertEqual(summary.droppedEvents, 1)
        XCTAssertEqual(summary.pluginAcceptedEvents, 0)
        XCTAssertTrue(summary.pluginErrors.first?.contains("retired") ?? false)
    }
}
