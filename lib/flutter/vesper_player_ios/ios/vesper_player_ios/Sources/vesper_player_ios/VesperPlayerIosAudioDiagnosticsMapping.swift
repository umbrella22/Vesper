import Foundation
import VesperPlayerKit

extension Dictionary where Key == String, Value == Any {
    private func positiveDiagnosticInteger(_ key: String) throws -> Int? {
        guard let raw = self[key], !(raw is NSNull) else { return nil }
        guard let number = raw as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              !["f", "d"].contains(String(cString: number.objCType)),
              let value = Int(number.stringValue), value > 0 else {
            throw PluginError.missingArgument("\(key) must be a positive integer")
        }
        return value
    }

    private func diagnosticString(_ key: String) throws -> String? {
        guard let raw = self[key], !(raw is NSNull) else { return nil }
        guard let value = raw as? String else { throw PluginError.missingArgument("\(key) must be a string") }
        return value
    }

    func toPlaybackStallPolicy() throws -> VesperPlaybackStallPolicy {
        let enabled: Bool
        if let raw = self["enabled"] {
            guard let number = raw as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else {
                throw PluginError.missingArgument("enabled must be a boolean")
            }
            enabled = number.boolValue
        } else { enabled = true }
        return try VesperPlaybackStallPolicy(enabled: enabled,
            positionThresholdMs: UInt64(positiveDiagnosticInteger("positionThresholdMs") ?? 5_000),
            bufferingThresholdMs: UInt64(positiveDiagnosticInteger("bufferingThresholdMs") ?? 15_000))
    }

    func toAudioDecoderCapabilityRequest() throws -> VesperAudioDecoderCapabilityRequest {
        try VesperAudioDecoderCapabilityRequest(sampleMimeType: diagnosticString("sampleMimeType"), codec: diagnosticString("codec"),
            channels: positiveDiagnosticInteger("channels"), sampleRate: positiveDiagnosticInteger("sampleRate"))
    }
}

extension VesperPlaybackStallObservation {
    func toMap() -> [String: Any] {
        ["playbackEpoch": playbackEpoch, "kind": kind.rawValue, "stalledForMs": stalledForMs,
         "elapsedSinceLoadStartMs": elapsedSinceLoadStartMs, "mediaPositionMs": mediaPositionMs, "audio": audio.toMap()]
    }
}

extension VesperAudioDecoderCapabilityResult {
    func toMap() -> [String: Any] {
        ["request": ["sampleMimeType": flutterValue(request.sampleMimeType), "codec": flutterValue(request.codec),
                     "channels": flutterValue(request.channels), "sampleRate": flutterValue(request.sampleRate)],
         "status": status.rawValue, "reason": reason, "evidence": evidence,
         "resolvedMimeType": flutterValue(resolvedMimeType),
         "candidates": candidates.map { ["name": $0.name, "hardwareAccelerated": flutterValue($0.hardwareAccelerated),
                                        "status": $0.status.rawValue, "reason": $0.reason] }]
    }
}
