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

    func testInvalidConfigurationDoesNotAllocateRecorder() async {
        var recorderCreated = false
        let coordinator = makeCoordinator {
            recorderCreated = true
            return TestPerformanceRecorder()
        }

        await assertDiagnosticsError(.invalidConfiguration) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(maxRawEvents: 2_049),
                probe: .flutterFrameTiming,
                initialPlaybackActive: false
            )
        }

        XCTAssertFalse(recorderCreated)
        XCTAssertFalse(coordinator.isEnabled)
    }

    func testUnavailableArtifactDoesNotAllocateRecorder() async {
        var recorderCreated = false
        let coordinator = VesperBenchmarkCoordinator(
            artifactValidator: { throw TestDiagnosticsFailure.unavailable },
            recorderFactory: { _ in
                recorderCreated = true
                return TestPerformanceRecorder()
            },
            frameProbeFactory: { _, _ in nil }
        )

        await assertDiagnosticsError(.artifactUnavailable) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .flutterFrameTiming,
                initialPlaybackActive: false
            )
        }

        XCTAssertFalse(recorderCreated)
        XCTAssertFalse(coordinator.isEnabled)
    }

    func testSinkOpenFailureReturnsArtifactUnavailableBeforeRegisteringFrameProbe() async {
        var recorder: VesperBenchmarkRecorder?
        var frameProbeCreated = false
        let coordinator = VesperBenchmarkCoordinator(
            artifactValidator: {},
            recorderFactory: { configuration in
                let created = VesperBenchmarkRecorder(
                    configuration: try self.benchmarkConfiguration(for: configuration),
                    sinkSessionFactory: { _ in throw TestDiagnosticsFailure.unavailable }
                )
                recorder = created
                return created
            },
            frameProbeFactory: { _, _ in
                frameProbeCreated = true
                return TestPerformanceFrameProbe()
            }
        )

        await assertDiagnosticsError(.artifactUnavailable) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .iosDisplayLink,
                initialPlaybackActive: false
            )
        }

        XCTAssertFalse(frameProbeCreated)
        XCTAssertFalse(coordinator.isEnabled)
        let createdRecorder = try? XCTUnwrap(recorder)
        let readiness = await createdRecorder?.awaitSinkReadiness(timeout: 0)
        let didShutdown = await createdRecorder?.awaitSinkShutdown(timeout: 1)
        XCTAssertEqual(readiness, .openFailed)
        XCTAssertEqual(didShutdown, true)
    }

    func testSinkReadinessTimeoutReturnsInternalFailureAndCleansUpBeforeReturning() async {
        let session = TestBenchmarkSinkSession()
        let allowOpen = DispatchSemaphore(value: 0)
        var recorder: VesperBenchmarkRecorder?
        var frameProbeCreated = false
        let coordinator = VesperBenchmarkCoordinator(
            artifactValidator: {},
            recorderFactory: { configuration in
                let created = VesperBenchmarkRecorder(
                    configuration: try self.benchmarkConfiguration(for: configuration),
                    sinkSessionFactory: { _ in
                        _ = allowOpen.wait(timeout: .now() + 2)
                        return session
                    }
                )
                recorder = created
                return created
            },
            frameProbeFactory: { _, _ in
                frameProbeCreated = true
                return TestPerformanceFrameProbe()
            },
            performanceStartTimeout: 0.01
        )
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 0.05) {
            allowOpen.signal()
        }

        await assertDiagnosticsError(.internalFailure) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .iosDisplayLink,
                initialPlaybackActive: false
            )
        }

        XCTAssertFalse(frameProbeCreated)
        XCTAssertFalse(coordinator.isEnabled)
        let createdRecorder = try? XCTUnwrap(recorder)
        let didShutdown = await createdRecorder?.awaitSinkShutdown(timeout: 1)
        XCTAssertEqual(didShutdown, true)
        XCTAssertEqual(session.disposeCount, 1)
    }

    func testProbeConstructionFailureDisposesRecorderAndClearsRun() async {
        let recorder = TestPerformanceRecorder()
        let coordinator = VesperBenchmarkCoordinator(
            artifactValidator: {},
            recorderFactory: { _ in recorder },
            frameProbeFactory: { _, _ in throw TestDiagnosticsFailure.unavailable }
        )

        await assertDiagnosticsError(.internalFailure) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .iosDisplayLink,
                initialPlaybackActive: false
            )
        }

        XCTAssertTrue(recorder.disposed)
        XCTAssertTrue(recorder.awaitedShutdown)
        XCTAssertFalse(coordinator.isEnabled)
    }

    func testSecondStartReturnsAlreadyActive() async throws {
        let coordinator = makeCoordinator { TestPerformanceRecorder() }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )

        await assertDiagnosticsError(.alreadyActive) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .flutterFrameTiming,
                initialPlaybackActive: false
            )
        }
        _ = try await coordinator.stop(runId: runId, player: nil)
    }

    func testDisposeDuringStartReturnsControllerDisposedAndFinalizesOnce() async throws {
        let recorder = TestPerformanceRecorder(blockReadiness: true)
        var frameProbeCreated = false
        let coordinator = VesperBenchmarkCoordinator(
            artifactValidator: {},
            recorderFactory: { _ in recorder },
            frameProbeFactory: { _, _ in
                frameProbeCreated = true
                return TestPerformanceFrameProbe()
            }
        )
        let startTask = Task {
            try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .iosDisplayLink,
                initialPlaybackActive: false
            )
        }
        for _ in 0..<100 where !recorder.readinessStarted {
            await Task.yield()
        }
        XCTAssertTrue(recorder.readinessStarted)

        coordinator.dispose()
        recorder.releaseReadiness()

        await assertDiagnosticsError(.controllerDisposed) {
            _ = try await startTask.value
        }
        let report = try await coordinator.stop(runId: recorder.runId, player: nil)
        XCTAssertEqual(report.runId, recorder.runId)
        XCTAssertFalse(frameProbeCreated)
        XCTAssertTrue(recorder.disposed)
        XCTAssertEqual(recorder.shutdownAwaitCount, 1)
    }

    func testStartAfterDisposeReturnsControllerDisposedWithoutAllocatingRecorder() async {
        var recorderCreated = false
        let coordinator = makeCoordinator {
            recorderCreated = true
            return TestPerformanceRecorder()
        }
        coordinator.dispose()

        await assertDiagnosticsError(.controllerDisposed) {
            _ = try await coordinator.startPerformance(
                configuration: VesperPerformanceDiagnosticsConfiguration(),
                probe: .flutterFrameTiming,
                initialPlaybackActive: false
            )
        }

        XCTAssertFalse(recorderCreated)
    }

    func testFrameBatchBoundaryAndCaptureTimeOverlayState() async throws {
        let recorder = TestPerformanceRecorder()
        let coordinator = makeCoordinator { recorder }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )
        let sample = VesperPerformanceFrameSample(
            loadNs: 10,
            budgetNs: 20,
            overlayState: VesperPerformanceOverlayState(active: false)
        )

        XCTAssertThrowsError(
            try coordinator.recordPerformanceFrames(runId: runId, samples: Array(repeating: sample, count: 121))
        ) { error in
            XCTAssertEqual(
                (error as? VesperPerformanceDiagnosticsError)?.code,
                .protocolViolation
            )
        }
        try coordinator.updateOverlayState(
            runId: runId,
            state: VesperPerformanceOverlayState(active: true)
        )
        try coordinator.recordPerformanceFrames(runId: runId, samples: [sample])

        let frame = try XCTUnwrap(
            recorder.events.last { $0.name == "performance_frame_sample" }
        )
        XCTAssertEqual(frame.attributes["overlayActive"], "false")
        XCTAssertEqual(frame.attributes["sampleClass"], "steady")
        _ = try await coordinator.stop(runId: runId, player: nil)
    }

    func testStaleFrameProbeCallbackCannotWriteIntoNewerRun() async throws {
        var callbacks: [(UInt64, UInt64) -> Void] = []
        var recorders: [TestPerformanceRecorder] = []
        let coordinator = VesperBenchmarkCoordinator(
            artifactValidator: {},
            recorderFactory: { _ in
                let recorder = TestPerformanceRecorder()
                recorders.append(recorder)
                return recorder
            },
            frameProbeFactory: { _, callback in
                callbacks.append(callback)
                return TestPerformanceFrameProbe()
            }
        )
        let firstRunId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .iosDisplayLink,
            initialPlaybackActive: false
        )
        _ = try await coordinator.stop(runId: firstRunId, player: nil)
        let secondRunId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .iosDisplayLink,
            initialPlaybackActive: false
        )

        callbacks.first?(30, 20)
        callbacks.last?(40, 20)

        XCTAssertFalse(recorders[1].events.contains { $0.name == "performance_frame_sample" &&
            $0.attributes["frameLoadNs"] == "30"
        })
        XCTAssertTrue(recorders[1].events.contains { $0.name == "performance_frame_sample" &&
            $0.attributes["frameLoadNs"] == "40"
        })
        _ = try await coordinator.stop(runId: secondRunId, player: nil)
    }

    func testMarkerCountIsBoundedPerRun() async throws {
        let coordinator = makeCoordinator { TestPerformanceRecorder() }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )
        for index in 0..<64 {
            try coordinator.recordMarker(
                runId: runId,
                name: "marker_\(index)",
                value: nil,
                sequenceIndex: index,
                expectedOverlayActive: nil
            )
        }

        XCTAssertThrowsError(
            try coordinator.recordMarker(
                runId: runId,
                name: "marker_overflow",
                value: nil,
                sequenceIndex: nil,
                expectedOverlayActive: nil
            )
        ) { error in
            XCTAssertEqual(
                (error as? VesperPerformanceDiagnosticsError)?.code,
                .protocolViolation
            )
        }
        _ = try await coordinator.stop(runId: runId, player: nil)
    }

    func testBufferingNormalizationRedactsHostAttributes() async throws {
        let recorder = TestPerformanceRecorder()
        let coordinator = makeCoordinator { recorder }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: true
        )

        coordinator.record(
            "buffering_changed",
            sourceProtocol: nil,
            attributes: ["isBuffering": "true", "reason": "private-value"]
        )
        coordinator.record(
            "buffering_changed",
            sourceProtocol: nil,
            attributes: ["isBuffering": "false"]
        )

        XCTAssertEqual(
            recorder.events.map(\.name).filter {
                $0 == "performance_playback_buffering_start"
            }.count,
            1
        )
        let bufferingEvents = recorder.events.filter {
            $0.name.hasPrefix("performance_playback_buffering_")
        }
        XCTAssertTrue(bufferingEvents.allSatisfy {
            $0.attributes["sampleClass"] == "steady" &&
                $0.attributes["overlayActive"] == "false"
        })
        XCTAssertFalse(String(describing: recorder.events).contains("private-value"))
        _ = try await coordinator.stop(runId: runId, player: nil)
    }

    func testAccessLogCountersUseRunBaselineAndReconcileStallSources() async throws {
        let recorder = TestPerformanceRecorder()
        let coordinator = makeCoordinator { recorder }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false,
            initialAccessLogCounters: VesperPerformanceAccessLogCounters(
                droppedVideoFrames: 7,
                stallCount: 3
            )
        )

        coordinator.record(
            "playback_stalled",
            sourceProtocol: nil,
            attributes: ["durationNs": "600000000"]
        )
        try coordinator.recordAccessLogCounters(
            runId: runId,
            counters: VesperPerformanceAccessLogCounters(
                droppedVideoFrames: 9,
                stallCount: 4
            )
        )
        try coordinator.recordAccessLogCounters(
            runId: runId,
            counters: VesperPerformanceAccessLogCounters(
                droppedVideoFrames: 10,
                stallCount: 6
            )
        )

        XCTAssertEqual(
            recorder.events
                .filter { $0.name == "dropped_video_frames" }
                .compactMap { $0.attributes["count"] },
            ["2", "1"]
        )
        XCTAssertEqual(
            recorder.events
                .filter { $0.name == "playback_stalled" }
                .compactMap { $0.attributes["count"] },
            ["1", "2"]
        )
        XCTAssertEqual(
            recorder.events
                .filter { $0.name == "playback_stalled" }
                .compactMap { $0.attributes["durationNs"] },
            ["600000000"]
        )
        XCTAssertTrue(
            recorder.events
                .filter { $0.name == "playback_stalled" }
                .allSatisfy { $0.attributes["sampleClass"] == "steady" }
        )
        _ = try await coordinator.stop(runId: runId, player: nil)
    }

    func testStopAndDisposeCacheFinalReport() async throws {
        let coordinator = makeCoordinator { TestPerformanceRecorder() }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )

        let first = try await coordinator.stop(runId: runId, player: nil)
        let second = try await coordinator.stop(runId: runId, player: nil)
        XCTAssertEqual(first, second)

        let disposedCoordinator = makeCoordinator { TestPerformanceRecorder() }
        let disposedRunId = try await disposedCoordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )
        disposedCoordinator.dispose()
        let disposedReport = try await disposedCoordinator.stop(
            runId: disposedRunId,
            player: nil
        )
        XCTAssertEqual(disposedReport.runId, disposedRunId)
    }

    func testSinkRejectedEventsAreNotCountedTwiceInTheReport() async throws {
        let recorder = TestPerformanceRecorder(
            pluginDroppedEvents: 2,
            finalDroppedEvents: 2
        )
        let coordinator = makeCoordinator { recorder }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )

        let report = try await coordinator.snapshot(runId: runId, player: nil)

        XCTAssertEqual(report.droppedEvents, 2)
        _ = try await coordinator.stop(runId: runId, player: nil)
    }

    func testMalformedMeasurementUnitUsesProtocolViolation() async throws {
        let coordinator = makeCoordinator {
            TestPerformanceRecorder(frameBudgetUnit: "ms")
        }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )

        await assertDiagnosticsError(.protocolViolation) {
            _ = try await coordinator.snapshot(runId: runId, player: nil)
        }
        await assertDiagnosticsError(.protocolViolation) {
            _ = try await coordinator.stop(runId: runId, player: nil)
        }
    }

    func testShutdownTimeoutAfterDisposeIsCached() async throws {
        let recorder = TestPerformanceRecorder(shutdownCompletes: false)
        let coordinator = makeCoordinator { recorder }
        let runId = try await coordinator.startPerformance(
            configuration: VesperPerformanceDiagnosticsConfiguration(),
            probe: .flutterFrameTiming,
            initialPlaybackActive: false
        )

        coordinator.dispose()
        await assertDiagnosticsError(.internalFailure) {
            _ = try await coordinator.stop(runId: runId, player: nil)
        }
        await assertDiagnosticsError(.internalFailure) {
            _ = try await coordinator.stop(runId: runId, player: nil)
        }

        XCTAssertEqual(recorder.shutdownAwaitCount, 1)
    }

    private func makeCoordinator(
        recorderFactory: @escaping () -> TestPerformanceRecorder
    ) -> VesperBenchmarkCoordinator {
        VesperBenchmarkCoordinator(
            artifactValidator: {},
            recorderFactory: { _ in recorderFactory() },
            frameProbeFactory: { _, _ in nil }
        )
    }

    private func assertDiagnosticsError(
        _ expectedCode: VesperPerformanceDiagnosticsErrorCode,
        operation: () async throws -> Void
    ) async {
        do {
            try await operation()
            XCTFail("Expected a performance diagnostics error.")
        } catch let error as VesperPerformanceDiagnosticsError {
            XCTAssertEqual(error.code, expectedCode)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    private func benchmarkConfiguration(
        for configuration: VesperPerformanceDiagnosticsConfiguration
    ) throws -> VesperBenchmarkConfiguration {
        VesperBenchmarkConfiguration(
            enabled: true,
            maxBufferedEvents: configuration.includeRawEvents ? configuration.maxRawEvents : 0,
            includeRawEvents: configuration.includeRawEvents,
            pluginReferences: [
                try VesperPluginReference(
                    pluginId: "io.github.umbrella22.vesper.performance-diagnostics",
                    capabilityInstanceId: "io.github.umbrella22.vesper.performance-diagnostics.benchmark",
                    transport: .native
                )
            ]
        )
    }
}

private enum TestDiagnosticsFailure: Error {
    case unavailable
}

private final class TestBenchmarkSinkSession: VesperBenchmarkSinkSessionProtocol,
    @unchecked Sendable
{
    private let lock = NSLock()
    private var storedDisposeCount = 0

    var disposeCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return storedDisposeCount
    }

    func submit(_ events: [VesperBenchmarkEvent]) throws -> VesperBenchmarkSinkReportPayload {
        VesperBenchmarkSinkReport(acceptedEvents: UInt64(events.count), droppedEvents: 0)
    }

    func flush() throws -> VesperBenchmarkSinkReportPayload {
        VesperBenchmarkSinkReport(acceptedEvents: 0, droppedEvents: 0)
    }

    func dispose() {
        lock.lock()
        storedDisposeCount += 1
        lock.unlock()
    }
}

