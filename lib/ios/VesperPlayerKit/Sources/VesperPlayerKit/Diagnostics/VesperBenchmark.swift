import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

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

private struct VesperBenchmarkEventBatchPayload: Encodable {
    let events: [VesperBenchmarkEvent]
}

typealias VesperBenchmarkSinkReportPayload = VesperBenchmarkSinkReport

protocol VesperBenchmarkSinkSessionProtocol: AnyObject, Sendable {
    func submit(_ events: [VesperBenchmarkEvent]) throws -> VesperBenchmarkSinkReportPayload
    func flush() throws -> VesperBenchmarkSinkReportPayload
    func dispose()
}

typealias VesperBenchmarkSinkSessionFactory =
    @Sendable ([VesperPluginReference]) throws -> any VesperBenchmarkSinkSessionProtocol

private final class VesperBenchmarkShutdownWaiter: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Bool, Never>?

    init(continuation: CheckedContinuation<Bool, Never>) {
        self.continuation = continuation
    }

    func resume(returning value: Bool) {
        lock.lock()
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(returning: value)
    }
}

private struct VesperBenchmarkSinkStatsSnapshot {
    let acceptedEvents: UInt64
    let droppedEvents: UInt64
    let finalReport: VesperBenchmarkSinkReport?
    let errors: [String]

    static let empty = VesperBenchmarkSinkStatsSnapshot(
        acceptedEvents: 0,
        droppedEvents: 0,
        finalReport: nil,
        errors: []
    )
}

private final class VesperBenchmarkSinkWorker: @unchecked Sendable {
    private enum Command {
        case submit(VesperBenchmarkEvent)
        case flush
        case dispose
    }

    private let commandCapacity = 1_024
    private let maxErrors = 128
    private let commandWaitInterval: TimeInterval = 1
    private let condition = NSCondition()
    private var commands: [Command] = []
    private var accepting = true
    private let statsLock = NSLock()
    private var acceptedEvents: UInt64 = 0
    private var droppedEvents: UInt64 = 0
    private var finalReport: VesperBenchmarkSinkReport?
    private var errors: [String] = []
    private var didRecordShutdownTimeout = false
    private let pluginReferences: [VesperPluginReference]
    private let sessionFactory: VesperBenchmarkSinkSessionFactory
    private let workerQueue = DispatchQueue(
        label: "io.github.umbrella22.vesper.benchmark-sink",
        qos: .utility
    )
    private let shutdownGroup = DispatchGroup()
    private static let shutdownNotificationQueue = DispatchQueue(
        label: "io.github.umbrella22.vesper.benchmark-sink.shutdown",
        qos: .utility,
        attributes: .concurrent
    )

    init(
        pluginReferences: [VesperPluginReference],
        sessionFactory: @escaping VesperBenchmarkSinkSessionFactory
    ) {
        self.pluginReferences = pluginReferences
        self.sessionFactory = sessionFactory
        shutdownGroup.enter()
        workerQueue.async { [self] in
            defer { shutdownGroup.leave() }
            run()
        }
    }

    func offer(_ event: VesperBenchmarkEvent) {
        condition.lock()
        guard accepting else {
            condition.unlock()
            recordQueueDrop()
            return
        }
        guard commands.count < commandCapacity else {
            condition.unlock()
            recordQueueDrop()
            return
        }
        commands.append(.submit(event))
        condition.signal()
        condition.unlock()
    }

    func flush() {
        condition.lock()
        guard accepting, !commands.contains(where: { command in
            if case .flush = command { return true }
            return false
        }) else {
            condition.unlock()
            return
        }
        enqueueControlLocked(.flush)
        condition.unlock()
    }

    func dispose() {
        condition.lock()
        guard accepting else {
            condition.unlock()
            return
        }
        accepting = false
        enqueueControlLocked(.dispose)
        condition.unlock()
    }

    func snapshot() -> VesperBenchmarkSinkStatsSnapshot {
        statsLock.lock()
        defer { statsLock.unlock() }
        return VesperBenchmarkSinkStatsSnapshot(
            acceptedEvents: acceptedEvents,
            droppedEvents: droppedEvents,
            finalReport: finalReport,
            errors: errors
        )
    }

    func awaitShutdown(timeout: TimeInterval) async -> Bool {
        let boundedTimeout = max(timeout, 0)
        return await withCheckedContinuation { continuation in
            let waiter = VesperBenchmarkShutdownWaiter(continuation: continuation)
            shutdownGroup.notify(queue: Self.shutdownNotificationQueue) {
                waiter.resume(returning: true)
            }
            Self.shutdownNotificationQueue.asyncAfter(
                deadline: .now() + boundedTimeout
            ) {
                waiter.resume(returning: false)
            }
        }
    }

    func recordShutdownTimeout() {
        statsLock.lock()
        guard !didRecordShutdownTimeout else {
            statsLock.unlock()
            return
        }
        didRecordShutdownTimeout = true
        appendErrorLocked("benchmark sink shutdown timed out")
        statsLock.unlock()
    }

