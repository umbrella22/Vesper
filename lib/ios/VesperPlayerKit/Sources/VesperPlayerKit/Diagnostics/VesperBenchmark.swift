import Foundation

public struct VesperBenchmarkConfiguration: Equatable {
    public let enabled: Bool
    public let maxBufferedEvents: Int
    public let includeRawEvents: Bool
    public let consoleLogging: Bool
    public let pluginReferences: [VesperPluginReference]

    public init(
        enabled: Bool = false,
        maxBufferedEvents: Int = 2_048,
        includeRawEvents: Bool = true,
        consoleLogging: Bool = false,
        pluginReferences: [VesperPluginReference] = []
    ) {
        self.enabled = enabled
        self.maxBufferedEvents = max(maxBufferedEvents, 0)
        self.includeRawEvents = includeRawEvents
        self.consoleLogging = consoleLogging
        self.pluginReferences = pluginReferences
    }

    public static let disabled = VesperBenchmarkConfiguration()
}

public struct VesperBenchmarkEvent: Codable, Equatable, Sendable {
    public let runId: String
    public let sessionId: String
    public let platform: String
    public let sourceProtocol: String?
    public let eventName: String
    public let timestampNs: UInt64
    public let elapsedNs: UInt64
    public let thread: String?
    public let attributes: [String: String]
}

public struct VesperBenchmarkMetricSummary: Codable, Equatable, Sendable {
    public let name: String
    public let count: Int
    public let minNs: UInt64
    public let maxNs: UInt64
    public let p50Ns: UInt64
    public let p90Ns: UInt64
    public let p95Ns: UInt64
}

public struct VesperPluginMeasurement: Codable, Equatable, Sendable {
    public let name: String
    public let value: Double
    public let unit: String
    public let attributes: [String: String]

    public init(
        name: String,
        value: Double,
        unit: String,
        attributes: [String: String] = [:]
    ) {
        self.name = name
        self.value = value
        self.unit = unit
        self.attributes = attributes
    }
}

public struct VesperBenchmarkThresholdViolation: Codable, Equatable, Sendable {
    public let measurement: String
    public let actual: Double
    public let threshold: Double
    public let comparison: String

    public init(
        measurement: String,
        actual: Double,
        threshold: Double,
        comparison: String
    ) {
        self.measurement = measurement
        self.actual = actual
        self.threshold = threshold
        self.comparison = comparison
    }
}

public struct VesperPluginDiagnosticSeverity: RawRepresentable, Codable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let info = VesperPluginDiagnosticSeverity(rawValue: "info")
    public static let warning = VesperPluginDiagnosticSeverity(rawValue: "warning")
    public static let error = VesperPluginDiagnosticSeverity(rawValue: "error")

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        rawValue = try container.decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperPluginDiagnostic: Codable, Equatable, Sendable {
    public let code: String
    public let severity: VesperPluginDiagnosticSeverity
    public let message: String
    public let attributes: [String: String]

    public init(
        code: String,
        severity: VesperPluginDiagnosticSeverity,
        message: String,
        attributes: [String: String] = [:]
    ) {
        self.code = code
        self.severity = severity
        self.message = message
        self.attributes = attributes
    }
}

public struct VesperBenchmarkSinkReport: Codable, Equatable, Sendable {
    public let acceptedEvents: UInt64
    public let droppedEvents: UInt64
    public let measurements: [VesperPluginMeasurement]
    public let thresholdViolations: [VesperBenchmarkThresholdViolation]
    public let diagnostics: [VesperPluginDiagnostic]

    public init(
        acceptedEvents: UInt64,
        droppedEvents: UInt64,
        measurements: [VesperPluginMeasurement] = [],
        thresholdViolations: [VesperBenchmarkThresholdViolation] = [],
        diagnostics: [VesperPluginDiagnostic] = []
    ) {
        self.acceptedEvents = acceptedEvents
        self.droppedEvents = droppedEvents
        self.measurements = measurements
        self.thresholdViolations = thresholdViolations
        self.diagnostics = diagnostics
    }
}

