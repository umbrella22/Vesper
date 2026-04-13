import AVFoundation
import Combine
import Flutter
import UIKit
import VesperPlayerKit

public final class VesperPlayerIosPlugin: NSObject, FlutterPlugin, FlutterStreamHandler {
    private var methodChannel: FlutterMethodChannel?
    private var eventChannel: FlutterEventChannel?
    @MainActor private var eventSink: FlutterEventSink?
    @MainActor private var sessions: [String: PlayerSession] = [:]

    public static func register(with registrar: FlutterPluginRegistrar) {
        let instance = VesperPlayerIosPlugin()
        let methodChannel = FlutterMethodChannel(
            name: methodChannelName,
            binaryMessenger: registrar.messenger()
        )
        let eventChannel = FlutterEventChannel(
            name: eventChannelName,
            binaryMessenger: registrar.messenger()
        )

        instance.methodChannel = methodChannel
        instance.eventChannel = eventChannel

        methodChannel.setMethodCallHandler { [weak instance] call, result in
            guard let instance else {
                result(FlutterMethodNotImplemented)
                return
            }
            instance.handle(call, result: result)
        }
        eventChannel.setStreamHandler(instance)
        registrar.register(PlayerViewFactory(plugin: instance), withId: playerViewType)
    }

    public func onListen(
        withArguments arguments: Any?,
        eventSink events: @escaping FlutterEventSink
    ) -> FlutterError? {
        Task { @MainActor in
            eventSink = events
            sessions.values.forEach { emitSnapshot(for: $0) }
        }
        return nil
    }

    public func onCancel(withArguments arguments: Any?) -> FlutterError? {
        Task { @MainActor in
            eventSink = nil
        }
        return nil
    }

