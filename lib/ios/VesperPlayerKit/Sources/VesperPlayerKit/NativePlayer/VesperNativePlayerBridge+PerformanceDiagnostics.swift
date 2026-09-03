import AVFoundation
import Foundation

extension VesperNativePlayerBridge {
    func startPerformanceDiagnostics(
        configuration: VesperPerformanceDiagnosticsConfiguration,
        probe: VesperPerformanceProbe
    ) async throws -> String {
        try await benchmarkRecorder.startPerformance(
            configuration: configuration,
            probe: probe,
            initialPlaybackActive: publishedUiState.playbackState == .playing &&
                !publishedUiState.isBuffering,
            initialAccessLogCounters: vesperPerformanceAccessLogCounters(player)
        )
    }

    func updatePerformanceOverlayState(
        runId: String,
        state: VesperPerformanceOverlayState
    ) throws {
        try benchmarkRecorder.updateOverlayState(runId: runId, state: state)
    }

    func recordPerformanceMarker(
        runId: String,
        name: String,
        value: Double?,
        sequenceIndex: Int?,
        expectedOverlayActive: Bool?
    ) throws {
        try benchmarkRecorder.recordMarker(
            runId: runId,
            name: name,
            value: value,
            sequenceIndex: sequenceIndex,
            expectedOverlayActive: expectedOverlayActive
        )
    }

    func submitPerformanceFrameSamples(
        runId: String,
        samples: [VesperPerformanceFrameSample]
    ) throws {
        try benchmarkRecorder.recordPerformanceFrames(runId: runId, samples: samples)
    }

    func performanceDiagnosticsSnapshot(
        runId: String
    ) async throws -> VesperPerformanceDiagnosticsReport {
        try await benchmarkRecorder.snapshot(runId: runId, player: player)
    }

    func stopPerformanceDiagnostics(
        runId: String
    ) async throws -> VesperPerformanceDiagnosticsReport {
        try await benchmarkRecorder.stop(runId: runId, player: player)
    }
}
