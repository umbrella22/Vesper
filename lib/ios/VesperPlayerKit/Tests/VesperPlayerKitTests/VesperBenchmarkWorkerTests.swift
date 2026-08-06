import Foundation
@testable import VesperPlayerKit
import XCTest

@MainActor
final class VesperBenchmarkWorkerTests: XCTestCase {
    func testReportPayloadDecodesStructuredFieldsAndUnknownSeverity() throws {
        let json = """
            {
              "acceptedEvents": 3,
              "droppedEvents": 1,
              "measurements": [
                {"name":"startup","value":12.5,"unit":"ms","attributes":{"phase":"ready"}}
              ],
              "thresholdViolations": [
                {"measurement":"startup","actual":12.5,"threshold":10.0,"comparison":"lessThan"}
              ],
              "diagnostics": [
                {"code":"sink.notice","severity":"futureSeverity","message":"kept","attributes":{}}
              ]
            }
            """

        let report = try JSONDecoder().decode(
            VesperBenchmarkSinkReportPayload.self,
            from: Data(json.utf8)
        )

        XCTAssertEqual(report.acceptedEvents, 3)
        XCTAssertEqual(report.droppedEvents, 1)
        XCTAssertEqual(report.measurements.first?.name, "startup")
        XCTAssertEqual(report.thresholdViolations.first?.threshold, 10.0)
        XCTAssertEqual(report.diagnostics.first?.severity.rawValue, "futureSeverity")
    }

    func testPluginLifecycleRunsOffMainActorInOrder() async throws {
        let probe = BenchmarkWorkerProbe()
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )

        recorder.record("play-command", sourceProtocol: .hls)
        recorder.dispose()
        let completed = await recorder.awaitSinkShutdown(timeout: 2)
        XCTAssertTrue(completed)
        await fulfillment(of: [probe.disposed], timeout: 2)