    public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        Task { @MainActor in
            handleOnMain(call, result: result)
        }
    }

    @MainActor
    private func handleOnMain(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        switch call.method {
        case "createPlayer":
            handleCreatePlayer(call, result: result)
        case "disposePlayer":
            handleSessionCommand(call, result: result) { session in
                disposeSession(session)
                return nil
            }
        case "initialize":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.initialize()
                emitSnapshot(for: session)
                return nil
            }
        case "selectSource":
            handleSessionCommand(call, result: result) { session in
                let sourceMap = try requireNestedMap(arguments: arguments(of: call), key: "source")
                session.lastError = nil
                session.controller.selectSource(try sourceMap.toVesperPlayerSource())
                emitSnapshot(for: session)
                return nil
            }
        case "play":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.play()
                emitSnapshot(for: session)
                return nil
            }
        case "pause":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.pause()
                emitSnapshot(for: session)
                return nil
            }
        case "togglePause":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.togglePause()
                emitSnapshot(for: session)
                return nil
            }
        case "stop":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.stop()
                emitSnapshot(for: session)
                return nil
            }
        case "seekBy":
            handleSessionCommand(call, result: result) { session in
                let arguments = arguments(of: call)
                guard let deltaMs = (arguments["deltaMs"] as? NSNumber)?.int64Value else {
                    throw PluginError.missingArgument("deltaMs")
                }
                session.lastError = nil
                session.controller.seek(by: deltaMs)
                emitSnapshot(for: session)
                return nil
            }
        case "seekToRatio":
            handleSessionCommand(call, result: result) { session in
                let arguments = arguments(of: call)
                guard let ratio = (arguments["ratio"] as? NSNumber)?.doubleValue else {
                    throw PluginError.missingArgument("ratio")
                }
                session.lastError = nil
                session.controller.seek(toRatio: ratio)
                emitSnapshot(for: session)
                return nil
            }
        case "seekToLiveEdge":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.seekToLiveEdge()
                emitSnapshot(for: session)
                return nil
            }
        case "setPlaybackRate":
            handleSessionCommand(call, result: result) { session in
                let arguments = arguments(of: call)
                guard let rate = (arguments["rate"] as? NSNumber)?.floatValue else {
                    throw PluginError.missingArgument("rate")
                }
                session.lastError = nil
                session.controller.setPlaybackRate(rate)
                emitSnapshot(for: session)
                return nil
            }
        case "setVideoTrackSelection":
            handleSessionCommand(call, result: result) { session in
                let selectionMap = try requireNestedMap(arguments: arguments(of: call), key: "selection")
                session.lastError = nil
                session.controller.setVideoTrackSelection(try selectionMap.toTrackSelection())
                emitSnapshot(for: session)
                return nil
            }
        case "setAudioTrackSelection":
            handleSessionCommand(call, result: result) { session in
                let selectionMap = try requireNestedMap(arguments: arguments(of: call), key: "selection")
                session.lastError = nil
                session.controller.setAudioTrackSelection(try selectionMap.toTrackSelection())
                emitSnapshot(for: session)
                return nil
            }
        case "setSubtitleTrackSelection":
            handleSessionCommand(call, result: result) { session in
                let selectionMap = try requireNestedMap(arguments: arguments(of: call), key: "selection")
                session.lastError = nil
                session.controller.setSubtitleTrackSelection(try selectionMap.toTrackSelection())
                emitSnapshot(for: session)
                return nil
            }
        case "setAbrPolicy":
            handleSessionCommand(call, result: result) { session in
                let policyMap = try requireNestedMap(arguments: arguments(of: call), key: "policy")
                session.lastError = nil
                session.controller.setAbrPolicy(try policyMap.toAbrPolicy())
                emitSnapshot(for: session)
                return nil
            }
        case "setResiliencePolicy":
            handleSessionCommand(call, result: result) { session in
                let policyMap = try requireNestedMap(arguments: arguments(of: call), key: "policy")
                session.lastError = nil
                session.controller.setResiliencePolicy(try policyMap.toResiliencePolicy())
                emitSnapshot(for: session)
                return nil
            }
        case "updateViewport":
            handleSessionCommand(call, result: result) { session in
                emitSnapshot(for: session)
                return nil
            }
        case "clearViewport":
            handleSessionCommand(call, result: result) { session in
                emitSnapshot(for: session)
                return nil
            }
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    @MainActor
    fileprivate func bindSessionHost(playerId: String, host: PlayerSurfaceView) {
        guard let session = sessions[playerId] else { return }
        if session.hostView !== host {
            session.controller.detachSurfaceHost()
            session.hostView = host
        }
        session.controller.attachSurfaceHost(host)
        emitSnapshot(for: session)
    }

    @MainActor
    fileprivate func unbindSessionHost(playerId: String, host: PlayerSurfaceView) {
        guard let session = sessions[playerId], session.hostView === host else { return }
        session.controller.detachSurfaceHost()
        session.hostView = nil
        emitSnapshot(for: session)
    }

    @MainActor
    private func handleCreatePlayer(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        do {
            let arguments = arguments(of: call)
            let initialSource: VesperPlayerSource?
            if let initialSourceMap = try nestedMap(arguments["initialSource"]) {
                initialSource = try initialSourceMap.toVesperPlayerSource()
            } else {
                initialSource = nil
            }
            let resiliencePolicy: VesperPlaybackResiliencePolicy
            if let resiliencePolicyMap = try nestedMap(arguments["resiliencePolicy"]) {
                resiliencePolicy = try resiliencePolicyMap.toResiliencePolicy()
            } else {
                resiliencePolicy = VesperPlaybackResiliencePolicy()
            }

            let session = PlayerSession(
                id: UUID().uuidString,
                controller: VesperPlayerControllerFactory.makeDefault(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy
                )
            )
            sessions[session.id] = session
            observeSession(session)

            result([
                "playerId": session.id,
                "snapshot": buildSnapshotMap(for: session),
            ])
        } catch {
            result(asFlutterError(error, code: "vesper_create_failed"))
        }
    }

    @MainActor
    private func handleSessionCommand(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult,
        action: (PlayerSession) throws -> Any?
    ) {
        do {
            let arguments = arguments(of: call)
            guard let playerId = arguments["playerId"] as? String, !playerId.isEmpty else {
                throw PluginError.missingArgument("playerId")
            }
            guard let session = sessions[playerId] else {
                throw PluginError.unknownPlayer(playerId)
            }

            let value = try action(session)
            result(value)
        } catch {
            if
                let playerId = arguments(of: call)["playerId"] as? String,
                let session = sessions[playerId]
            {
                session.lastError = errorMap(from: error)
                emitError(for: session, error: error)
            }
            result(asFlutterError(error, code: "vesper_operation_failed"))
        }
    }

    @MainActor
    private func observeSession(_ session: PlayerSession) {
        session.observation = session.controller.objectWillChange.sink { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.emitSnapshot(for: session)
            }
        }
    }

    @MainActor
    private func emitSnapshot(for session: PlayerSession) {
        emitEvent([
            "playerId": session.id,
            "type": "snapshot",
            "snapshot": buildSnapshotMap(for: session),
        ])
    }

    @MainActor
    private func emitError(for session: PlayerSession, error: Error) {
        emitEvent([
            "playerId": session.id,
            "type": "error",
            "error": session.lastError ?? errorMap(from: error),
            "snapshot": buildSnapshotMap(for: session),
        ])
    }

    @MainActor
    private func emitEvent(_ payload: [String: Any]) {
        eventSink?(payload)
    }

    @MainActor
    private func buildSnapshotMap(for session: PlayerSession) -> [String: Any] {
        let uiState = session.controller.uiState
        let trackCatalog = session.controller.trackCatalog
        let trackSelection = session.controller.trackSelection

        return [
            "title": uiState.title,
            "subtitle": uiState.subtitle,
            "sourceLabel": uiState.sourceLabel,
            "playbackState": uiState.playbackState.toWireName(),
            "playbackRate": Double(uiState.playbackRate),
            "isBuffering": uiState.isBuffering,
            "isInterrupted": uiState.isInterrupted,
            "hasVideoSurface": session.hostView != nil,
            "timeline": uiState.timeline.toMap(),
            "backendFamily": session.controller.backend.toBackendFamilyWireName(),
            "capabilities": buildCapabilitiesMap(),
            "trackCatalog": trackCatalog.toMap(),
            "trackSelection": trackSelection.toMap(),
            "lastError": flutterValue(session.lastError),
        ]
    }

    @MainActor
    private func buildCapabilitiesMap() -> [String: Any] {
        [
            "supportsLocalFiles": true,
            "supportsRemoteUrls": true,
            "supportsHls": true,
            "supportsDash": false,
            "supportsTrackCatalog": true,
            "supportsTrackSelection": true,
            "supportsAbrPolicy": true,
            "supportsResiliencePolicy": true,
            "supportsHolePunch": false,
            "supportsPlaybackRate": true,
            "supportsLiveEdgeSeeking": true,
            "isExperimental": true,
            "supportedPlaybackRates": VesperPlayerController.supportedPlaybackRates.map(Double.init),
        ]
    }

    @MainActor
    private func disposeSession(_ session: PlayerSession) {
        session.observation?.cancel()
        session.controller.detachSurfaceHost()
        session.hostView = nil
        session.controller.dispose()
        sessions.removeValue(forKey: session.id)
        emitEvent([
            "playerId": session.id,
            "type": "disposed",
        ])
    }
}

