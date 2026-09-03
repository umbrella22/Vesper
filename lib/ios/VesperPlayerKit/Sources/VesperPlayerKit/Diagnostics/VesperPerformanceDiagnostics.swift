import AVFoundation
import Foundation
import QuartzCore
import UIKit

private let vesperPerformanceFlushTimeout: TimeInterval = 2
private let vesperPerformanceStartTimeout: TimeInterval = 2
private let vesperPerformanceMarkerLimit = 64
private let vesperPerformanceMarkerByteLimit = 64

public struct VesperPerformanceDiagnosticsConfiguration: Equatable, Sendable {
    public let includeRawEvents: Bool
    public let maxRawEvents: Int

    public init(includeRawEvents: Bool = false, maxRawEvents: Int = 256) {
        self.includeRawEvents = includeRawEvents
        self.maxRawEvents = maxRawEvents
    }
}

public struct VesperPerformanceSampleClass: RawRepresentable, Codable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let steady = VesperPerformanceSampleClass(rawValue: "steady")
    public static let transition = VesperPerformanceSampleClass(rawValue: "transition")
    public static let excluded = VesperPerformanceSampleClass(rawValue: "excluded")

    public init(from decoder: any Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperPerformanceProbe: RawRepresentable, Codable, Equatable, Hashable, Sendable {
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let flutterFrameTiming = VesperPerformanceProbe(rawValue: "flutterFrameTiming")
    public static let androidFrameMetrics = VesperPerformanceProbe(rawValue: "androidFrameMetrics")
    public static let iosDisplayLink = VesperPerformanceProbe(rawValue: "iosDisplayLink")

    public init(from decoder: any Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperPerformanceDiagnosisKind: RawRepresentable, Codable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let insufficientEvidence = Self(rawValue: "insufficientEvidence")
    public static let noSignificantPressure = Self(rawValue: "noSignificantPressure")
    public static let overlayCorrelatedUiPressure = Self(rawValue: "overlayCorrelatedUiPressure")
    public static let hostUiPressureUncorrelated = Self(rawValue: "hostUiPressureUncorrelated")
    public static let playbackPressure = Self(rawValue: "playbackPressure")
    public static let mixedPressure = Self(rawValue: "mixedPressure")

    public init(from decoder: any Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperPerformanceConfidence: RawRepresentable, Codable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let low = Self(rawValue: "low")
    public static let medium = Self(rawValue: "medium")
    public static let high = Self(rawValue: "high")

    public init(from decoder: any Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperPerformanceDiagnosticSeverity: RawRepresentable, Codable, Equatable,
    Hashable, Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let info = Self(rawValue: "info")
    public static let warning = Self(rawValue: "warning")
    public static let error = Self(rawValue: "error")

    public init(from decoder: any Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperPerformanceOverlayState: Equatable, Sendable {
    public let active: Bool
    public let sampleClass: VesperPerformanceSampleClass
    public let loadedBasicItemCount: Int?
    public let loadedAdvancedItemCount: Int?
    public let advancedEffectsActive: Bool

    public init(
        active: Bool,
        sampleClass: VesperPerformanceSampleClass = .steady,
        loadedBasicItemCount: Int? = nil,
        loadedAdvancedItemCount: Int? = nil,
        advancedEffectsActive: Bool = false
    ) {
        self.active = active
        self.sampleClass = sampleClass
        self.loadedBasicItemCount = loadedBasicItemCount
        self.loadedAdvancedItemCount = loadedAdvancedItemCount
        self.advancedEffectsActive = advancedEffectsActive
    }
}

public struct VesperPerformanceFrameSample: Equatable, Sendable {
    public let loadNs: UInt64
    public let budgetNs: UInt64
    public let overlayState: VesperPerformanceOverlayState?

    public init(
        loadNs: UInt64,
        budgetNs: UInt64,
        overlayState: VesperPerformanceOverlayState? = nil
    ) {
        self.loadNs = loadNs
        self.budgetNs = budgetNs
        self.overlayState = overlayState
    }
}

public struct VesperPerformanceFrameCohort: Codable, Equatable, Sendable {
    public let sampleCount: UInt64
    public let jankCount: UInt64
    public let severeJankCount: UInt64
    public let jankRatio: Double
    public let severeJankRatio: Double
    public let minLoadNs: UInt64
    public let p50LoadNs: UInt64
    public let p95LoadNs: UInt64
    public let maxLoadNs: UInt64

    public var minLoadMs: Double { Double(minLoadNs) / 1_000_000 }
    public var p50LoadMs: Double { Double(p50LoadNs) / 1_000_000 }
    public var p95LoadMs: Double { Double(p95LoadNs) / 1_000_000 }
    public var maxLoadMs: Double { Double(maxLoadNs) / 1_000_000 }
}

public struct VesperPerformancePlaybackSummary: Codable, Equatable, Sendable {
    public let activeDurationNs: UInt64
    public let droppedVideoFrames: UInt64
    public let bufferingCount: UInt64
    public let bufferingDurationNs: UInt64
    public let stallCount: UInt64

    public var activeDurationMs: Double { Double(activeDurationNs) / 1_000_000 }
    public var bufferingDurationMs: Double { Double(bufferingDurationNs) / 1_000_000 }
}

public struct VesperPerformanceDiagnosis: Codable, Equatable, Sendable {
    public let kind: VesperPerformanceDiagnosisKind
    public let confidence: VesperPerformanceConfidence
    public let evidenceCodes: [String]
}

public struct VesperPerformanceDiagnostic: Codable, Equatable, Sendable {
    public let code: String
    public let severity: VesperPerformanceDiagnosticSeverity
    public let message: String
    public let attributes: [String: String]
}

public struct VesperPerformanceDiagnosticsReport: Codable, Equatable, Sendable {
    public let schemaVersion: Int
    public let runId: String
    public let sessionId: String
    public let platform: String
    public let probe: VesperPerformanceProbe
    public let durationNs: UInt64
    public let frameBudgetNs: UInt64
    public let cohorts: [String: VesperPerformanceFrameCohort]
    public let playback: VesperPerformancePlaybackSummary
    public let diagnosis: VesperPerformanceDiagnosis
    public let acceptedEvents: UInt64
    public let droppedEvents: UInt64
    public let rawEventsDropped: UInt64
    public let diagnostics: [VesperPerformanceDiagnostic]
    public let rawEvents: [VesperBenchmarkEvent]

    public var durationMs: Double { Double(durationNs) / 1_000_000 }
    public var frameBudgetMs: Double { Double(frameBudgetNs) / 1_000_000 }
}

public struct VesperPerformanceDiagnosticsErrorCode: RawRepresentable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let alreadyActive = Self(rawValue: "alreadyActive")
    public static let artifactUnavailable = Self(rawValue: "artifactUnavailable")
    public static let probeUnavailable = Self(rawValue: "probeUnavailable")
    public static let invalidConfiguration = Self(rawValue: "invalidConfiguration")
    public static let controllerDisposed = Self(rawValue: "controllerDisposed")
    public static let protocolViolation = Self(rawValue: "protocolViolation")
    public static let internalFailure = Self(rawValue: "internalFailure")
}

public struct VesperPerformanceDiagnosticsError: LocalizedError, Equatable, Sendable {
    public let code: VesperPerformanceDiagnosticsErrorCode
    public let message: String

    public init(code: VesperPerformanceDiagnosticsErrorCode, message: String) {
        self.code = code
        self.message = message
    }

    public var errorDescription: String? { message }
}

@MainActor
public final class VesperPerformanceDiagnosticsSession {
    public let runId: String

    private weak var controller: VesperPlayerController?
    private var finalReport: VesperPerformanceDiagnosticsReport?
    private var stopTask: Task<VesperPerformanceDiagnosticsReport, Error>?

    init(controller: VesperPlayerController, runId: String) {
        self.controller = controller
        self.runId = runId
    }

    public func updateOverlayState(_ state: VesperPerformanceOverlayState) throws {
        guard let controller else { throw controllerDisposedError() }
        try controller.updatePerformanceOverlayState(runId: runId, state: state)
    }

    public func recordMarker(
        _ name: String,
        value: Double? = nil,
        sequenceIndex: Int? = nil,
        expectedOverlayActive: Bool? = nil
    ) throws {
        guard let controller else { throw controllerDisposedError() }
        try controller.recordPerformanceMarker(
            runId: runId,
            name: name,
            value: value,
            sequenceIndex: sequenceIndex,
            expectedOverlayActive: expectedOverlayActive
        )
    }

    public func submitFrameSamples(_ samples: [VesperPerformanceFrameSample]) throws {
        guard let controller else { throw controllerDisposedError() }
        try controller.submitPerformanceFrameSamples(runId: runId, samples: samples)
    }

    public func snapshot() async throws -> VesperPerformanceDiagnosticsReport {
        guard let controller else { throw controllerDisposedError() }
        return try await controller.performanceDiagnosticsSnapshot(runId: runId)
    }

    public func stop() async throws -> VesperPerformanceDiagnosticsReport {
        if let finalReport { return finalReport }
        if let stopTask { return try await stopTask.value }
        guard let controller else { throw controllerDisposedError() }
        let task = Task { @MainActor in
            try await controller.stopPerformanceDiagnostics(runId: runId)
        }
        stopTask = task
        let report = try await task.value
        finalReport = report
        return report
    }

    private func controllerDisposedError() -> VesperPerformanceDiagnosticsError {
        VesperPerformanceDiagnosticsError(
            code: .controllerDisposed,
            message: "The performance diagnostics controller is unavailable."
        )
    }
}

@MainActor
final class VesperBenchmarkCoordinator: VesperBenchmarkRecording {
    final class ActiveRun {
        let recorder: any VesperBenchmarkRecording
        let mode: Mode
        let probe: VesperPerformanceProbe?
        var overlayState: VesperPerformanceOverlayState
        var frameProbe: (any VesperPerformanceFrameProbe)?
        var markerCount = 0
        var playbackPlaying: Bool
        var buffering = false
        var activePlaybackStartedNs: UInt64?
        var accumulatedActivePlaybackNs: UInt64 = 0
        var lastDroppedVideoFrames: UInt64 = 0
        var lastStallCount: UInt64 = 0
        var observedStallCount: UInt64 = 0
        var observedStallDurationNs: UInt64 = 0
        var accessLogStallCount: UInt64 = 0
        var reportedStallCount: UInt64 = 0
        var reportedStallDurationNs: UInt64 = 0

        init(
            recorder: any VesperBenchmarkRecording,
            mode: Mode,
            probe: VesperPerformanceProbe?,
            overlayState: VesperPerformanceOverlayState,
            initialPlaybackActive: Bool = false,
            initialAccessLogCounters: VesperPerformanceAccessLogCounters? = nil
        ) {
            self.recorder = recorder
            self.mode = mode
            self.probe = probe
            self.overlayState = overlayState
            playbackPlaying = initialPlaybackActive
            activePlaybackStartedNs = initialPlaybackActive
                ? DispatchTime.now().uptimeNanoseconds
                : nil
            lastDroppedVideoFrames = initialAccessLogCounters?.droppedVideoFrames ?? 0
            lastStallCount = initialAccessLogCounters?.stallCount ?? 0
        }

        func updatePlaybackActivity(nowNs: UInt64 = DispatchTime.now().uptimeNanoseconds) {
            let shouldBeActive = playbackPlaying && !buffering
            if shouldBeActive, activePlaybackStartedNs == nil {
                activePlaybackStartedNs = nowNs
            } else if !shouldBeActive, let startedNs = activePlaybackStartedNs {
                accumulatedActivePlaybackNs = saturatingAdd(
                    accumulatedActivePlaybackNs,
                    nowNs >= startedNs ? nowNs - startedNs : 0
                )
                activePlaybackStartedNs = nil
            }
        }

        func activePlaybackDurationNs(
            nowNs: UInt64 = DispatchTime.now().uptimeNanoseconds
        ) -> UInt64 {
            guard let startedNs = activePlaybackStartedNs else {
                return accumulatedActivePlaybackNs
            }
            return saturatingAdd(
                accumulatedActivePlaybackNs,
                nowNs >= startedNs ? nowNs - startedNs : 0
            )
        }
    }

    enum Mode {
        case legacy
        case performance
    }

    typealias ArtifactValidator = () throws -> Void
    typealias RecorderFactory = (VesperPerformanceDiagnosticsConfiguration) throws ->
        any VesperBenchmarkRecording
    typealias FrameProbeFactory = (
        VesperPerformanceProbe,
        @escaping @MainActor (UInt64, UInt64) -> Void
    ) throws -> (any VesperPerformanceFrameProbe)?

    private let artifactValidator: ArtifactValidator
    private let recorderFactory: RecorderFactory
    private let frameProbeFactory: FrameProbeFactory
    private let performanceStartTimeout: TimeInterval
    private let disabledRecorder = VesperBenchmarkRecorder(configuration: .disabled)
    private var activeRun: ActiveRun?
    private var pendingFinalization: (
        runId: String,
        task: Task<Result<VesperPerformanceDiagnosticsReport, VesperPerformanceDiagnosticsError>, Never>
    )?
    private var lastPerformanceReport: VesperPerformanceDiagnosticsReport?
    private var lastPerformanceFailure: (
        runId: String,
        error: VesperPerformanceDiagnosticsError
    )?
    private var isDisposed = false

    init(
        configuration: VesperBenchmarkConfiguration = .disabled,
        artifactValidator: @escaping ArtifactValidator = {
            _ = try VesperBundledPluginResolver.resolvePluginArtifacts([
                VesperBundledPluginReferences.performanceDiagnostics
            ])
        },
        recorderFactory: RecorderFactory? = nil,
        frameProbeFactory: FrameProbeFactory? = nil,
        performanceStartTimeout: TimeInterval = vesperPerformanceStartTimeout
    ) {
        self.artifactValidator = artifactValidator
        self.recorderFactory = recorderFactory ?? { configuration in
            VesperBenchmarkRecorder(
                configuration: VesperBenchmarkConfiguration(
                    enabled: true,
                    maxBufferedEvents: configuration.includeRawEvents
                        ? configuration.maxRawEvents
                        : 0,
                    includeRawEvents: configuration.includeRawEvents,
                    pluginReferences: [VesperBundledPluginReferences.performanceDiagnostics]
                )
            )
        }
        self.frameProbeFactory = frameProbeFactory ?? { probe, onFrame in
            switch probe {
            case .iosDisplayLink:
                return VesperDisplayLinkFrameProbe(onFrame: onFrame)
            case .flutterFrameTiming:
                return nil
            default:
                throw diagnosticsError(
                    .probeUnavailable,
                    "The requested iOS performance probe is unavailable."
                )
            }
        }
        self.performanceStartTimeout = performanceStartTimeout
        if configuration.enabled {
            activeRun = ActiveRun(
                recorder: VesperBenchmarkRecorder(configuration: configuration),
                mode: .legacy,
                probe: nil,
                overlayState: VesperPerformanceOverlayState(active: false)
            )
        }
    }

    var isEnabled: Bool { activeRun?.recorder.isEnabled == true }

    func record(
        _ eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: [String: String] = [:]
    ) {
        guard let run = activeRun else { return }
        if run.mode == .legacy {
            run.recorder.record(eventName, sourceProtocol: sourceProtocol, attributes: attributes)
            return
        }
        if recordNormalizedPlaybackEvent(run, eventName: eventName, attributes: attributes) {
            return
        }
        guard let safeAttributes = sanitizedPerformanceAttributes(
            eventName: eventName,
            attributes: attributes
        ) else { return }
        run.recorder.record(eventName, sourceProtocol: sourceProtocol, attributes: safeAttributes)
    }

    func startPerformance(
        configuration: VesperPerformanceDiagnosticsConfiguration,
        probe: VesperPerformanceProbe,
        initialPlaybackActive: Bool,
        initialAccessLogCounters: VesperPerformanceAccessLogCounters? = nil
    ) async throws -> String {
        guard !isDisposed else {
            throw diagnosticsError(
                .controllerDisposed,
                "The player controller has been disposed."
            )
        }
        guard activeRun == nil, pendingFinalization == nil else {
            throw diagnosticsError(.alreadyActive, "A performance diagnostics run is already active.")
        }
        guard (0...2_048).contains(configuration.maxRawEvents) else {
            throw diagnosticsError(
                .invalidConfiguration,
                "maxRawEvents must be between 0 and 2048."
            )
        }
        guard probe == .iosDisplayLink || probe == .flutterFrameTiming else {
            throw diagnosticsError(
                .probeUnavailable,
                "The requested iOS performance probe is unavailable."
            )
        }
        do {
            try artifactValidator()
        } catch {
            throw diagnosticsError(
                .artifactUnavailable,
                "The Vesper performance diagnostics artifact is unavailable."
            )
        }

        let recorder: any VesperBenchmarkRecording
        do {
            recorder = try recorderFactory(configuration)
        } catch {
            throw diagnosticsError(
                .artifactUnavailable,
                "The Vesper performance diagnostics artifact is unavailable."
            )
        }
        let run = ActiveRun(
            recorder: recorder,
            mode: .performance,
            probe: probe,
            overlayState: VesperPerformanceOverlayState(active: false),
            initialPlaybackActive: initialPlaybackActive,
            initialAccessLogCounters: initialAccessLogCounters
        )
        activeRun = run
        let readiness = await recorder.awaitSinkReadiness(timeout: performanceStartTimeout)
        guard !isDisposed, activeRun === run else {
            throw diagnosticsError(
                .controllerDisposed,
                "The player controller was disposed while performance diagnostics started."
            )
        }
        switch readiness {
        case .ready:
            break
        case .openFailed:
            activeRun = nil
            guard await cleanupUnpublishedRun(run) else {
                throw diagnosticsError(
                    .internalFailure,
                    "Performance diagnostics cleanup timed out."
                )
            }
            throw diagnosticsError(
                .artifactUnavailable,
                "The Vesper performance diagnostics artifact could not be opened."
            )
        case .timedOut:
            activeRun = nil
            _ = await cleanupUnpublishedRun(run)
            throw diagnosticsError(
                .internalFailure,
                "Performance diagnostics artifact startup timed out."
            )
        }
        do {
            run.frameProbe = try frameProbeFactory(probe) { [weak self, weak run] loadNs, budgetNs in
                guard let self, let run else { return }
                self.recordPerformanceFrame(
                    for: run,
                    loadNs: loadNs,
                    budgetNs: budgetNs,
                    state: nil
                )
            }
            recorder.record(
                "performance_session_context",
                sourceProtocol: nil,
                attributes: ["probe": probe.rawValue, "activePlaybackNs": "0"]
            )
            return recorder.summary().runId
        } catch {
            activeRun = nil
            guard await cleanupUnpublishedRun(run) else {
                throw diagnosticsError(
                    .internalFailure,
                    "Performance diagnostics cleanup timed out."
                )
            }
            if let error = error as? VesperPerformanceDiagnosticsError { throw error }
            throw diagnosticsError(.internalFailure, "Performance diagnostics could not start.")
        }
    }

    private func cleanupUnpublishedRun(_ run: ActiveRun) async -> Bool {
        run.frameProbe?.stop()
        run.recorder.dispose()
        return await run.recorder.awaitSinkShutdown(timeout: vesperPerformanceFlushTimeout)
    }

    func updateOverlayState(runId: String, state: VesperPerformanceOverlayState) throws {
        try validateOverlayState(state)
        let run = try requirePerformanceRun(runId)
        let previous = run.overlayState
        run.overlayState = state
        if previous.active != state.active || previous.sampleClass != state.sampleClass {
            run.recorder.record(
                "performance_overlay_transition",
                sourceProtocol: nil,
                attributes: overlayAttributes(state)
            )
        }
    }

    func recordMarker(
        runId: String,
        name: String,
        value: Double?,
        sequenceIndex: Int?,
        expectedOverlayActive: Bool?
    ) throws {
        guard isValidPerformanceMarker(name), value?.isFinite != false else {
            throw diagnosticsError(
                .protocolViolation,
                "The performance marker does not satisfy the wire contract."
            )
        }
        let run = try requirePerformanceRun(runId)
        guard run.markerCount < vesperPerformanceMarkerLimit else {
            throw diagnosticsError(
                .protocolViolation,
                "A performance diagnostics run accepts at most 64 markers."
            )
        }
        run.markerCount += 1
        var attributes = ["name": name]
        if let value { attributes["value"] = String(value) }
        if let sequenceIndex { attributes["sequenceIndex"] = String(sequenceIndex) }
        if let expectedOverlayActive {
            attributes["expectedOverlayActive"] = String(expectedOverlayActive)
        }
        run.recorder.record(
            "performance_marker",
            sourceProtocol: nil,
            attributes: attributes
        )
    }

    func recordPerformanceFrames(
        runId: String,
        samples: [VesperPerformanceFrameSample]
    ) throws {
        let run = try requirePerformanceRun(runId)
        guard samples.count <= 120 else {
            throw diagnosticsError(
                .protocolViolation,
                "Performance frame batches are limited to 120 samples."
            )
        }
        for sample in samples {
            guard sample.budgetNs > 0 else {
                throw diagnosticsError(
                    .protocolViolation,
                    "Performance frame samples require a positive budget."
                )
            }
            if let state = sample.overlayState { try validateOverlayState(state) }
            recordPerformanceFrame(
                for: run,
                loadNs: sample.loadNs,
                budgetNs: sample.budgetNs,
                state: sample.overlayState
            )
        }
    }

    func snapshot(
        runId: String,
        player: AVPlayer?
    ) async throws -> VesperPerformanceDiagnosticsReport {
        let run = try requirePerformanceRun(runId)
        sampleAccessLog(run, player: player)
        recordSessionContext(run)
        guard await run.recorder.flushSinksAndAwait(timeout: vesperPerformanceFlushTimeout) else {
            throw diagnosticsError(.internalFailure, "Performance diagnostics snapshot timed out.")
        }
        return try buildPerformanceReport(run)
    }

    func stop(
        runId: String,
        player: AVPlayer?
    ) async throws -> VesperPerformanceDiagnosticsReport {
        if let report = lastPerformanceReport, report.runId == runId { return report }
        if let failure = lastPerformanceFailure, failure.runId == runId {
            throw failure.error
        }
        if let pendingFinalization, pendingFinalization.runId == runId {
            return try await pendingFinalization.task.value.get()
        }
        let run = try requirePerformanceRun(runId)
        activeRun = nil
        prepareForFinalization(run, player: player)
        let task = finalizationTask(for: run)
        pendingFinalization = (runId, task)
        let result = await task.value
        pendingFinalization = nil
        switch result {
        case let .success(report):
            lastPerformanceReport = report
            return report
        case let .failure(error):
            lastPerformanceFailure = (runId, error)
            throw error
        }
    }

    func dispose() {
        dispose(player: nil)
    }

    func dispose(player: AVPlayer?) {
        isDisposed = true
        guard let run = activeRun else { return }
        activeRun = nil
        run.frameProbe?.stop()
        if run.mode == .legacy {
            run.recorder.dispose()
            return
        }
        prepareForFinalization(run, player: player)
        let runId = run.recorder.summary().runId
        let task = finalizationTask(for: run)
        pendingFinalization = (runId, task)
        Task { @MainActor [weak self] in
            let result = await task.value
            guard let self else { return }
            if self.pendingFinalization?.runId == runId {
                self.pendingFinalization = nil
                switch result {
                case let .success(report):
                    self.lastPerformanceReport = report
                case let .failure(error):
                    self.lastPerformanceFailure = (runId, error)
                }
            }
        }
    }

    func drainEvents() -> [VesperBenchmarkEvent] {
        activeRun?.recorder.drainEvents() ?? []
    }

    func snapshotEvents() -> [VesperBenchmarkEvent] {
        activeRun?.recorder.snapshotEvents() ?? []
    }

    func summary() -> VesperBenchmarkSummary {
        if let recorder = activeRun?.recorder { return recorder.summary() }
        if let report = lastPerformanceReport { return legacySummary(from: report) }
        return disabledRecorder.summary()
    }

    func flushSinks() {
        activeRun?.recorder.flushSinks()
    }

    func flushSinksAndAwait(timeout: TimeInterval) async -> Bool {
        guard let recorder = activeRun?.recorder else { return true }
        return await recorder.flushSinksAndAwait(timeout: timeout)
    }

    func awaitSinkShutdown(timeout: TimeInterval) async -> Bool {
        guard let recorder = activeRun?.recorder else { return true }
        return await recorder.awaitSinkShutdown(timeout: timeout)
    }

    func durationNs() -> UInt64 {
        activeRun?.recorder.durationNs() ?? 0
    }

    private func finalizationTask(
        for run: ActiveRun
    ) -> Task<Result<VesperPerformanceDiagnosticsReport, VesperPerformanceDiagnosticsError>, Never> {
        Task { @MainActor in
            run.recorder.dispose()
            guard await run.recorder.awaitSinkShutdown(timeout: vesperPerformanceFlushTimeout) else {
                return .failure(diagnosticsError(
                    .internalFailure,
                    "Performance diagnostics sink shutdown timed out."
                ))
            }
            do {
                return .success(try buildPerformanceReport(run))
            } catch let error as VesperPerformanceDiagnosticsError {
                return .failure(error)
            } catch {
                return .failure(diagnosticsError(
                    .internalFailure,
                    "Performance diagnostics could not build its final report."
                ))
            }
        }
    }

    private func prepareForFinalization(_ run: ActiveRun, player: AVPlayer?) {
        run.frameProbe?.stop()
        run.updatePlaybackActivity()
        sampleAccessLog(run, player: player)
        if run.buffering {
            run.buffering = false
            run.recorder.record(
                "performance_playback_buffering_end",
                sourceProtocol: nil,
                attributes: overlayAttributes(run.overlayState)
            )
        }
        recordSessionContext(run)
    }

    private func recordPerformanceFrame(
        for expectedRun: ActiveRun,
        loadNs: UInt64,
        budgetNs: UInt64,
        state: VesperPerformanceOverlayState?
    ) {
        guard
            let run = activeRun,
            run === expectedRun,
            run.mode == .performance,
            budgetNs > 0
        else { return }
        let overlayState = state ?? run.overlayState
        run.recorder.record(
            "performance_frame_sample",
            sourceProtocol: nil,
            attributes: overlayAttributes(overlayState).merging([
                "frameLoadNs": String(loadNs),
                "frameBudgetNs": String(budgetNs),
                "probe": run.probe?.rawValue ?? "unknown",
            ]) { current, _ in current }
        )
    }

    private func recordSessionContext(_ run: ActiveRun) {
        run.recorder.record(
            "performance_session_context",
            sourceProtocol: nil,
            attributes: [
                "probe": run.probe?.rawValue ?? "unknown",
                "activePlaybackNs": String(run.activePlaybackDurationNs()),
            ]
        )
    }

    private func sampleAccessLog(_ run: ActiveRun, player: AVPlayer?) {
        guard let counters = vesperPerformanceAccessLogCounters(player) else { return }
        accumulateAccessLogCounters(run, counters: counters)
    }

    func recordAccessLogCounters(
        runId: String,
        counters: VesperPerformanceAccessLogCounters
    ) throws {
        accumulateAccessLogCounters(try requirePerformanceRun(runId), counters: counters)
    }

    private func accumulateAccessLogCounters(
        _ run: ActiveRun,
        counters: VesperPerformanceAccessLogCounters
    ) {
        let droppedDelta = monotonicDelta(
            current: counters.droppedVideoFrames,
            previous: run.lastDroppedVideoFrames
        )
        let stallDelta = monotonicDelta(
            current: counters.stallCount,
            previous: run.lastStallCount
        )
        run.lastDroppedVideoFrames = counters.droppedVideoFrames
        run.lastStallCount = counters.stallCount
        if droppedDelta > 0 {
            run.recorder.record(
                "dropped_video_frames",
                sourceProtocol: nil,
                attributes: ["count": String(droppedDelta)]
            )
        }
        if stallDelta > 0 {
            run.accessLogStallCount = saturatingAdd(run.accessLogStallCount, stallDelta)
        }
        recordReconciledStalls(run)
    }

    private func recordNormalizedPlaybackEvent(
        _ run: ActiveRun,
        eventName: String,
        attributes: [String: String]
    ) -> Bool {
        switch eventName {
        case "time_control_status_changed":
            guard let status = attributes["status"] else { return true }
            let wasBuffering = run.buffering
            switch status {
            case "playing":
                run.playbackPlaying = true
                run.buffering = false
            case "waiting":
                run.playbackPlaying = true
                run.buffering = true
            case "paused":
                run.playbackPlaying = false
                run.buffering = false
            default:
                return true
            }
            run.updatePlaybackActivity()
            recordBufferingTransition(run, from: wasBuffering)
            return true
        case "playback_state_changed":
            run.playbackPlaying = attributes["state"]?.lowercased() == "playing"
            run.updatePlaybackActivity()
            return true
        case "buffering_changed":
            guard let value = attributes["isBuffering"], let buffering = Bool(value) else {
                return true
            }
            let wasBuffering = run.buffering
            run.buffering = buffering
            run.updatePlaybackActivity()
            recordBufferingTransition(run, from: wasBuffering)
            return true
        case "playback_stalled":
            let count = attributes["count"].flatMap(UInt64.init) ?? 1
            let durationNs = attributes["durationNs"].flatMap(UInt64.init) ?? 0
            run.observedStallCount = saturatingAdd(run.observedStallCount, count)
            run.observedStallDurationNs = saturatingAdd(
                run.observedStallDurationNs,
                durationNs
            )
            recordReconciledStalls(run)
            return true
        default:
            return false
        }
    }

    private func recordReconciledStalls(_ run: ActiveRun) {
        let totalCount = max(run.observedStallCount, run.accessLogStallCount)
        let countDelta = monotonicDelta(
            current: totalCount,
            previous: run.reportedStallCount
        )
        let totalDurationNs = run.observedStallDurationNs
        let durationDeltaNs = monotonicDelta(
            current: totalDurationNs,
            previous: run.reportedStallDurationNs
        )
        guard countDelta > 0 || durationDeltaNs > 0 else { return }
        run.reportedStallCount = totalCount
        run.reportedStallDurationNs = totalDurationNs
        var attributes = overlayAttributes(run.overlayState)
        attributes["count"] = String(countDelta)
        if durationDeltaNs > 0 {
            attributes["durationNs"] = String(durationDeltaNs)
        }
        run.recorder.record(
            "playback_stalled",
            sourceProtocol: nil,
            attributes: attributes
        )
    }

    private func recordBufferingTransition(_ run: ActiveRun, from previous: Bool) {
        guard previous != run.buffering else { return }
        run.recorder.record(
            run.buffering
                ? "performance_playback_buffering_start"
                : "performance_playback_buffering_end",
            sourceProtocol: nil,
            attributes: overlayAttributes(run.overlayState)
        )
    }

    private func requirePerformanceRun(_ runId: String) throws -> ActiveRun {
        guard
            let run = activeRun,
            run.mode == .performance,
            run.recorder.summary().runId == runId
        else {
            throw diagnosticsError(
                .controllerDisposed,
                "The performance diagnostics session is no longer active."
            )
        }
        return run
    }
}

@MainActor
protocol VesperPerformanceFrameProbe: AnyObject {
    func stop()
}

@MainActor
private final class VesperDisplayLinkFrameProbe: VesperPerformanceFrameProbe {
    private final class Target: NSObject {
        weak var owner: VesperDisplayLinkFrameProbe?
        let onFrame: @MainActor (UInt64, UInt64) -> Void

        init(onFrame: @escaping @MainActor (UInt64, UInt64) -> Void) {
            self.onFrame = onFrame
        }

        @objc func tick(_ displayLink: CADisplayLink) {
            MainActor.assumeIsolated {
                owner?.handle(displayLink, onFrame: onFrame)
            }
        }
    }

    private let target: Target
    private let displayLink: CADisplayLink
    private var previousTimestamp: CFTimeInterval?

    init(onFrame: @escaping @MainActor (UInt64, UInt64) -> Void) {
        target = Target(onFrame: onFrame)
        displayLink = CADisplayLink(target: target, selector: #selector(Target.tick(_:)))
        target.owner = self
        displayLink.add(to: .main, forMode: .common)
    }

    func stop() {
        displayLink.invalidate()
        previousTimestamp = nil
    }

    private func handle(
        _ displayLink: CADisplayLink,
        onFrame: @MainActor (UInt64, UInt64) -> Void
    ) {
        defer { previousTimestamp = displayLink.timestamp }
        guard let previousTimestamp else { return }
        let predictedInterval = displayLink.targetTimestamp - displayLink.timestamp
        let fallbackInterval = 1 / Double(max(UIScreen.main.maximumFramesPerSecond, 1))
        let budgetSeconds = predictedInterval > 0 ? predictedInterval : fallbackInterval
        let elapsedSeconds = max(displayLink.timestamp - previousTimestamp, budgetSeconds)
        let intervalRatio = (elapsedSeconds / budgetSeconds).rounded()
        let vsyncIntervals = clampedPerformanceUInt64(intervalRatio).clampedToPositive()
        let budgetNs = secondsToNanoseconds(budgetSeconds)
        let loadNs = saturatingMultiply(budgetNs, vsyncIntervals)
        onFrame(loadNs, budgetNs)
    }
}

struct VesperPerformanceAccessLogCounters: Equatable {
    let droppedVideoFrames: UInt64
    let stallCount: UInt64
}

func vesperPerformanceAccessLogCounters(
    _ player: AVPlayer?
) -> VesperPerformanceAccessLogCounters? {
    guard let events = player?.currentItem?.accessLog()?.events else { return nil }
    let droppedVideoFrames = events.reduce(UInt64(0)) { partial, event in
        saturatingAdd(partial, UInt64(max(event.numberOfDroppedVideoFrames, 0)))
    }
    let stallCount = events.reduce(UInt64(0)) { partial, event in
        saturatingAdd(partial, UInt64(max(event.numberOfStalls, 0)))
    }
    return VesperPerformanceAccessLogCounters(
        droppedVideoFrames: droppedVideoFrames,
        stallCount: stallCount
    )
}

private func overlayAttributes(_ state: VesperPerformanceOverlayState) -> [String: String] {
    var attributes: [String: String] = [
        "overlayActive": String(state.active),
        "sampleClass": state.sampleClass.rawValue,
        "advancedEffectsActive": String(state.advancedEffectsActive),
    ]
    if let count = state.loadedBasicItemCount {
        attributes["loadedBasicItemCount"] = String(count)
    }
    if let count = state.loadedAdvancedItemCount {
        attributes["loadedAdvancedItemCount"] = String(count)
    }
    return attributes
}

private func sanitizedPerformanceAttributes(
    eventName: String,
    attributes: [String: String]
) -> [String: String]? {
    let allowedKeys: Set<String>
    switch eventName {
    case "dropped_video_frames":
        allowedKeys = ["count"]
    case "playback_stalled":
        allowedKeys = ["count", "durationNs"]
    case "playback_error":
        allowedKeys = ["code", "category", "retriable"]
    case "first_frame_rendered", "playback_ended", "initialize_start",
        "initialize_completed", "source_load_start", "source_load_configured",
        "performance_playback_buffering_start", "performance_playback_buffering_end":
        allowedKeys = []
    default:
        return nil
    }
    return attributes.filter { allowedKeys.contains($0.key) }
}

private func validateOverlayState(_ state: VesperPerformanceOverlayState) throws {
    guard
        state.sampleClass == .steady ||
            state.sampleClass == .transition ||
            state.sampleClass == .excluded,
        state.loadedBasicItemCount.map({ $0 >= 0 }) ?? true,
        state.loadedAdvancedItemCount.map({ $0 >= 0 }) ?? true
    else {
        throw diagnosticsError(.protocolViolation, "The performance overlay state is invalid.")
    }
}

private func isValidPerformanceMarker(_ name: String) -> Bool {
    let bytes = Array(name.utf8)
    guard
        !bytes.isEmpty,
        bytes.count <= vesperPerformanceMarkerByteLimit,
        bytes.allSatisfy({ $0 <= 0x7f }),
        isAsciiLetter(bytes[0]) || bytes[0] == 0x5f
    else { return false }
    return bytes.allSatisfy { byte in
        isAsciiLetter(byte) ||
            (0x30...0x39).contains(byte) ||
            byte == 0x5f || byte == 0x2e || byte == 0x2d
    }
}

private func isAsciiLetter(_ byte: UInt8) -> Bool {
    (0x41...0x5a).contains(byte) || (0x61...0x7a).contains(byte)
}

private func diagnosticsError(
    _ code: VesperPerformanceDiagnosticsErrorCode,
    _ message: String
) -> VesperPerformanceDiagnosticsError {
    VesperPerformanceDiagnosticsError(code: code, message: message)
}

private func secondsToNanoseconds(_ value: Double) -> UInt64 {
    guard value.isFinite, value > 0 else { return 1 }
    return clampedPerformanceUInt64(value * 1_000_000_000).clampedToPositive()
}

private func clampedPerformanceUInt64(_ value: Double) -> UInt64 {
    guard value.isFinite, value > 0 else { return 0 }
    if value >= Double(UInt64.max) { return UInt64.max }
    return UInt64(value)
}

private func monotonicDelta(current: UInt64, previous: UInt64) -> UInt64 {
    current >= previous ? current - previous : current
}

private func saturatingAdd(_ lhs: UInt64, _ rhs: UInt64) -> UInt64 {
    let result = lhs.addingReportingOverflow(rhs)
    return result.overflow ? UInt64.max : result.partialValue
}

private func saturatingMultiply(_ lhs: UInt64, _ rhs: UInt64) -> UInt64 {
    let result = lhs.multipliedReportingOverflow(by: rhs)
    return result.overflow ? UInt64.max : result.partialValue
}

private extension UInt64 {
    func clampedToPositive() -> UInt64 { Swift.max(self, 1) }
}

@MainActor
private func buildPerformanceReport(
    _ run: VesperBenchmarkCoordinator.ActiveRun
) throws -> VesperPerformanceDiagnosticsReport {
    let recorder = run.recorder
    let summary = recorder.summary()
    guard let pluginReport = summary.pluginFinalReport else {
        throw diagnosticsError(
            .internalFailure,
            "The performance diagnostics sink did not produce a report."
        )
    }
    let measurements = PerformanceMeasurementReader(pluginReport.measurements)
    func cohort(_ name: String) throws -> VesperPerformanceFrameCohort {
        let cohort = VesperPerformanceFrameCohort(
            sampleCount: try measurements.count("frame_sample_count", cohort: name),
            jankCount: try measurements.count("frame_jank_count", cohort: name),
            severeJankCount: try measurements.count("frame_severe_jank_count", cohort: name),
            jankRatio: try measurements.ratio("frame_jank_ratio", cohort: name),
            severeJankRatio: try measurements.ratio("frame_severe_jank_ratio", cohort: name),
            minLoadNs: try measurements.nanoseconds("frame_load_min", cohort: name),
            p50LoadNs: try measurements.nanoseconds("frame_load_p50", cohort: name),
            p95LoadNs: try measurements.nanoseconds("frame_load_p95", cohort: name),
            maxLoadNs: try measurements.nanoseconds("frame_load_max", cohort: name)
        )
        guard cohort.severeJankCount <= cohort.jankCount,
              cohort.jankCount <= cohort.sampleCount,
              cohort.minLoadNs <= cohort.p50LoadNs,
              cohort.p50LoadNs <= cohort.p95LoadNs,
              cohort.p95LoadNs <= cohort.maxLoadNs
        else {
            throw performanceReportProtocolViolation()
        }
        return cohort
    }
    let diagnosisDiagnostics = pluginReport.diagnostics.filter {
        $0.code == "performance.diagnosis"
    }
    guard diagnosisDiagnostics.count == 1,
          let diagnosisKind = diagnosisDiagnostics[0].attributes["kind"],
          !diagnosisKind.isEmpty,
          let diagnosisConfidence = diagnosisDiagnostics[0].attributes["confidence"],
          !diagnosisConfidence.isEmpty,
          let evidenceValue = diagnosisDiagnostics[0].attributes["evidenceCodes"]
    else {
        throw performanceReportProtocolViolation()
    }
    let evidenceCodes = evidenceValue.split(separator: ",", omittingEmptySubsequences: false)
        .map(String.init)
    guard !evidenceCodes.isEmpty, evidenceCodes.allSatisfy({ !$0.isEmpty }) else {
        throw performanceReportProtocolViolation()
    }
    let cohortNames = ["overlayInactive", "overlayActive", "transition", "excluded"]
    var cohorts: [String: VesperPerformanceFrameCohort] = [:]
    for name in cohortNames {
        cohorts[name] = try cohort(name)
    }
    let frameBudgetNs = try measurements.nanoseconds("frame_budget")
    guard frameBudgetNs > 0 || cohorts.values.allSatisfy({ $0.sampleCount == 0 }) else {
        throw performanceReportProtocolViolation()
    }
    return VesperPerformanceDiagnosticsReport(
        schemaVersion: 1,
        runId: summary.runId,
        sessionId: summary.sessionId,
        platform: "ios",
        probe: run.probe ?? VesperPerformanceProbe(rawValue: "unknown"),
        durationNs: recorder.durationNs(),
        frameBudgetNs: frameBudgetNs,
        cohorts: cohorts,
        playback: VesperPerformancePlaybackSummary(
            activeDurationNs: try measurements.nanoseconds("active_playback_duration"),
            droppedVideoFrames: try measurements.count("dropped_video_frames"),
            bufferingCount: try measurements.count("buffering_count"),
            bufferingDurationNs: try measurements.nanoseconds("buffering_duration"),
            stallCount: try measurements.count("stall_count")
        ),
        diagnosis: VesperPerformanceDiagnosis(
            kind: VesperPerformanceDiagnosisKind(rawValue: diagnosisKind),
            confidence: VesperPerformanceConfidence(rawValue: diagnosisConfidence),
            evidenceCodes: evidenceCodes
        ),
        acceptedEvents: pluginReport.acceptedEvents,
        droppedEvents: max(pluginReport.droppedEvents, summary.pluginDroppedEvents),
        rawEventsDropped: summary.droppedEvents,
        diagnostics: pluginReport.diagnostics.map { diagnostic in
            VesperPerformanceDiagnostic(
                code: diagnostic.code,
                severity: VesperPerformanceDiagnosticSeverity(
                    rawValue: diagnostic.severity.rawValue
                ),
                message: diagnostic.message,
                attributes: diagnostic.attributes
            )
        },
        rawEvents: recorder.snapshotEvents()
    )
}

private struct PerformanceMeasurementReader {
    let measurements: [VesperPluginMeasurement]

    init(_ measurements: [VesperPluginMeasurement]) {
        self.measurements = measurements
    }

    func count(_ name: String, cohort: String? = nil) throws -> UInt64 {
        try exactNonnegativeInteger(name, unit: "count", cohort: cohort)
    }

    func nanoseconds(_ name: String, cohort: String? = nil) throws -> UInt64 {
        try exactNonnegativeInteger(name, unit: "ns", cohort: cohort)
    }

    func ratio(_ name: String, cohort: String? = nil) throws -> Double {
        let value = try requiredValue(name, unit: "ratio", cohort: cohort)
        guard (0...1).contains(value) else { throw performanceReportProtocolViolation() }
        return value
    }

    private func exactNonnegativeInteger(
        _ name: String,
        unit: String,
        cohort: String?
    ) throws -> UInt64 {
        let value = try requiredValue(name, unit: unit, cohort: cohort)
        guard value.rounded(.towardZero) == value,
              value < 18_446_744_073_709_551_616.0
        else {
            throw performanceReportProtocolViolation()
        }
        return UInt64(value)
    }

    private func requiredValue(
        _ name: String,
        unit: String,
        cohort: String?
    ) throws -> Double {
        let matches = measurements.filter { measurement in
            measurement.name == name && measurement.attributes["cohort"] == cohort
        }
        guard matches.count == 1 else { throw performanceReportProtocolViolation() }
        let measurement = matches[0]
        guard measurement.unit == unit,
              measurement.value.isFinite,
              measurement.value >= 0
        else {
            throw performanceReportProtocolViolation()
        }
        return measurement.value
    }
}

private func performanceReportProtocolViolation() -> VesperPerformanceDiagnosticsError {
    diagnosticsError(
        .protocolViolation,
        "The performance diagnostics sink returned a malformed schema v1 report."
    )
}

private func legacySummary(
    from report: VesperPerformanceDiagnosticsReport
) -> VesperBenchmarkSummary {
    VesperBenchmarkSummary(
        runId: report.runId,
        sessionId: report.sessionId,
        acceptedEvents: report.acceptedEvents,
        droppedEvents: report.rawEventsDropped,
        pluginAcceptedEvents: report.acceptedEvents,
        pluginDroppedEvents: report.droppedEvents,
        metrics: [],
        pluginFinalReport: nil,
        pluginErrors: []
    )
}
