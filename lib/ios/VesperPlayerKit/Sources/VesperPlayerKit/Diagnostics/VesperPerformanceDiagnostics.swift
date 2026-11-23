import Foundation

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