        let operations = probe.operationsSnapshot()
        XCTAssertEqual(operations.map(\.name), ["open", "submit:play-command", "flush", "dispose"])
        XCTAssertTrue(operations.allSatisfy { !$0.wasMainThread })
        XCTAssertEqual(recorder.summary().pluginAcceptedEvents, 1)
        XCTAssertEqual(recorder.summary().pluginDroppedEvents, 0)
        XCTAssertTrue(recorder.summary().pluginErrors.isEmpty)
    }

    func testFullQueueDropsIncomingEventWithoutReplacingPendingTail() async throws {
        let probe = BenchmarkWorkerProbe(blockOpen: true, blockFirstSubmit: true)
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )
        await fulfillment(of: [probe.openStarted], timeout: 2)

        for index in 0...1_024 {
            recorder.record("event-\(index)", sourceProtocol: nil)
        }
        XCTAssertEqual(recorder.summary().pluginDroppedEvents, 1)

        probe.allowOpen.signal()
        await fulfillment(of: [probe.firstSubmitStarted], timeout: 2)
        recorder.dispose()
        probe.allowFirstSubmit.signal()
        await fulfillment(of: [probe.disposed], timeout: 5)

        let submittedNames = probe.submittedEventNamesSnapshot()
        XCTAssertEqual(submittedNames.count, 1_024)
        XCTAssertTrue(submittedNames.contains("event-1023"))
        XCTAssertFalse(submittedNames.contains("event-1024"))
        XCTAssertEqual(recorder.summary().pluginAcceptedEvents, 1_024)
        XCTAssertEqual(recorder.summary().pluginDroppedEvents, 1)
        XCTAssertEqual(
            probe.operationsSnapshot().suffix(2).map(\.name),
            ["flush", "dispose"]
        )
    }

    func testFinalFlushReportIsNotAddedToSubmitAcknowledgements() async throws {
        let finalReport = VesperBenchmarkSinkReport(
            acceptedEvents: 3,
            droppedEvents: 2,
            measurements: [
                VesperPluginMeasurement(name: "startup", value: 12.5, unit: "ms")
            ],
            thresholdViolations: [
                VesperBenchmarkThresholdViolation(
                    measurement: "startup",
                    actual: 12.5,
                    threshold: 10,
                    comparison: "lessThan"
                )
            ],
            diagnostics: [
                VesperPluginDiagnostic(
                    code: "sink.final",
                    severity: .warning,
                    message: "threshold exceeded"
                )
            ]
        )
        let probe = BenchmarkWorkerProbe(finalReport: finalReport)
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )

        recorder.record("play-command", sourceProtocol: .hls)
        recorder.dispose()
        await fulfillment(of: [probe.disposed], timeout: 2)

        let summary = recorder.summary()
        XCTAssertEqual(summary.pluginAcceptedEvents, 1)
        XCTAssertEqual(summary.pluginDroppedEvents, 0)
        XCTAssertEqual(summary.pluginFinalReport, finalReport)
    }

    func testDuplicateDisposeAndRecordAfterDisposeAreIgnored() async throws {
        let probe = BenchmarkWorkerProbe()
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )

        recorder.record("before-dispose", sourceProtocol: nil)
        recorder.dispose()
        recorder.dispose()
        recorder.record("after-dispose", sourceProtocol: nil)
        let completed = await awaitShutdown(of: recorder, timeout: 2)
        XCTAssertTrue(completed)

        XCTAssertEqual(recorder.summary().acceptedEvents, 1)
        XCTAssertEqual(probe.submittedEventNamesSnapshot(), ["before-dispose"])
        XCTAssertEqual(
            probe.operationsSnapshot().filter { $0.name == "dispose" }.count,
            1
        )
    }

    func testOpenFailureCompletesShutdownAndReportsError() async throws {
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { _ in throw BenchmarkWorkerProbeError.open }
        )

        recorder.record("ignored-by-missing-session", sourceProtocol: nil)
        recorder.dispose()
        let completed = await awaitShutdown(of: recorder, timeout: 2)
        XCTAssertTrue(completed)

        XCTAssertEqual(recorder.summary().pluginErrors, ["benchmark sink open failed"])
    }

    func testFlushFailureStillDisposesSessionOnce() async throws {
        let probe = BenchmarkWorkerProbe(failFlush: true)
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )

        recorder.record("event", sourceProtocol: nil)
        recorder.dispose()
        let completed = await awaitShutdown(of: recorder, timeout: 2)
        XCTAssertTrue(completed)

        XCTAssertEqual(recorder.summary().pluginErrors, ["benchmark sink flush failed"])
        XCTAssertEqual(
            probe.operationsSnapshot().filter { $0.name == "dispose" }.count,
            1
        )
    }

    func testErrorHistoryKeepsExactlyNewest128Entries() async throws {
        let probe = BenchmarkWorkerProbe(failSubmits: true)
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )

        for index in 0..<130 {
            recorder.record("event-\(index)", sourceProtocol: nil)
        }
        recorder.dispose()
        let completed = await awaitShutdown(of: recorder, timeout: 2)
        XCTAssertTrue(completed)

        let errors = recorder.summary().pluginErrors
        XCTAssertEqual(errors.count, 128)
        XCTAssertEqual(errors.first, "benchmark sink submit event-2 failed")
        XCTAssertEqual(errors.last, "benchmark sink submit event-129 failed")
    }

    func testShutdownTimeoutIsBoundedAndRecordedOnce() async throws {
        let probe = BenchmarkWorkerProbe(blockOpen: true)
        let recorder = VesperBenchmarkRecorder(
            configuration: try configuration(),
            sinkSessionFactory: { references in probe.makeSession(references) }
        )
        await fulfillment(of: [probe.openStarted], timeout: 2)

        recorder.dispose()
        let firstAttempt = await awaitShutdown(of: recorder, timeout: 0.01)
        let secondAttempt = await awaitShutdown(of: recorder, timeout: 0.01)
        XCTAssertFalse(firstAttempt)
        XCTAssertFalse(secondAttempt)
        XCTAssertEqual(recorder.summary().pluginErrors, ["benchmark sink shutdown timed out"])

        probe.allowOpen.signal()
        let completed = await awaitShutdown(of: recorder, timeout: 2)
        XCTAssertTrue(completed)
    }

    private func configuration() throws -> VesperBenchmarkConfiguration {
        VesperBenchmarkConfiguration(
            enabled: true,
            pluginReferences: [
                try VesperPluginReference(
                    pluginId: "dev.vesper.benchmark-sink",
                    capabilityInstanceId: "dev.vesper.benchmark-sink.default",
                    transport: .native
                )
            ]
        )
    }

    private func awaitShutdown(
        of recorder: VesperBenchmarkRecorder,
        timeout: TimeInterval
    ) async -> Bool {
        await recorder.awaitSinkShutdown(timeout: timeout)
    }
}