@MainActor
private final class TestPerformanceFrameProbe: VesperPerformanceFrameProbe {
    func stop() {}
}

@MainActor
private final class TestPerformanceRecorder: VesperBenchmarkRecording {
    struct Event: CustomStringConvertible {
        let name: String
        let attributes: [String: String]

        var description: String { "\(name):\(attributes)" }
    }

    let runId = UUID().uuidString
    let sessionId = UUID().uuidString
    var events: [Event] = []
    var disposed = false
    var awaitedShutdown = false
    private let pluginDroppedEvents: UInt64
    private let finalDroppedEvents: UInt64
    private let shutdownCompletes: Bool
    private let frameBudgetUnit: String
    private let blockReadiness: Bool
    private var readinessContinuation: CheckedContinuation<Void, Never>?
    private(set) var readinessStarted = false
    private(set) var shutdownAwaitCount = 0

    init(
        pluginDroppedEvents: UInt64 = 0,
        finalDroppedEvents: UInt64 = 0,
        shutdownCompletes: Bool = true,
        frameBudgetUnit: String = "ns",
        blockReadiness: Bool = false
    ) {
        self.pluginDroppedEvents = pluginDroppedEvents
        self.finalDroppedEvents = finalDroppedEvents
        self.shutdownCompletes = shutdownCompletes
        self.frameBudgetUnit = frameBudgetUnit
        self.blockReadiness = blockReadiness
    }