private final class PlayerSession {
    let id: String
    let controller: VesperPlayerController
    var hostView: PlayerSurfaceView?
    var observation: AnyCancellable?
    var lastError: [String: Any]?

    init(id: String, controller: VesperPlayerController) {
        self.id = id
        self.controller = controller
    }
}

private final class PlayerViewFactory: NSObject, FlutterPlatformViewFactory {
    private weak var plugin: VesperPlayerIosPlugin?

    init(plugin: VesperPlayerIosPlugin) {
        self.plugin = plugin
    }

    func createArgsCodec() -> FlutterMessageCodec & NSObjectProtocol {
        FlutterStandardMessageCodec.sharedInstance()
    }

    func create(
        withFrame frame: CGRect,
        viewIdentifier viewId: Int64,
        arguments args: Any?
    ) -> FlutterPlatformView {
        let arguments = args as? [String: Any] ?? [:]
        let playerId = arguments["playerId"] as? String
        let hostView = PlayerSurfaceView(frame: frame)

        if let playerId {
            Task { @MainActor [weak plugin, weak hostView] in
                guard let plugin, let hostView else { return }
                plugin.bindSessionHost(playerId: playerId, host: hostView)
            }
        }

        return PlayerPlatformView(hostView: hostView) { [weak plugin, weak hostView] in
            guard let playerId else { return }
            Task { @MainActor in
                guard let plugin, let hostView else { return }
                plugin.unbindSessionHost(playerId: playerId, host: hostView)
            }
        }
    }
}

