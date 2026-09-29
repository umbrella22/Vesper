import Flutter
import Foundation
import CoreFoundation
import VesperPlayerKit

@MainActor
final class VesperPluginSourceSession {
    let session: VesperSourceSession
    var handles: [String: VesperSourceHandle] = [:]
    // Retain only the latest task for each registered source, never a historical task log.
    var tasks: [String: VesperPreloadTask] = [:]
    init(_ session: VesperSourceSession) { self.session = session }
}

@MainActor
extension VesperPlayerIosPlugin {
    func sourceHandle(_ value: [String: Any]) throws -> VesperSourceHandle {
        guard let sessionId = value["sessionId"] as? String, let sourceId = value["sourceId"] as? String,
              let handle = sourceSessions[sessionId]?.handles[sourceId] else {
            throw PluginError.operationFailed("Unknown source reference.")
        }
        return handle
    }

    func handleSourceLifecycle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        do {
            let value = arguments(of: call)
            if call.method == "createSourceSession" {
                guard sourceSessions.count < 32 else { throw PluginError.operationFailed("Source session capacity exceeded.") }
                let config = try sourceOptionsMap(value["configuration"])
                let session = try VesperSourceSession(configuration: .init(
                    maxSources: try sourceInt(config, "maxSources", 128),
                    maxConcurrentPreloads: try sourceInt(config, "maxConcurrentPreloads", 2),
                    maxPendingPreloads: try sourceInt(config, "maxPendingPreloads", 4),
                    maxMemoryBytes: try sourceUInt(config, "maxMemoryBytes", 8 * 1024 * 1024)))
                sourceSessions[session.id] = VesperPluginSourceSession(session)
                result(["sessionId": session.id]); return
            }
            guard let id = value["sessionId"] as? String, let entry = sourceSessions[id] else {
                if ["releaseSource", "disposeSourceSession", "invalidateSourceSession", "cancelSourcePreload"].contains(call.method) { result(nil); return }
                throw PluginError.operationFailed("Unknown source session.")
            }
            switch call.method {
            case "registerSource":
                let descriptor = try requireNestedMap(arguments: value, key: "source").toVesperPlayerSource()
                let expiry = value["expiresAtEpochMs"] == nil || value["expiresAtEpochMs"] is NSNull
                    ? nil : try sourceUInt(value, "expiresAtEpochMs", 0)
                let handle = try entry.session.register(descriptor, expiresAtEpochMs: expiry)
                entry.handles[handle.id] = handle
                var reference: [String: Any] = ["sessionId": id, "sourceId": handle.id]
                if let expiry { reference["expiresAtEpochMs"] = expiry }
                result(reference)
            case "releaseSource":
                let sourceId = value["sourceId"] as? String ?? ""
                entry.handles.removeValue(forKey: sourceId)?.close()
                entry.tasks.removeValue(forKey: sourceId)
                result(nil)
            case "invalidateSourceSession", "disposeSourceSession":
                if call.method == "invalidateSourceSession" { entry.session.invalidate() } else { entry.session.close() }
                sourceSessions.removeValue(forKey: id)
                result(nil)
            case "preloadSource":
                let handle = try sourceHandle(value)
                let options = try sourceOptionsMap(value["options"])
                let task = try handle.preload(options: .init(maximumBytes: try sourceUInt(options, "maximumBytes", 8 * 1024 * 1024),
                                                            timeoutMs: try sourceUInt(options, "timeoutMs", 5_000)))
                entry.tasks[handle.id] = task
                result(sourcePreloadWire(task.snapshot))
            case "sourcePreloadSnapshot", "awaitSourcePreload", "cancelSourcePreload":
                guard let taskId = value["taskId"] as? String, let task = entry.tasks.values.first(where: { $0.id == taskId }) else {
                    if call.method == "cancelSourcePreload" { result(nil); return }
                    throw PluginError.operationFailed("Unknown preload task.")
                }
                if call.method == "cancelSourcePreload" { task.cancel(); result(nil) }
                else if call.method == "sourcePreloadSnapshot" { result(sourcePreloadWire(task.snapshot)) }
                else {
                    guard sourceWaiters < 128 else { throw PluginError.operationFailed("Source waiter capacity exceeded.") }
                    sourceWaiters += 1
                    Task { @MainActor in
                        defer { self.sourceWaiters -= 1 }
                        result(sourcePreloadWire(await task.result))
                    }
                }
            case "activateSource":
                guard sourceWaiters < 128, let playerId = value["playerId"] as? String, let player = sessions[playerId] else {
                    throw PluginError.operationFailed("Player unavailable or source waiter capacity exceeded.")
                }
                let handle = try sourceHandle(value)
                let options = try sourceActivationOptions(value["options"])
                sourceWaiters += 1
                Task { @MainActor in
                    defer { self.sourceWaiters -= 1 }
                    do { result(try await player.controller.activate(handle, options: options).wire) }
                    catch { result(sourceLifecycleError(error)) }
                }
            default: throw PluginError.operationFailed("Unknown source command.")
            }
        } catch { result(sourceLifecycleError(error)) }
    }
}