public struct VesperBenchmarkSummary: Codable, Equatable, Sendable {
    public let runId: String
    public let sessionId: String
    public let acceptedEvents: UInt64
    public let droppedEvents: UInt64
    public let pluginAcceptedEvents: UInt64
    public let pluginDroppedEvents: UInt64
    public let metrics: [VesperBenchmarkMetricSummary]
    public let pluginFinalReport: VesperBenchmarkSinkReport?
    public let pluginErrors: [String]
}

@MainActor
final class VesperBenchmarkRecorder {
    private let configuration: VesperBenchmarkConfiguration
    private let runId = UUID().uuidString
    private let sessionId = UUID().uuidString
    private let baseTimestampNs = DispatchTime.now().uptimeNanoseconds
    private var rawEvents: [VesperBenchmarkEvent] = []
    private var samplesByName: [String: [UInt64]] = [:]
    private let maxSamplesPerName = 10_000
    private var acceptedEvents: UInt64 = 0
    private var droppedEvents: UInt64 = 0
    private var disposed = false

    init(configuration: VesperBenchmarkConfiguration) {
        self.configuration = configuration
    }

    var isEnabled: Bool {
        configuration.enabled && !disposed
    }

    func record(
        _ eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: [String: String] = [:]
    ) {
        guard isEnabled else {
            return
        }
        let now = DispatchTime.now().uptimeNanoseconds
        let elapsed = now >= baseTimestampNs ? now - baseTimestampNs : 0
        acceptedEvents += 1
        var samples = samplesByName[eventName, default: []]
        if samples.count >= maxSamplesPerName {
            samples.removeFirst(samples.count - maxSamplesPerName + 1)
        }
        samples.append(elapsed)
        samplesByName[eventName] = samples

        let event = VesperBenchmarkEvent(
            runId: runId,
            sessionId: sessionId,
            platform: "ios",
            sourceProtocol: sourceProtocol?.rawValue,
            eventName: eventName,
            timestampNs: now,
            elapsedNs: elapsed,
            thread: Thread.isMainThread ? "main" : (Thread.current.name ?? "background"),
            attributes: attributes
        )

        if configuration.includeRawEvents {
            if rawEvents.count < configuration.maxBufferedEvents {
                rawEvents.append(event)
            } else {
                droppedEvents += 1
            }
        }
    }

    func drainEvents() -> [VesperBenchmarkEvent] {
        let events = rawEvents
        rawEvents.removeAll(keepingCapacity: true)
        return events
    }

    func summary() -> VesperBenchmarkSummary {
        let pluginErrors = configuration.enabled && !configuration.pluginReferences.isEmpty
            ? ["Benchmark sink plugins have been retired. Benchmark events are recorded by the host."]
            : []
        return VesperBenchmarkSummary(
            runId: runId,
            sessionId: sessionId,
            acceptedEvents: acceptedEvents,
            droppedEvents: droppedEvents,
            pluginAcceptedEvents: 0,
            pluginDroppedEvents: 0,
            metrics: samplesByName
                .map { name, samples in metricSummary(name: name, samples: samples) }
                .sorted { $0.name < $1.name },
            pluginFinalReport: nil,
            pluginErrors: pluginErrors
        )
    }

    func dispose() {
        disposed = true
    }

    private func metricSummary(
        name: String,
        samples: [UInt64]
    ) -> VesperBenchmarkMetricSummary {
        let sorted = samples.sorted()
        return VesperBenchmarkMetricSummary(
            name: name,
            count: sorted.count,
            minNs: sorted.first ?? 0,
            maxNs: sorted.last ?? 0,
            p50Ns: percentile(sorted, ratio: 0.50),
            p90Ns: percentile(sorted, ratio: 0.90),
            p95Ns: percentile(sorted, ratio: 0.95)
        )
    }

    private func percentile(_ sorted: [UInt64], ratio: Double) -> UInt64 {
        guard !sorted.isEmpty else {
            return 0
        }
        let index = Int((Double(sorted.count - 1) * ratio).rounded(.up))
        return sorted[min(max(index, 0), sorted.count - 1)]
    }

}