private enum BenchmarkWorkerProbeError: LocalizedError, Sendable {
    case open
    case submit(String)
    case flush

    var errorDescription: String? {
        switch self {
        case .open:
            "benchmark sink open failed"
        case let .submit(eventName):
            "benchmark sink submit \(eventName) failed"
        case .flush:
            "benchmark sink flush failed"
        }
    }
}

private final class BenchmarkWorkerProbe: @unchecked Sendable {
    struct Operation {
        let name: String
        let wasMainThread: Bool
    }

    let openStarted = XCTestExpectation(description: "benchmark sink open started")
    let firstSubmitStarted = XCTestExpectation(description: "benchmark first submit started")
    let disposed = XCTestExpectation(description: "benchmark sink disposed")
    let allowOpen = DispatchSemaphore(value: 0)
    let allowFirstSubmit = DispatchSemaphore(value: 0)

    private let lock = NSLock()
    private let blockOpen: Bool
    private let blockFirstSubmit: Bool
    private let failSubmits: Bool
    private let failFlush: Bool
    private let finalReport: VesperBenchmarkSinkReport
    private var isFirstSubmit = true
    private var operations: [Operation] = []
    private var submittedEventNames: [String] = []

    init(
        blockOpen: Bool = false,
        blockFirstSubmit: Bool = false,
        failSubmits: Bool = false,
        failFlush: Bool = false,
        finalReport: VesperBenchmarkSinkReport = VesperBenchmarkSinkReport(
            acceptedEvents: 0,
            droppedEvents: 0
        )
    ) {
        self.blockOpen = blockOpen
        self.blockFirstSubmit = blockFirstSubmit
        self.failSubmits = failSubmits
        self.failFlush = failFlush
        self.finalReport = finalReport
    }

    func makeSession(
        _ references: [VesperPluginReference]
    ) -> any VesperBenchmarkSinkSessionProtocol {
        precondition(!references.isEmpty)
        recordOperation("open")
        openStarted.fulfill()
        if blockOpen {
            _ = allowOpen.wait(timeout: .now() + 2)
        }
        return BenchmarkWorkerProbeSession(probe: self)
    }

    func submit(_ events: [VesperBenchmarkEvent]) throws -> VesperBenchmarkSinkReportPayload {
        let eventName = events.first?.eventName ?? "missing"
        lock.lock()
        submittedEventNames.append(eventName)
        operations.append(Operation(name: "submit:\(eventName)", wasMainThread: Thread.isMainThread))
        let shouldBlock = blockFirstSubmit && isFirstSubmit
        isFirstSubmit = false
        lock.unlock()

        if shouldBlock {
            firstSubmitStarted.fulfill()
            _ = allowFirstSubmit.wait(timeout: .now() + 2)
        }
        if failSubmits {
            throw BenchmarkWorkerProbeError.submit(eventName)
        }
        return VesperBenchmarkSinkReportPayload(
            acceptedEvents: UInt64(events.count),
            droppedEvents: 0
        )
    }

    func flush() throws -> VesperBenchmarkSinkReportPayload {
        recordOperation("flush")
        if failFlush {
            throw BenchmarkWorkerProbeError.flush
        }
        return finalReport
    }

    func dispose() {
        recordOperation("dispose")
        disposed.fulfill()
    }

    func operationsSnapshot() -> [Operation] {
        lock.lock()
        defer { lock.unlock() }
        return operations
    }

    func submittedEventNamesSnapshot() -> [String] {
        lock.lock()
        defer { lock.unlock() }
        return submittedEventNames
    }

    private func recordOperation(_ name: String) {
        lock.lock()
        operations.append(Operation(name: name, wasMainThread: Thread.isMainThread))
        lock.unlock()
    }
}

private final class BenchmarkWorkerProbeSession: VesperBenchmarkSinkSessionProtocol,
    @unchecked Sendable
{
    private let probe: BenchmarkWorkerProbe

    init(probe: BenchmarkWorkerProbe) {
        self.probe = probe
    }

    func submit(_ events: [VesperBenchmarkEvent]) throws -> VesperBenchmarkSinkReportPayload {
        try probe.submit(events)
    }

    func flush() throws -> VesperBenchmarkSinkReportPayload {
        try probe.flush()
    }

    func dispose() {
        probe.dispose()
    }
}