private final class PlayerPlatformView: NSObject, FlutterPlatformView {
    private let hostView: PlayerSurfaceView
    private let onDispose: () -> Void

    init(hostView: PlayerSurfaceView, onDispose: @escaping () -> Void) {
        self.hostView = hostView
        self.onDispose = onDispose
    }

    func view() -> UIView {
        hostView
    }

    func dispose() {
        onDispose()
    }
}

private enum PluginError: LocalizedError {
    case missingArgument(String)
    case invalidNestedMap(String)
    case invalidSource(String)
    case invalidTrackSelection(String)
    case invalidAbrPolicy(String)
    case unsupported(String)
    case unknownPlayer(String)

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
        case let .unknownPlayer(playerId):
            "Unknown playerId: \(playerId)"
        }
    }
}

private func arguments(of call: FlutterMethodCall) -> [String: Any] {
    stringKeyedMap(call.arguments) ?? [:]
}

private func nestedMap(_ value: Any?) throws -> [String: Any]? {
    guard let value else { return nil }
    if value is NSNull {
        return nil
    }
    if let map = stringKeyedMap(value) {
        return map
    }
    throw PluginError.invalidNestedMap("value")
}

private func requireNestedMap(arguments: [String: Any], key: String) throws -> [String: Any] {
    guard let raw = arguments[key] else {
        throw PluginError.missingArgument(key)
    }
    guard let map = stringKeyedMap(raw) else {
        throw PluginError.invalidNestedMap(key)
    }
    return map
}