    var isEnabled: Bool { !disposed }

    func record(
        _ eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: [String: String]
    ) {
        guard !disposed else { return }
        events.append(Event(name: eventName, attributes: attributes))
    }

    func drainEvents() -> [VesperBenchmarkEvent] { [] }
    func snapshotEvents() -> [VesperBenchmarkEvent] { [] }

    func summary() -> VesperBenchmarkSummary {
        VesperBenchmarkSummary(
            runId: runId,
            sessionId: sessionId,
            acceptedEvents: UInt64(events.count),
            droppedEvents: 0,
            pluginAcceptedEvents: UInt64(events.count),
            pluginDroppedEvents: pluginDroppedEvents,
            metrics: [],
            pluginFinalReport: VesperBenchmarkSinkReport(
                acceptedEvents: UInt64(events.count),
                droppedEvents: finalDroppedEvents,
                measurements: emptySchemaV1Measurements(),
                diagnostics: [
                    VesperPluginDiagnostic(
                        code: "performance.diagnosis",
                        severity: .warning,
                        message: "Correlation only.",
                        attributes: [
                            "kind": "insufficientEvidence",
                            "confidence": "low",
                            "evidenceCodes": "steady_cohorts_below_120",
                        ]
                    ),
                ]
            ),
            pluginErrors: []
        )
    }

