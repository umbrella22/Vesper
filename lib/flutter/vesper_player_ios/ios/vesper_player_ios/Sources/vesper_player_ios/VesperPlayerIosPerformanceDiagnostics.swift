import Foundation
import CoreFoundation
import VesperPlayerKit

extension PlayerSession {
    @MainActor
    func requirePerformanceDiagnostics(
        arguments: [String: Any]
    ) throws -> VesperPerformanceDiagnosticsSession {
        guard let requestedRunId = arguments["runId"] as? String,
            !requestedRunId.isEmpty
        else {
            throw PluginError.missingArgument("runId")
        }
        guard let diagnostics = performanceDiagnosticsSession,
            diagnostics.runId == requestedRunId
        else {
            throw VesperPerformanceDiagnosticsError(
                code: .controllerDisposed,
                message: "The performance diagnostics session is no longer active."
            )
        }
        return diagnostics
    }
}

extension Dictionary where Key == String, Value == Any {
    func toPerformanceDiagnosticsConfiguration()
        throws -> VesperPerformanceDiagnosticsConfiguration
    {
        let includeRawEvents = try optionalPerformanceBool(
            "includeRawEvents",
            defaultValue: false,
            code: .invalidConfiguration
        )
        let maxRawEvents = keys.contains("maxRawEvents")
            ? try requiredPerformanceInt("maxRawEvents", code: .invalidConfiguration)
            : 256
        return VesperPerformanceDiagnosticsConfiguration(
            includeRawEvents: includeRawEvents,
            maxRawEvents: maxRawEvents
        )
    }

    func toPerformanceOverlayState() throws -> VesperPerformanceOverlayState {
        let basicCount = try optionalNonnegativeInt("loadedBasicItemCount")
        let advancedCount = try optionalNonnegativeInt("loadedAdvancedItemCount")
        return VesperPerformanceOverlayState(
            active: try optionalPerformanceBool("active", defaultValue: false),
            sampleClass: VesperPerformanceSampleClass(
                rawValue: self["sampleClass"] as? String ?? "steady"
            ),
            loadedBasicItemCount: basicCount,
            loadedAdvancedItemCount: advancedCount,
            advancedEffectsActive: try optionalPerformanceBool(
                "advancedEffectsActive",
                defaultValue: false
            )
        )
    }

    func toPerformanceFrameSample() throws -> VesperPerformanceFrameSample {
        let loadNs = try requiredPerformanceUInt64("loadNs")
        let budgetNs = try requiredPerformanceUInt64("budgetNs")
        let overlayState = try nestedMap(self["overlayState"])?
            .toPerformanceOverlayState()
        return VesperPerformanceFrameSample(
            loadNs: loadNs,
            budgetNs: budgetNs,
            overlayState: overlayState
        )
    }

    private func optionalNonnegativeInt(_ key: String) throws -> Int? {
        guard let rawValue = self[key], !(rawValue is NSNull) else { return nil }
        let value = try requiredPerformanceInt(key)
        guard value >= 0 else {
            throw VesperPerformanceDiagnosticsError(
                code: .protocolViolation,
                message: "\(key) must be a nonnegative platform integer."
            )
        }
        return value
    }

    func optionalPerformanceMarkerValue() throws -> Double? {
        guard let rawValue = self["value"], !(rawValue is NSNull) else { return nil }
        guard
            let number = rawValue as? NSNumber,
            !isPerformanceBoolean(number),
            number.doubleValue.isFinite
        else {
            throw performanceMappingError(
                .protocolViolation,
                "Performance marker value must be a finite number."
            )
        }
        return number.doubleValue
    }

    func optionalPerformanceSequenceIndex() throws -> Int? {
        guard let value = self["sequenceIndex"], !(value is NSNull) else { return nil }
        return try requiredPerformanceInt("sequenceIndex")
    }

    func optionalPerformanceExpectedOverlayActive() throws -> Bool? {
        guard let value = self["expectedOverlayActive"], !(value is NSNull) else {
            return nil
        }
        guard let number = value as? NSNumber, isPerformanceBoolean(number) else {
            throw performanceMappingError(
                .protocolViolation,
                "expectedOverlayActive must be a boolean."
            )
        }
        return number.boolValue
    }

    private func requiredPerformanceUInt64(_ key: String) throws -> UInt64 {
        guard let number = self[key] as? NSNumber,
              !isPerformanceBoolean(number),
              let kind = performanceIntegerKind(number)
        else {
            throw performanceMappingError(
                .protocolViolation,
                "\(key) must be an integer."
            )
        }
        if kind.isUnsigned { return number.uint64Value }
        let value = number.int64Value
        guard value >= 0 else {
            throw performanceMappingError(
                .protocolViolation,
                "\(key) must be nonnegative."
            )
        }
        return UInt64(value)
    }