private func stringKeyedMap(_ value: Any?) -> [String: Any]? {
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

private extension Dictionary where Key == String, Value == Any {
    func toVesperPlayerSource() throws -> VesperPlayerSource {
        guard let uri = self["uri"] as? String, !uri.isEmpty else {
            throw PluginError.invalidSource("Missing source uri.")
        }
        let label = self["label"] as? String ?? uri
        let kind = (self["kind"] as? String) == "remote"
            ? VesperPlayerSourceKind.remote
            : VesperPlayerSourceKind.local
        let `protocol`: VesperPlayerSourceProtocol
        switch self["protocol"] as? String {
        case "file":
            `protocol` = .file
        case "content":
            `protocol` = .content
        case "progressive":
            `protocol` = .progressive
        case "hls":
            `protocol` = .hls
        case "dash":
            `protocol` = .dash
        default:
            `protocol` = .unknown
        }
        return try VesperPlayerSource(
            uri: uri,
            label: label,
            kind: kind,
            protocol: `protocol`
        )
        .validatedForIosBackend()
    }

    func toTrackSelection() throws -> VesperTrackSelection {
        switch self["mode"] as? String {
        case "disabled":
            return .disabled()
        case "track":
            guard let trackId = self["trackId"] as? String, !trackId.isEmpty else {
                throw PluginError.invalidTrackSelection("Missing trackId for track selection.")
            }
            return .track(trackId)
        default:
            return .auto()
        }
    }

    func toAbrPolicy() throws -> VesperAbrPolicy {
        switch self["mode"] as? String {
        case "constrained":
            return .constrained(
                maxBitRate: (self["maxBitRate"] as? NSNumber)?.int64Value,
                maxWidth: (self["maxWidth"] as? NSNumber)?.intValue,
                maxHeight: (self["maxHeight"] as? NSNumber)?.intValue
            )
        case "fixedTrack":
            guard let trackId = self["trackId"] as? String, !trackId.isEmpty else {
                throw PluginError.invalidAbrPolicy("Missing trackId for fixed track policy.")
            }
            return .fixedTrack(trackId)
        default:
            return .auto()
        }
    }

    func toResiliencePolicy() throws -> VesperPlaybackResiliencePolicy {
        let buffering = try (nestedMap(self["buffering"])?.toBufferingPolicy()) ?? VesperBufferingPolicy()
        let retry = try (nestedMap(self["retry"])?.toRetryPolicy()) ?? VesperRetryPolicy()
        let cache = try (nestedMap(self["cache"])?.toCachePolicy()) ?? VesperCachePolicy()
        return VesperPlaybackResiliencePolicy(
            buffering: buffering,
            retry: retry,
            cache: cache
        )
    }

    func toBufferingPolicy() -> VesperBufferingPolicy {
        let preset: VesperBufferingPreset
        switch self["preset"] as? String {
        case "balanced":
            preset = .balanced
        case "streaming":
            preset = .streaming
        case "resilient":
            preset = .resilient
        case "lowLatency":
            preset = .lowLatency
        default:
            preset = .default
        }
        return VesperBufferingPolicy(
            preset: preset,
            minBufferMs: (self["minBufferMs"] as? NSNumber)?.int64Value,
            maxBufferMs: (self["maxBufferMs"] as? NSNumber)?.int64Value,
            bufferForPlaybackMs: (self["bufferForPlaybackMs"] as? NSNumber)?.int64Value,
            bufferForPlaybackAfterRebufferMs:
                (self["bufferForPlaybackAfterRebufferMs"] as? NSNumber)?.int64Value
        )
    }

    func toRetryPolicy() -> VesperRetryPolicy {
        let backoff: VesperRetryBackoff?
        switch self["backoff"] as? String {
        case "fixed":
            backoff = .fixed
        case "linear":
            backoff = .linear
        case "exponential":
            backoff = .exponential
        default:
            backoff = nil
        }
        return VesperRetryPolicy(
            maxAttempts: (self["maxAttempts"] as? NSNumber)?.intValue,
            baseDelayMs: (self["baseDelayMs"] as? NSNumber)?.uint64Value,
            maxDelayMs: (self["maxDelayMs"] as? NSNumber)?.uint64Value,
            backoff: backoff
        )
    }

    func toCachePolicy() -> VesperCachePolicy {
        let preset: VesperCachePreset
        switch self["preset"] as? String {
        case "disabled":
            preset = .disabled
        case "streaming":
            preset = .streaming
        case "resilient":
            preset = .resilient
        default:
            preset = .default
        }
        return VesperCachePolicy(
            preset: preset,
            maxMemoryBytes: (self["maxMemoryBytes"] as? NSNumber)?.int64Value,
            maxDiskBytes: (self["maxDiskBytes"] as? NSNumber)?.int64Value
        )
    }
}

private extension TimelineUiState {
    func toMap() -> [String: Any] {
        [
            "kind": kind.toWireName(),
            "isSeekable": isSeekable,
            "seekableRange": flutterValue(seekableRange.map {
                [
                    "startMs": $0.startMs,
                    "endMs": $0.endMs,
                ]
            }),
            "liveEdgeMs": flutterValue(liveEdgeMs),
            "positionMs": positionMs,
            "durationMs": flutterValue(durationMs),
        ]
    }
}

private extension VesperTrackCatalog {
    func toMap() -> [String: Any] {
        [
            "tracks": tracks.map(\.toMap),
            "adaptiveVideo": adaptiveVideo,
            "adaptiveAudio": adaptiveAudio,
        ]
    }
}

private extension VesperMediaTrack {
    var toMap: [String: Any] {
        [
            "id": id,
            "kind": kind.toWireName(),
            "label": flutterValue(label),
            "language": flutterValue(language),
            "codec": flutterValue(codec),
            "bitRate": flutterValue(bitRate),
            "width": flutterValue(width),
            "height": flutterValue(height),
            "frameRate": flutterValue(frameRate),
            "channels": flutterValue(channels),
            "sampleRate": flutterValue(sampleRate),
            "isDefault": isDefault,
            "isForced": isForced,
        ]
    }
}

private extension VesperTrackSelectionSnapshot {
    func toMap() -> [String: Any] {
        [
            "video": video.toMap(),
            "audio": audio.toMap(),
            "subtitle": subtitle.toMap(),
            "abrPolicy": abrPolicy.toMap(),
        ]
    }
}

private extension VesperTrackSelection {
    func toMap() -> [String: Any] {
        [
            "mode": mode.toWireName(),
            "trackId": flutterValue(trackId),
        ]
    }
}

private extension VesperAbrPolicy {
    func toMap() -> [String: Any] {
        [
            "mode": mode.toWireName(),
            "trackId": flutterValue(trackId),
            "maxBitRate": flutterValue(maxBitRate),
            "maxWidth": flutterValue(maxWidth),
            "maxHeight": flutterValue(maxHeight),
        ]
    }
}

private func flutterValue(_ value: Any?) -> Any {
    value ?? NSNull()
}

private func errorMap(from error: Error) -> [String: Any] {
    let category: String
    if let pluginError = error as? PluginError {
        switch pluginError {
        case .invalidSource:
            category = "source"
        case .invalidTrackSelection, .invalidAbrPolicy:
            category = "capability"
        case .unsupported:
            category = "unsupported"
        default:
            category = "platform"
        }
    } else {
        category = "platform"
    }
    return [
        "message": error.localizedDescription,
        "category": category,
        "retriable": false,
    ]
}

private func asFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: errorMap(from: error)
    )
}

