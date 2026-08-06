import Foundation

private let maxPipelineEventHookReports = 1_024
private let maxPipelineEventHookMeasurements = 128
private let maxPipelineEventHookDiagnostics = 64
private let maxPipelineEventHookAttributes = 32
private let maxPipelineEventHookAttributeKeyBytes = 64
private let maxPipelineEventHookAttributeValueBytes = 256
private let maxPipelineEventHookMessageBytes = 256

/// The raw result status returned by a playback EventHook.
public struct VesperPipelineEventHookResultStatus: RawRepresentable, Codable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let accepted = VesperPipelineEventHookResultStatus(rawValue: "accepted")
    public static let rejected = VesperPipelineEventHookResultStatus(rawValue: "rejected")
    public static let error = VesperPipelineEventHookResultStatus(rawValue: "error")

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        rawValue = try container.decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

/// The raw error code returned by a playback EventHook.
public struct VesperPipelineEventHookErrorCode: RawRepresentable, Codable, Equatable, Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public static let invalidInput = VesperPipelineEventHookErrorCode(rawValue: "invalidInput")
    public static let payloadCodec = VesperPipelineEventHookErrorCode(rawValue: "payloadCodec")
    public static let abiViolation = VesperPipelineEventHookErrorCode(rawValue: "abiViolation")
    public static let rejected = VesperPipelineEventHookErrorCode(rawValue: "rejected")
    public static let failed = VesperPipelineEventHookErrorCode(rawValue: "failed")
    public static let protocolViolation = VesperPipelineEventHookErrorCode(rawValue: "protocolViolation")

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        rawValue = try container.decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

/// A structured measurement returned by a playback EventHook.
public struct VesperPipelineEventHookMeasurement: Codable, Equatable, Sendable {
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

    fileprivate func toMap() -> [String: Any] {
        [
            "name": name,
            "value": value,
            "unit": unit,
            "attributes": attributes,
        ]
    }
}

/// A structured diagnostic returned by a playback EventHook.
public struct VesperPipelineEventHookDiagnostic: Codable, Equatable, Sendable {
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

    fileprivate func toMap() -> [String: Any] {
        [
            "code": code,
            "severity": severity.rawValue,
            "message": message,
            "attributes": attributes,
        ]
    }
}

/// The successful outcome returned by a playback EventHook.
public struct VesperPipelineEventHookOutcome: Codable, Equatable, Sendable {
    public let accepted: Bool
    public let measurements: [VesperPipelineEventHookMeasurement]
    public let diagnostics: [VesperPipelineEventHookDiagnostic]

    public init(
        accepted: Bool,
        measurements: [VesperPipelineEventHookMeasurement] = [],
        diagnostics: [VesperPipelineEventHookDiagnostic] = []
    ) {
        self.accepted = accepted
        self.measurements = measurements
        self.diagnostics = diagnostics
    }

    fileprivate func toMap() -> [String: Any] {
        [
            "accepted": accepted,
            "measurements": measurements.map { $0.toMap() },
            "diagnostics": diagnostics.map { $0.toMap() },
        ]
    }
}

/// An error returned by a playback EventHook.
public struct VesperPipelineEventHookError: Codable, Equatable, Sendable {
    public let code: VesperPipelineEventHookErrorCode
    public let message: String

    public init(code: VesperPipelineEventHookErrorCode, message: String) {
        self.code = code
        self.message = message
    }

    fileprivate func toMap() -> [String: Any] {
        [
            "code": code.rawValue,
            "message": message,
        ]
    }
}

/// The typed result of one playback EventHook invocation.
public struct VesperPipelineEventHookResult: Codable, Equatable, Sendable {
    public let status: VesperPipelineEventHookResultStatus
    public let outcome: VesperPipelineEventHookOutcome?
    public let error: VesperPipelineEventHookError?

    public init(
        status: VesperPipelineEventHookResultStatus,
        outcome: VesperPipelineEventHookOutcome? = nil,
        error: VesperPipelineEventHookError? = nil
    ) {
        self.status = status
        self.outcome = outcome
        self.error = error
    }

    fileprivate func toMap() -> [String: Any] {
        [
            "status": status.rawValue,
            "outcome": outcome.map { $0.toMap() } ?? NSNull(),
            "error": error.map { $0.toMap() } ?? NSNull(),
        ]
    }
}

/// A typed report produced after one playback pipeline event reaches a hook.
public struct VesperPipelineEventHookReport: Equatable, Sendable {
    public let pluginId: String
    public let capabilityInstanceId: String?
    public let transport: VesperPluginTransport
    public let runId: String
    public let sessionId: String
    public let eventName: String
    public let result: VesperPipelineEventHookResult

    public init(
        pluginId: String,
        capabilityInstanceId: String?,
        transport: VesperPluginTransport,
        runId: String,
        sessionId: String,
        eventName: String,
        result: VesperPipelineEventHookResult
    ) {
        self.pluginId = pluginId
        self.capabilityInstanceId = capabilityInstanceId
        self.transport = transport
        self.runId = runId
        self.sessionId = sessionId
        self.eventName = eventName
        self.result = result
    }