    func flushSinks() {}
    func flushSinksAndAwait(timeout: TimeInterval) async -> Bool { true }

    func awaitSinkReadiness(timeout: TimeInterval) async -> VesperBenchmarkSinkReadiness {
        guard blockReadiness else { return .ready }
        readinessStarted = true
        await withCheckedContinuation { continuation in
            readinessContinuation = continuation
        }
        return .ready
    }

    func releaseReadiness() {
        let continuation = readinessContinuation
        readinessContinuation = nil
        continuation?.resume()
    }

    func dispose() {
        disposed = true
    }

    func awaitSinkShutdown(timeout: TimeInterval) async -> Bool {
        awaitedShutdown = true
        shutdownAwaitCount += 1
        return shutdownCompletes
    }

    func durationNs() -> UInt64 { 1 }

    private func emptySchemaV1Measurements() -> [VesperPluginMeasurement] {
        var measurements: [VesperPluginMeasurement] = []
        func append(_ name: String, unit: String, cohort: String? = nil) {
            measurements.append(
                VesperPluginMeasurement(
                    name: name,
                    value: 0,
                    unit: unit,
                    attributes: cohort.map { ["cohort": $0] } ?? [:]
                )
            )
        }
        for cohort in ["overlayInactive", "overlayActive", "transition", "excluded"] {
            append("frame_sample_count", unit: "count", cohort: cohort)
            append("frame_jank_count", unit: "count", cohort: cohort)
            append("frame_severe_jank_count", unit: "count", cohort: cohort)
            append("frame_jank_ratio", unit: "ratio", cohort: cohort)
            append("frame_severe_jank_ratio", unit: "ratio", cohort: cohort)
            append("frame_load_min", unit: "ns", cohort: cohort)
            append("frame_load_p50", unit: "ns", cohort: cohort)
            append("frame_load_p95", unit: "ns", cohort: cohort)
            append("frame_load_max", unit: "ns", cohort: cohort)
        }
        append("frame_budget", unit: frameBudgetUnit)
        append("overlay_transitions", unit: "count")
        append("active_playback_duration", unit: "ns")
        append("dropped_video_frames", unit: "count")
        append("buffering_count", unit: "count")
        append("buffering_duration", unit: "ns")
        append("stall_count", unit: "count")
        return measurements
    }
}