private extension PlaybackStateUi {
    func toWireName() -> String {
        switch self {
        case .ready:
            "ready"
        case .playing:
            "playing"
        case .paused:
            "paused"
        case .finished:
            "finished"
        }
    }
}

private extension TimelineKindUi {
    func toWireName() -> String {
        switch self {
        case .vod:
            "vod"
        case .live:
            "live"
        case .liveDvr:
            "liveDvr"
        }
    }
}

private extension PlayerBridgeBackend {
    func toBackendFamilyWireName() -> String {
        switch self {
        case .fakeDemo:
            "fakeDemo"
        case .rustNativeStub:
            "iosHostKit"
        }
    }
}

private extension VesperPlayerSource {
    func validatedForIosBackend() throws -> VesperPlayerSource {
        if kind == .remote, `protocol` == .dash {
            throw PluginError.unsupported(iosDashUnsupportedMessage)
        }
        return self
    }
}

private extension VesperMediaTrackKind {
    func toWireName() -> String {
        switch self {
        case .video:
            "video"
        case .audio:
            "audio"
        case .subtitle:
            "subtitle"
        }
    }
}

private extension VesperTrackSelectionMode {
    func toWireName() -> String {
        switch self {
        case .auto:
            "auto"
        case .disabled:
            "disabled"
        case .track:
            "track"
        }
    }
}

private extension VesperAbrMode {
    func toWireName() -> String {
        switch self {
        case .auto:
            "auto"
        case .constrained:
            "constrained"
        case .fixedTrack:
            "fixedTrack"
        }
    }
}

private let methodChannelName = "io.github.ikaros.vesper_player"
private let eventChannelName = "io.github.ikaros.vesper_player/events"
private let playerViewType = "io.github.ikaros.vesper_player/platform_view"
private let iosDashUnsupportedMessage = "DASH streams are not supported by the iOS AVPlayer backend."
