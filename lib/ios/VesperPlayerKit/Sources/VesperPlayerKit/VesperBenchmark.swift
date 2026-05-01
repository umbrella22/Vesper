import Foundation
import VesperPlayerKitBridgeShim

public struct VesperBenchmarkConfiguration: Equatable {
    public let enabled: Bool
    public let maxBufferedEvents: Int
    public let includeRawEvents: Bool
    public let pluginLibraryPaths: [String]

    public init(
        enabled: Bool = false,
        maxBufferedEvents: Int = 2_048,
        includeRawEvents: Bool = true,
        pluginLibraryPaths: [String] = []
    ) {
        self.enabled = enabled
        self.maxBufferedEvents = max(maxBufferedEvents, 0)
        self.includeRawEvents = includeRawEvents
        self.pluginLibraryPaths = pluginLibraryPaths
    }

    public static let disabled = VesperBenchmarkConfiguration()
}

public struct VesperBenchmarkEvent: Codable, Equatable {
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

public struct VesperBenchmarkMetricSummary: Codable, Equatable {
    public let name: String
    public let count: Int
    public let minNs: UInt64
    public let maxNs: UInt64
    public let p50Ns: UInt64
    public let p90Ns: UInt64
    public let p95Ns: UInt64
}

public struct VesperBenchmarkSummary: Codable, Equatable {
    public let runId: String
    public let sessionId: String
    public let acceptedEvents: UInt64
    public let droppedEvents: UInt64
    public let pluginAcceptedEvents: UInt64
    public let pluginDroppedEvents: UInt64
    public let metrics: [VesperBenchmarkMetricSummary]
    public let pluginErrors: [String]
}

private struct VesperBenchmarkEventBatchPayload: Encodable {
    let events: [VesperBenchmarkEvent]
}

private struct VesperBenchmarkSinkReportPayload: Decodable {
    let acceptedEvents: UInt64
    let droppedEvents: UInt64
    let pluginErrors: [String]
}

@MainActor
final class VesperBenchmarkRecorder {
    private let configuration: VesperBenchmarkConfiguration
    private let runId = UUID().uuidString
    private let sessionId = UUID().uuidString
    private let baseTimestampNs = DispatchTime.now().uptimeNanoseconds
    private var rawEvents: [VesperBenchmarkEvent] = []
    private var samplesByName: [String: [UInt64]] = [:]
    private var acceptedEvents: UInt64 = 0
    private var droppedEvents: UInt64 = 0
    private var pluginAcceptedEvents: UInt64 = 0
    private var pluginDroppedEvents: UInt64 = 0
    private var pluginErrors: [String] = []
    private let sinkSession: VesperBenchmarkSinkSession?

    init(configuration: VesperBenchmarkConfiguration) {
        self.configuration = configuration
        if configuration.enabled, !configuration.pluginLibraryPaths.isEmpty {
            do {
                sinkSession = try VesperBenchmarkSinkSession(
                    pluginLibraryPaths: configuration.pluginLibraryPaths
                )
            } catch {
                sinkSession = nil
                pluginErrors.append(error.localizedDescription)
            }
        } else {
            sinkSession = nil
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
        guard configuration.enabled else {
            return
        }
        let now = DispatchTime.now().uptimeNanoseconds
        let elapsed = now >= baseTimestampNs ? now - baseTimestampNs : 0
        acceptedEvents += 1
        samplesByName[eventName, default: []].append(elapsed)

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

        submitBenchmarkEvents([event])
    }

    func drainEvents() -> [VesperBenchmarkEvent] {
        let events = rawEvents
        rawEvents.removeAll(keepingCapacity: true)
        return events
    }

    func summary() -> VesperBenchmarkSummary {
        VesperBenchmarkSummary(
            runId: runId,
            sessionId: sessionId,
            acceptedEvents: acceptedEvents,
            droppedEvents: droppedEvents,
            pluginAcceptedEvents: pluginAcceptedEvents,
            pluginDroppedEvents: pluginDroppedEvents,
            metrics: samplesByName
                .map { name, samples in metricSummary(name: name, samples: samples) }
                .sorted { $0.name < $1.name },
            pluginErrors: pluginErrors
        )
    }

    func dispose() {
        flushSinks()
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

    private func submitBenchmarkEvents(_ events: [VesperBenchmarkEvent]) {
        guard let sinkSession, !events.isEmpty else {
            return
        }

        do {
            let report = try sinkSession.submit(events)
            pluginAcceptedEvents += report.acceptedEvents
            pluginDroppedEvents += report.droppedEvents
            pluginErrors.append(contentsOf: report.pluginErrors)
        } catch {
            pluginErrors.append(error.localizedDescription)
        }
    }

    private func flushSinks() {
        guard let sinkSession else {
            return
        }

        do {
            let report = try sinkSession.flush()
            pluginErrors.append(contentsOf: report.pluginErrors)
        } catch {
            pluginErrors.append(error.localizedDescription)
        }
    }
}

private final class VesperBenchmarkSinkSession {
    private let handle: UInt64
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(pluginLibraryPaths: [String]) throws {
        var pathPointers = makeCStringList(pluginLibraryPaths)
        defer { freeCStringList(&pathPointers, count: pluginLibraryPaths.count) }

        var handle: UInt64 = 0
        var errorMessage: UnsafeMutablePointer<CChar>?
        let created = withUnsafeMutablePointer(to: &handle) { handlePointer in
            withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                vesper_runtime_benchmark_sink_session_create(
                    pathPointers,
                    UInt(pluginLibraryPaths.count),
                    handlePointer,
                    errorPointer
                )
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
        vesper_runtime_benchmark_sink_session_dispose(handle)
    }

    func submit(_ events: [VesperBenchmarkEvent]) throws -> VesperBenchmarkSinkReportPayload {
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
        try executeReportCall { reportPointer, errorPointer in
            vesper_runtime_benchmark_sink_session_flush_json(
                handle,
                reportPointer,
                errorPointer
            )
        }
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
    for (index, value) in values.enumerated() {
        pointer[index] = strdup(value)
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
