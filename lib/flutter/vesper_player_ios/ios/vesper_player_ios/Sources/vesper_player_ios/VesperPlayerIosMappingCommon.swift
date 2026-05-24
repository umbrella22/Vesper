import Flutter
import Foundation
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