    fileprivate func toMap() -> [String: Any] {
        [
            "pluginId": pluginId,
            "capabilityInstanceId": capabilityInstanceId.map { $0 as Any } ?? NSNull(),
            "transport": transport.rawValue,
            "runId": runId,
            "sessionId": sessionId,
            "eventName": eventName,
            "result": result.toMap(),
        ]
    }
}

/// A bounded batch drained from one playback EventHook dispatcher.
public struct VesperPipelineEventHookReportBatch {
    /// Reports produced by the selected hooks.
    public let reports: [VesperPipelineEventHookReport]
    /// Events rejected before reaching the hook worker.
    public let droppedEvents: UInt64
    /// Reports discarded because the host-side report queue was full.
    public let droppedReports: UInt64
    /// The first dispatcher or decoding error observed by the session.
    public let dispatcherError: String?

    public init(
        reports: [VesperPipelineEventHookReport] = [],
        droppedEvents: UInt64 = 0,
        droppedReports: UInt64 = 0,
        dispatcherError: String? = nil
    ) {
        self.reports = reports
        self.droppedEvents = droppedEvents
        self.droppedReports = droppedReports
        self.dispatcherError = dispatcherError
    }

    public var isEmpty: Bool {
        reports.isEmpty &&
            droppedEvents == 0 &&
            droppedReports == 0 &&
            dispatcherError == nil
    }

    /// Returns the stable host wire representation for this report batch.
    public func toMap() -> [String: Any] {
        [
            "reports": reports.map { $0.toMap() },
            "droppedEvents": droppedEvents,
            "droppedReports": droppedReports,
            "dispatcherError": dispatcherError.map { $0 as Any } ?? NSNull(),
        ]
    }
}

private enum VesperPipelineEventHookDecodeError: LocalizedError {
    case invalid(String)

    var errorDescription: String? {
        switch self {
        case let .invalid(message):
            message
        }
    }
}

/// Decodes the native report envelope into typed host models.
func decodeVesperPipelineEventHookReportBatch(
    data: Data?,
    bridgeError: String?
) -> VesperPipelineEventHookReportBatch {
    guard let data,
          let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else {
        return VesperPipelineEventHookReportBatch(
            dispatcherError: bridgeError ?? "invalid playback EventHook report payload"
        )
    }

    do {
        guard let rawReports = payload["reports"] as? [Any] else {
            throw VesperPipelineEventHookDecodeError.invalid(
                "playback EventHook report payload did not contain a reports array"
            )
        }
        guard rawReports.count <= maxPipelineEventHookReports else {
            throw VesperPipelineEventHookDecodeError.invalid(
                "playback EventHook report batch exceeds the 1024-report limit"
            )
        }
        let reports = try rawReports.map(decodeVesperPipelineEventHookReport)
        return VesperPipelineEventHookReportBatch(
            reports: reports,
            droppedEvents: try decodeUnsignedCounter(payload["droppedEvents"]),
            droppedReports: try decodeUnsignedCounter(payload["droppedReports"]),
            dispatcherError: payload["dispatcherError"] as? String
        )
    } catch {
        return VesperPipelineEventHookReportBatch(
            dispatcherError: bridgeError ?? error.localizedDescription
        )
    }
}

private func decodeVesperPipelineEventHookReport(_ rawValue: Any) throws
    -> VesperPipelineEventHookReport
{
    guard let report = rawValue as? [String: Any] else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook report entry was not an object"
        )
    }
    return VesperPipelineEventHookReport(
        pluginId: try decodeRequiredString(report["pluginId"], field: "pluginId"),
        capabilityInstanceId: try decodeOptionalString(
            report["capabilityInstanceId"],
            field: "capabilityInstanceId"
        ),
        transport: VesperPluginTransport(
            rawValue: try decodeRequiredString(report["transport"], field: "transport")
        ),
        runId: try decodeRequiredString(report["runId"], field: "runId"),
        sessionId: try decodeRequiredString(report["sessionId"], field: "sessionId"),
        eventName: try decodeRequiredString(report["eventName"], field: "eventName"),
        result: try decodeVesperPipelineEventHookResult(report["result"])
    )
}

private func decodeVesperPipelineEventHookResult(_ rawValue: Any?) throws
    -> VesperPipelineEventHookResult
{
    guard let result = rawValue as? [String: Any] else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook report result was not an object"
        )
    }
    let outcome = try decodeOptionalObject(result["outcome"], field: "outcome")
        .map(decodeVesperPipelineEventHookOutcome)
    let error = try decodeOptionalObject(result["error"], field: "error")
        .map(decodeVesperPipelineEventHookError)
    return VesperPipelineEventHookResult(
        status: VesperPipelineEventHookResultStatus(
            rawValue: try decodeRequiredString(result["status"], field: "status")
        ),
        outcome: outcome,
        error: error
    )
}

