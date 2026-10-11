import Flutter
import Foundation
import CoreFoundation
import VesperPlayerKit

enum PluginError: LocalizedError {
    case missingArgument(String)
    case invalidNestedMap(String)
    case invalidSource(String)
    case invalidTrackSelection(String)
    case invalidAbrPolicy(String)
    case unsupported(String)
    case operationFailed(String)
    case unknownPlayer(String)
    case unknownDownload(String)

    var errorDescription: String? {
        switch self {
        case let .missingArgument(argument):
            "Missing \(argument)."
        case let .invalidNestedMap(key):
            "Invalid \(key): expected a map."
        case let .invalidSource(message):
            message
        case let .invalidTrackSelection(message):
            message
        case let .invalidAbrPolicy(message):
            message
        case let .unsupported(message):
            message
        case let .operationFailed(message):
            message
        case let .unknownPlayer(playerId):
            "Unknown playerId: \(playerId)"
        case let .unknownDownload(downloadId):
            "Unknown downloadId: \(downloadId)"
        }
    }
}

struct PluginSubtitleError: LocalizedError {
    let code: String
    let phase: String
    let trackId: String?
    let retriable: Bool
    let message: String
    let commandId: UInt64?
    let sourceEpoch: UInt64?

    init(
        code: String,
        phase: String,
        trackId: String? = nil,
        retriable: Bool = false,
        message: String,
        commandId: UInt64? = nil,
        sourceEpoch: UInt64? = nil
    ) {
        self.code = code
        self.phase = phase
        self.trackId = trackId
        self.retriable = retriable
        self.message = message
        self.commandId = commandId
        self.sourceEpoch = sourceEpoch
    }

    var errorDescription: String? { message }
}

func arguments(of call: FlutterMethodCall) -> [String: Any] {
    stringKeyedMap(call.arguments) ?? [:]
}

func nestedMap(_ value: Any?) throws -> [String: Any]? {
    guard let value else { return nil }
    if value is NSNull {
        return nil
    }
    if let map = stringKeyedMap(value) {
        return map
    }
    throw PluginError.invalidNestedMap("value")
}

func requireNestedMap(arguments: [String: Any], key: String) throws -> [String: Any] {
    guard let raw = arguments[key] else {
        throw PluginError.missingArgument(key)
    }
    guard let map = stringKeyedMap(raw) else {
        throw PluginError.invalidNestedMap(key)
    }
    return map
}

func stringKeyedMap(_ value: Any?) -> [String: Any]? {
    if let map = value as? [String: Any] {
        return map
    }
    if let map = value as? [AnyHashable: Any] {
        var normalized: [String: Any] = [:]
        normalized.reserveCapacity(map.count)
        for (key, value) in map {
            guard let stringKey = key as? String else {
                return nil
            }
            normalized[stringKey] = value
        }
        return normalized
    }
    if let dictionary = value as? NSDictionary {
        var normalized: [String: Any] = [:]
        normalized.reserveCapacity(dictionary.count)
        for (rawKey, rawValue) in dictionary {
            guard let stringKey = rawKey as? String else {
                return nil
            }
            normalized[stringKey] = rawValue
        }
        return normalized
    }
    return nil
}

func stringMap(_ value: Any?) -> [String: String] {
    guard let raw = stringKeyedMap(value), !raw.isEmpty else {
        return [:]
    }

    var decoded: [String: String] = [:]
    decoded.reserveCapacity(raw.count)
    for (key, value) in raw {
        guard let stringValue = value as? String else {
            continue
        }
        decoded[key] = stringValue
    }
    return decoded
}