    private func enqueueControlLocked(_ command: Command) {
        if commands.count >= commandCapacity,
           let eventIndex = commands.lastIndex(where: { pending in
               if case .submit = pending { return true }
               return false
           })
        {
            commands.remove(at: eventIndex)
            recordQueueDrop()
        }
        guard commands.count < commandCapacity else {
            recordError("benchmark sink control queue is unavailable")
            return
        }
        commands.append(command)
        condition.signal()
    }

    private func run() {
        let session: (any VesperBenchmarkSinkSessionProtocol)?
        do {
            session = try sessionFactory(pluginReferences)
        } catch {
            session = nil
            recordError(error.localizedDescription)
        }
        defer { session?.dispose() }

        while true {
            let command = nextCommand()
            switch command {
            case let .submit(event):
                guard let session else { continue }
                executeSubmit { try session.submit([event]) }
            case .flush:
                guard let session else { continue }
                executeFlush { try session.flush() }
            case .dispose:
                if let session {
                    executeFlush { try session.flush() }
                }
                return
            }
        }
    }

    private func nextCommand() -> Command {
        condition.lock()
        while commands.isEmpty {
            _ = condition.wait(
                until: Date(timeIntervalSinceNow: commandWaitInterval)
            )
        }
        let command = commands.removeFirst()
        condition.unlock()
        return command
    }

    private func executeSubmit(
        _ operation: () throws -> VesperBenchmarkSinkReportPayload
    ) {
        do {
            recordSubmitReport(try operation())
        } catch {
            recordError(error.localizedDescription)
        }
    }

    private func executeFlush(
        _ operation: () throws -> VesperBenchmarkSinkReportPayload
    ) {
        do {
            recordFinalReport(try operation())
        } catch {
            recordError(error.localizedDescription)
        }
    }

    private func recordSubmitReport(_ report: VesperBenchmarkSinkReportPayload) {
        statsLock.lock()
        acceptedEvents += report.acceptedEvents
        droppedEvents += report.droppedEvents
        statsLock.unlock()
    }

    private func recordFinalReport(_ report: VesperBenchmarkSinkReportPayload) {
        statsLock.lock()
        finalReport = report
        statsLock.unlock()
    }

    private func recordQueueDrop() {
        statsLock.lock()
        droppedEvents += 1
        statsLock.unlock()
    }

    private func recordError(_ error: String) {
        statsLock.lock()
        appendErrorLocked(error)
        statsLock.unlock()
    }

    private func appendErrorLocked(_ error: String) {
        if errors.count >= maxErrors {
            errors.removeFirst(errors.count - maxErrors + 1)
        }
        errors.append(error)
    }
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
    private let sinkWorker: VesperBenchmarkSinkWorker?

    init(
        configuration: VesperBenchmarkConfiguration,
        sinkSessionFactory: @escaping VesperBenchmarkSinkSessionFactory = {
            try VesperBenchmarkSinkSession(pluginReferences: $0)
        }
    ) {
        self.configuration = configuration
        if configuration.enabled, !configuration.pluginReferences.isEmpty {
            sinkWorker = VesperBenchmarkSinkWorker(
                pluginReferences: configuration.pluginReferences,
                sessionFactory: sinkSessionFactory
            )
        } else {
            sinkWorker = nil
        }
    }

    var isEnabled: Bool {
        configuration.enabled
    }