private func decodeVesperPipelineEventHookOutcome(_ value: [String: Any]) throws
    -> VesperPipelineEventHookOutcome
{
    let rawMeasurements = try decodeArray(value["measurements"], field: "measurements")
    guard rawMeasurements.count <= maxPipelineEventHookMeasurements else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook outcome exceeds the 128-measurement limit"
        )
    }
    let rawDiagnostics = try decodeArray(value["diagnostics"], field: "diagnostics")
    guard rawDiagnostics.count <= maxPipelineEventHookDiagnostics else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook outcome exceeds the 64-diagnostic limit"
        )
    }
    return VesperPipelineEventHookOutcome(
        accepted: try decodeBool(value["accepted"], field: "accepted"),
        measurements: try rawMeasurements.map(decodeVesperPipelineEventHookMeasurement),
        diagnostics: try rawDiagnostics.map(decodeVesperPipelineEventHookDiagnostic)
    )
}

private func decodeVesperPipelineEventHookMeasurement(_ rawValue: Any) throws
    -> VesperPipelineEventHookMeasurement
{
    guard let value = rawValue as? [String: Any],
          let number = value["value"] as? NSNumber,
          number.doubleValue.isFinite
    else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook measurement was malformed"
        )
    }
    return VesperPipelineEventHookMeasurement(
        name: try decodeRequiredString(value["name"], field: "measurement.name"),
        value: number.doubleValue,
        unit: try decodeRequiredString(value["unit"], field: "measurement.unit"),
        attributes: try decodeAttributes(value["attributes"])
    )
}

private func decodeVesperPipelineEventHookDiagnostic(_ rawValue: Any) throws
    -> VesperPipelineEventHookDiagnostic
{
    guard let value = rawValue as? [String: Any] else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook diagnostic was malformed"
        )
    }
    return VesperPipelineEventHookDiagnostic(
        code: try decodeRequiredString(value["code"], field: "diagnostic.code"),
        severity: VesperPluginDiagnosticSeverity(
            rawValue: try decodeRequiredString(value["severity"], field: "diagnostic.severity")
        ),
        message: try decodeRequiredString(value["message"], field: "diagnostic.message"),
        attributes: try decodeAttributes(value["attributes"])
    )
}

private func decodeVesperPipelineEventHookError(_ value: [String: Any]) throws
    -> VesperPipelineEventHookError
{
    VesperPipelineEventHookError(
        code: VesperPipelineEventHookErrorCode(
            rawValue: try decodeRequiredString(value["code"], field: "error.code")
        ),
        message: try decodeRequiredString(value["message"], field: "error.message")
    )
}

private func decodeAttributes(_ value: Any?) throws -> [String: String] {
    guard let value else { return [:] }
    if value is NSNull { return [:] }
    guard let attributes = value as? [String: Any] else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook attributes were not an object"
        )
    }
    guard attributes.count <= maxPipelineEventHookAttributes else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook attributes exceed the 32-entry limit"
        )
    }
    var decoded: [String: String] = [:]
    decoded.reserveCapacity(attributes.count)
    for (key, rawValue) in attributes {
        guard !key.isEmpty,
              key.utf8.count <= maxPipelineEventHookAttributeKeyBytes,
              let value = rawValue as? String,
              !value.isEmpty,
              value.utf8.count <= maxPipelineEventHookAttributeValueBytes
        else {
            throw VesperPipelineEventHookDecodeError.invalid(
                "playback EventHook attribute exceeded the protocol text limits"
            )
        }
        decoded[key] = value
    }
    return decoded
}

private func decodeRequiredString(_ value: Any?, field: String) throws -> String {
    guard let value = value as? String, !value.isEmpty else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook \(field) was missing or empty"
        )
    }
    if value.utf8.count > maxPipelineEventHookMessageBytes &&
        (field == "error.message" || field == "diagnostic.message")
    {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook \(field) exceeds the 256-byte limit"
        )
    }
    return value
}

private func decodeOptionalString(_ value: Any?, field: String) throws -> String? {
    guard let value, !(value is NSNull) else { return nil }
    guard let value = value as? String else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook \(field) was not a string"
        )
    }
    return value
}

private func decodeOptionalObject(_ value: Any?, field: String) throws -> [String: Any]? {
    guard let value, !(value is NSNull) else { return nil }
    guard let value = value as? [String: Any] else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook \(field) was not an object"
        )
    }
    return value
}

private func decodeArray(_ value: Any?, field: String) throws -> [Any] {
    guard let value else { return [] }
    guard let value = value as? [Any] else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook \(field) was not an array"
        )
    }
    return value
}

private func decodeBool(_ value: Any?, field: String) throws -> Bool {
    guard let value = value as? Bool else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook \(field) was not a boolean"
        )
    }
    return value
}

private func decodeUnsignedCounter(_ value: Any?) throws -> UInt64 {
    guard let value, !(value is NSNull) else { return 0 }
    guard let number = value as? NSNumber,
          number.doubleValue.rounded() == number.doubleValue,
          number.doubleValue >= 0,
          number.doubleValue <= Double(UInt64.max)
    else {
        throw VesperPipelineEventHookDecodeError.invalid(
            "playback EventHook counter was not a non-negative integer"
        )
    }
    return number.uint64Value
}