func sourceLifecycleError(_ error: Error) -> FlutterError {
    let code = (error as? VesperSourceActivationError)?.rawValue ?? (error as? VesperSourceSessionError)?.rawValue ?? "source_operation_failed"
    return FlutterError(code: code, message: code, details: nil)
}

func sourceActivationOptions(_ value: Any?) throws -> VesperSourceActivationOptions {
    let map = try sourceOptionsMap(value)
    let position = try sourceUInt(map, "startPositionMs", 0)
    guard position <= UInt64(Int64.max) else { throw PluginError.operationFailed("Invalid start position.") }
    let playWhenReady: Bool
    if let raw = map["playWhenReady"] {
        guard let number = raw as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else {
            throw PluginError.operationFailed("Invalid play intent.")
        }
        playWhenReady = number.boolValue
    } else { playWhenReady = true }
    let playbackRate: Float
    if let raw = map["playbackRate"] {
        guard let number = raw as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.floatValue.isFinite else { throw PluginError.operationFailed("Invalid playback rate.") }
        playbackRate = number.floatValue
    } else { playbackRate = 1 }
    return .init(playWhenReady: playWhenReady, startPositionMs: Int64(position),
                 playbackRate: playbackRate,
                 timeoutMs: try sourceUInt(map, "timeoutMs", 30_000))
}

func sourceInt(_ value: [String: Any], _ key: String, _ fallback: Int) throws -> Int {
    guard let raw = value[key] else { return fallback }
    guard let number = raw as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
          ["c", "s", "i", "l", "q", "C", "S", "I", "L", "Q"].contains(String(cString: number.objCType)),
          number.compare(NSNumber(value: 0)) != .orderedAscending,
          number.compare(NSNumber(value: Int.max)) != .orderedDescending else {
        throw PluginError.operationFailed("Invalid source limit.")
    }
    return number.intValue
}
private func sourceOptionsMap(_ value: Any?) throws -> [String: Any] {
    guard let value else { return [:] }
    guard let map = value as? [String: Any] else { throw PluginError.operationFailed("Invalid source options.") }
    return map
}
func sourceUInt(_ value: [String: Any], _ key: String, _ fallback: UInt64) throws -> UInt64 {
    guard value[key] != nil else { return fallback }
    return UInt64(try sourceInt(value, key, 0))
}
private func sourcePreloadWire(_ value: VesperPreloadResult) -> [String: Any] {
    var result: [String: Any] = ["taskId": value.taskId, "sessionId": value.sessionId, "sourceId": value.handleId,
                               "status": value.status.rawValue, "goal": value.goal.rawValue, "reuse": value.capability.rawValue,
                               "actualBytes": value.actualBytes]
    if let hit = value.cacheHit { result["cacheHit"] = hit }
    if let reason = value.reasonCode { result["reasonCode"] = reason }
    return result
}