    func record(
        _ eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: [String: String] = [:]
    ) {
        guard configuration.enabled, !disposed else {
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

        sinkWorker?.offer(event)
    }

    func drainEvents() -> [VesperBenchmarkEvent] {
        let events = rawEvents
        rawEvents.removeAll(keepingCapacity: true)
        return events
    }

    func summary() -> VesperBenchmarkSummary {
        let sinkStats = sinkWorker?.snapshot() ?? .empty
        return VesperBenchmarkSummary(
            runId: runId,
            sessionId: sessionId,
            acceptedEvents: acceptedEvents,
            droppedEvents: droppedEvents,
            pluginAcceptedEvents: sinkStats.acceptedEvents,
            pluginDroppedEvents: sinkStats.droppedEvents,
            metrics: samplesByName
                .map { name, samples in metricSummary(name: name, samples: samples) }
                .sorted { $0.name < $1.name },
            pluginFinalReport: sinkStats.finalReport,
            pluginErrors: sinkStats.errors
        )
    }

    func dispose() {
        guard !disposed else {
            return
        }
        disposed = true
        sinkWorker?.dispose()
    }

    func awaitSinkShutdown(timeout: TimeInterval) async -> Bool {
        guard let sinkWorker else {
            return true
        }
        let completed = await sinkWorker.awaitShutdown(timeout: timeout)
        if !completed {
            sinkWorker.recordShutdownTimeout()
        }
        return completed
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

// The worker owns this session and serializes every access on one queue.
private final class VesperBenchmarkSinkSession: VesperBenchmarkSinkSessionProtocol,
    @unchecked Sendable
{
    private var handle: UInt64
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(pluginReferences: [VesperPluginReference]) throws {
        let pluginRegistry = try VesperEmbeddedPluginRegistry.create(references: pluginReferences)
        let referenceJson = try encodeVesperPluginReferencesJSON(pluginReferences)

        var handle: UInt64 = 0
        var errorMessage: UnsafeMutablePointer<CChar>?
        let created = withExtendedLifetime(pluginRegistry) {
            withUnsafeMutablePointer(to: &handle) { handlePointer in
                withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                    vesper_runtime_benchmark_sink_session_create_with_references_json(
                        pluginRegistry.handle,
                        referenceJson,
                        handlePointer,
                        errorPointer
                    )
                }
            }
        }
        defer { freeBenchmarkCString(errorMessage) }

        guard created, handle != 0 else {
            throw VesperBenchmarkSinkSessionError.bridgeError(
                stringFromBenchmarkCString(errorMessage)
                    ?? "benchmark sink session create failed"
            )
        }
        self.handle = handle
    }

    deinit {
        dispose()
    }

    func submit(_ events: [VesperBenchmarkEvent]) throws -> VesperBenchmarkSinkReportPayload {
        guard handle != 0 else {
            throw VesperBenchmarkSinkSessionError.bridgeError(
                "benchmark sink session was disposed"
            )
        }
        let batch = VesperBenchmarkEventBatchPayload(events: events)
        let payload = try encoder.encode(batch)
        guard let json = String(data: payload, encoding: .utf8) else {
            throw VesperBenchmarkSinkSessionError.bridgeError(
                "benchmark batch payload was not valid UTF-8"
            )
        }

        return try json.withCString { pointer in
            try executeReportCall { reportPointer, errorPointer in
                vesper_runtime_benchmark_sink_session_submit_json(
                    handle,
                    pointer,
                    reportPointer,
                    errorPointer
                )
            }
        }
    }

    func flush() throws -> VesperBenchmarkSinkReportPayload {
        guard handle != 0 else {
            throw VesperBenchmarkSinkSessionError.bridgeError(
                "benchmark sink session was disposed"
            )
        }
        return try executeReportCall { reportPointer, errorPointer in
            vesper_runtime_benchmark_sink_session_flush_json(
                handle,
                reportPointer,
                errorPointer
            )
        }
    }

    func dispose() {
        guard handle != 0 else {
            return
        }
        vesper_runtime_benchmark_sink_session_dispose(handle)
        handle = 0
    }

    private func executeReportCall(
        _ call: (
            UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
            UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
        ) -> Bool
    ) throws -> VesperBenchmarkSinkReportPayload {
        var reportJson: UnsafeMutablePointer<CChar>?
        var errorMessage: UnsafeMutablePointer<CChar>?
        let succeeded = withUnsafeMutablePointer(to: &reportJson) { reportPointer in
            withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                call(reportPointer, errorPointer)
            }
        }
        defer {
            freeBenchmarkCString(reportJson)
            freeBenchmarkCString(errorMessage)
        }

        guard succeeded, let reportJson else {
            throw VesperBenchmarkSinkSessionError.bridgeError(
                stringFromBenchmarkCString(errorMessage)
                    ?? "benchmark sink session call failed"
            )
        }

        let reportString = String(cString: reportJson)
        return try decoder.decode(
            VesperBenchmarkSinkReportPayload.self,
            from: Data(reportString.utf8)
        )
    }
}

private enum VesperBenchmarkSinkSessionError: LocalizedError {
    case bridgeError(String)

    var errorDescription: String? {
        switch self {
        case let .bridgeError(message):
            message
        }
    }
}

private func makeCStringList(
    _ values: [String]
) -> UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>? {
    guard !values.isEmpty else {
        return nil
    }
    let pointer = UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>.allocate(
        capacity: values.count
    )
    pointer.initialize(repeating: nil, count: values.count)
    for (index, value) in values.enumerated() {
        guard let dup = strdup(value) else {
            for i in 0..<index {
                free(pointer[i])
            }
            pointer.deallocate()
            return nil
        }
        pointer[index] = dup
    }
    return pointer
}

private func freeCStringList(
    _ pointer: inout UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) {
    guard let rawPointer = pointer else {
        return
    }
    for index in 0..<count {
        free(rawPointer[index])
    }
    rawPointer.deallocate()
    pointer = nil
}

private func stringFromBenchmarkCString(_ pointer: UnsafeMutablePointer<CChar>?) -> String? {
    guard let pointer else {
        return nil
    }
    return String(cString: pointer)
}

private func freeBenchmarkCString(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else {
        return
    }
    vesper_runtime_benchmark_string_free(pointer)
}