func toVesperPluginReferences(
    _ value: Any?,
    fieldName: String = "pluginReferences"
) throws -> [VesperPluginReference] {
    guard let value, !(value is NSNull) else {
        return []
    }
    guard let entries = value as? [Any] else {
        throw PluginError.operationFailed("\(fieldName) must be a list")
    }

    return try entries.map { entry in
        guard let map = stringKeyedMap(entry),
              let pluginId = map["pluginId"] as? String,
              let transport = map["transport"] as? String,
              !transport.isEmpty
        else {
            throw PluginError.operationFailed("Invalid \(fieldName) entry")
        }
        let capabilityInstanceId = try optionalInputString(
            map["capabilityInstanceId"], field: "capabilityInstanceId"
        )
        return try VesperPluginReference(
            pluginId: pluginId,
            capabilityInstanceId: capabilityInstanceId,
            transportRawValue: transport
        )
    }
}

func optionalInputString(_ value: Any?, field: String) throws -> String? {
    guard let value, !(value is NSNull) else { return nil }
    guard let string = value as? String else {
        throw PluginError.invalidSource("\(field) must be a string.")
    }
    return string
}

func requiredInputString(_ value: Any?, field: String) throws -> String {
    guard let string = try optionalInputString(value, field: field) else {
        throw PluginError.invalidSource("\(field) must be a string.")
    }
    return string
}

func inputRecordList(_ value: Any?, field: String) throws -> [[String: Any]] {
    guard let value, !(value is NSNull) else { return [] }
    guard let entries = value as? [Any] else {
        throw PluginError.invalidSource("\(field) must be a list.")
    }
    return try entries.enumerated().map { index, entry in
        guard let map = stringKeyedMap(entry) else {
            throw PluginError.invalidSource("\(field)[\(index)] must be a map with string keys.")
        }
        return map
    }
}

func inputIdentityList(_ value: Any?, field: String) throws -> [String] {
    guard let value, !(value is NSNull) else { return [] }
    guard let entries = value as? [Any] else {
        throw PluginError.invalidSource("\(field) must be a list.")
    }
    return try entries.enumerated().map { index, entry in
        try requiredInputString(entry, field: "\(field)[\(index)]")
    }
}

func inputDownloadMetadata(_ value: Any?) throws -> [String: String] {
    guard let map = try nestedMap(value) else { return [:] }
    return try map.mapValues { try requiredInputString($0, field: "download metadata value") }
}

func inputDownloadOutputFormat(_ value: Any?) throws -> VesperDownloadOutputFormat? {
    switch try optionalInputString(value, field: "targetOutputFormat") {
    case nil: return nil
    case "original": return .original
    case "mp4": return .mp4
    case "mkv": return .mkv
    default: throw PluginError.invalidSource("Unknown targetOutputFormat.")
    }
}

func inputDownloadStreamKind(_ value: Any?) throws -> VesperDownloadStreamKind {
    switch try optionalInputString(value, field: "kind")?.lowercased() {
    case nil, "combined": return .combined
    case "video": return .video
    case "audio": return .audio
    case "secondaryaudio": return .secondaryAudio
    case "subtitle": return .subtitle
    case "auxiliary": return .auxiliary
    default: throw PluginError.invalidSource("Unknown download stream kind.")
    }
}

private func inputInteger<Integer: FixedWidthInteger>(
    _ value: Any, field: String, as: Integer.Type
) throws -> Integer {
    guard let number = value as? NSNumber,
          CFGetTypeID(number) != CFBooleanGetTypeID() else {
        throw PluginError.invalidSource("\(field) must be an integer.")
    }
    // Parse integer-backed NSNumber values without passing through lossy Double precision.
    let type = String(cString: number.objCType)
    guard ["c", "C", "s", "S", "i", "I", "l", "L", "q", "Q"].contains(type),
          let integer = Integer(number.stringValue) else {
        throw PluginError.invalidSource("\(field) must be an integer in range.")
    }
    return integer
}

func inputDownloadByteRangeInteger(_ value: Any?, field: String) throws -> UInt64 {
    guard let value, !(value is NSNull) else { return 0 }
    return try inputInteger(value, field: field, as: UInt64.self)
}

func inputRetryMaxAttempts(_ value: Any?) throws -> Int? {
    guard let value, !(value is NSNull) else { return nil }
    return try inputInteger(value, field: "maxAttempts", as: Int.self)
}