    private func requiredPerformanceInt(
        _ key: String,
        code: VesperPerformanceDiagnosticsErrorCode = .protocolViolation
    ) throws -> Int {
        guard let number = self[key] as? NSNumber,
              !isPerformanceBoolean(number),
              let kind = performanceIntegerKind(number)
        else {
            throw performanceMappingError(code, "\(key) must be an integer.")
        }
        if kind.isUnsigned {
            let value = number.uint64Value
            guard value <= UInt64(Int.max) else {
                throw performanceMappingError(code, "\(key) must be a platform integer.")
            }
            return Int(value)
        }
        let value = number.int64Value
        guard value >= Int64(Int.min), value <= Int64(Int.max) else {
            throw performanceMappingError(code, "\(key) must be a platform integer.")
        }
        return Int(value)
    }

    private func optionalPerformanceBool(
        _ key: String,
        defaultValue: Bool,
        code: VesperPerformanceDiagnosticsErrorCode = .protocolViolation
    ) throws -> Bool {
        guard keys.contains(key) else { return defaultValue }
        guard
            let number = self[key] as? NSNumber,
            isPerformanceBoolean(number)
        else {
            throw performanceMappingError(code, "\(key) must be a boolean.")
        }
        return number.boolValue
    }
}

private struct PerformanceIntegerKind {
    let isUnsigned: Bool
}

private func performanceIntegerKind(_ number: NSNumber) -> PerformanceIntegerKind? {
    switch String(cString: number.objCType) {
    case "c", "s", "i", "l", "q":
        return PerformanceIntegerKind(isUnsigned: false)
    case "C", "S", "I", "L", "Q":
        return PerformanceIntegerKind(isUnsigned: true)
    default:
        return nil
    }
}

private func isPerformanceBoolean(_ number: NSNumber) -> Bool {
    CFGetTypeID(number) == CFBooleanGetTypeID()
}

private func performanceMappingError(
    _ code: VesperPerformanceDiagnosticsErrorCode,
    _ message: String
) -> VesperPerformanceDiagnosticsError {
    VesperPerformanceDiagnosticsError(code: code, message: message)
}

extension VesperPerformanceDiagnosticsReport {
    func toFlutterMap() -> [String: Any] {
        [
            "schemaVersion": schemaVersion,
            "runId": runId,
            "sessionId": sessionId,
            "platform": platform,
            "probe": probe.rawValue,
            "durationNs": flutterSignedInteger(durationNs),
            "frameBudgetNs": flutterSignedInteger(frameBudgetNs),
            "cohorts": cohorts.mapValues { $0.toFlutterMap() },
            "playback": playback.toFlutterMap(),
            "diagnosis": [
                "kind": diagnosis.kind.rawValue,
                "confidence": diagnosis.confidence.rawValue,
                "evidenceCodes": diagnosis.evidenceCodes,
            ],
            "acceptedEvents": flutterSignedInteger(acceptedEvents),
            "droppedEvents": flutterSignedInteger(droppedEvents),
            "rawEventsDropped": flutterSignedInteger(rawEventsDropped),
            "diagnostics": diagnostics.map { diagnostic in
                [
                    "code": diagnostic.code,
                    "severity": diagnostic.severity.rawValue,
                    "message": diagnostic.message,
                    "attributes": diagnostic.attributes,
                ]
            },
            "rawEvents": rawEvents.map { event in
                [
                    "runId": event.runId,
                    "sessionId": event.sessionId,
                    "platform": event.platform,
                    "sourceProtocol": flutterValue(event.sourceProtocol),
                    "eventName": event.eventName,
                    "timestampNs": flutterSignedInteger(event.timestampNs),
                    "elapsedNs": flutterSignedInteger(event.elapsedNs),
                    "thread": flutterValue(event.thread),
                    "attributes": event.attributes,
                ]
            },
        ]
    }
}

private extension VesperPerformanceFrameCohort {
    func toFlutterMap() -> [String: Any] {
        [
            "sampleCount": flutterSignedInteger(sampleCount),
            "jankCount": flutterSignedInteger(jankCount),
            "severeJankCount": flutterSignedInteger(severeJankCount),
            "jankRatio": jankRatio,
            "severeJankRatio": severeJankRatio,
            "minLoadNs": flutterSignedInteger(minLoadNs),
            "p50LoadNs": flutterSignedInteger(p50LoadNs),
            "p95LoadNs": flutterSignedInteger(p95LoadNs),
            "maxLoadNs": flutterSignedInteger(maxLoadNs),
        ]
    }
}

private extension VesperPerformancePlaybackSummary {
    func toFlutterMap() -> [String: Any] {
        [
            "activeDurationNs": flutterSignedInteger(activeDurationNs),
            "droppedVideoFrames": flutterSignedInteger(droppedVideoFrames),
            "bufferingCount": flutterSignedInteger(bufferingCount),
            "bufferingDurationNs": flutterSignedInteger(bufferingDurationNs),
            "stallCount": flutterSignedInteger(stallCount),
        ]
    }
}

private func flutterSignedInteger(_ value: UInt64) -> Int64 {
    Int64(clamping: value)
}
