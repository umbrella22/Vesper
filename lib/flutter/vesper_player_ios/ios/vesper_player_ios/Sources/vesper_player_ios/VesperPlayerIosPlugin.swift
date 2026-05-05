import AVFoundation
import AVKit
import Combine
import Flutter
import UIKit
import VesperPlayerKit

public final class VesperPlayerIosPlugin: NSObject, FlutterPlugin, FlutterStreamHandler {
    private static let hostDetachGraceDelayNanoseconds: UInt64 = 250_000_000

    private var methodChannel: FlutterMethodChannel?
    private var eventChannel: FlutterEventChannel?
    private var downloadEventChannel: FlutterEventChannel?
    @MainActor fileprivate var eventSink: FlutterEventSink?
    @MainActor fileprivate var downloadEventSink: FlutterEventSink?
    @MainActor fileprivate var sessions: [String: PlayerSession] = [:]
    @MainActor fileprivate var downloadSessions: [String: DownloadSession] = [:]

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
        let downloadEventChannel = FlutterEventChannel(
            name: downloadEventChannelName,
            binaryMessenger: registrar.messenger()
        )

        instance.methodChannel = methodChannel
        instance.eventChannel = eventChannel
        instance.downloadEventChannel = downloadEventChannel

        methodChannel.setMethodCallHandler { [weak instance] call, result in
            guard let instance else {
                result(FlutterMethodNotImplemented)
                return
            }
            instance.handle(call, result: result)
        }
        eventChannel.setStreamHandler(instance)
        downloadEventChannel.setStreamHandler(DownloadEventStreamHandler(plugin: instance))
        registrar.register(PlayerViewFactory(plugin: instance), withId: playerViewType)
        registrar.register(AirPlayRouteButtonFactory(plugin: instance), withId: airPlayRouteButtonViewType)
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
        case "createDownloadManager":
            handleCreateDownloadManager(call, result: result)
        case "disposePlayer":
            handleSessionCommand(call, result: result) { session in
                disposeSession(session)
                return nil
            }
        case "refreshPlayer":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.refresh()
                emitSnapshot(for: session)
                return nil
            }
        case "refreshDownloadManager":
            handleDownloadSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.manager.refresh()
                emitDownloadSnapshot(for: session)
                return nil
            }
        case "disposeDownloadManager":
            handleDownloadSessionCommand(call, result: result) { session in
                disposeDownloadSession(session)
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
                let viewportMap = try requireNestedMap(arguments: arguments(of: call), key: "viewport")
                session.lastError = nil
                session.viewport = viewportMap.toFlutterViewport()
                session.viewportHint =
                    (try nestedMap(arguments(of: call)["viewportHint"]))?.toFlutterViewportHint()
                    ?? .hidden
                emitSnapshot(for: session)
                return nil
            }
        case "clearViewport":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.viewport = nil
                session.viewportHint = .hidden
                emitSnapshot(for: session)
                return nil
            }
        case "configureSystemPlayback":
            handleSessionCommand(call, result: result) { session in
                let configurationMap = try requireNestedMap(
                    arguments: arguments(of: call),
                    key: "configuration"
                )
                session.lastError = nil
                session.controller.configureSystemPlayback(
                    configurationMap.toSystemPlaybackConfiguration()
                )
                emitSnapshot(for: session)
                return nil
            }
        case "updateSystemPlaybackMetadata":
            handleSessionCommand(call, result: result) { session in
                let metadataMap = try requireNestedMap(arguments: arguments(of: call), key: "metadata")
                session.lastError = nil
                session.controller.updateSystemPlaybackMetadata(
                    metadataMap.toSystemPlaybackMetadata()
                )
                emitSnapshot(for: session)
                return nil
            }
        case "clearSystemPlayback":
            handleSessionCommand(call, result: result) { session in
                session.lastError = nil
                session.controller.clearSystemPlayback()
                emitSnapshot(for: session)
                return nil
            }
        case "requestSystemPlaybackPermissions":
            result(VesperPlayerController.requestSystemPlaybackPermissions().toWireName())
        case "getSystemPlaybackPermissionStatus":
            result(VesperPlayerController.getSystemPlaybackPermissionStatus().toWireName())
        case "createDownloadTask":
            handleDownloadSessionCommand(call, result: result) { session in
                let arguments = arguments(of: call)
                guard let assetId = arguments["assetId"] as? String, !assetId.isEmpty else {
                    throw PluginError.missingArgument("assetId")
                }
                let sourceMap = try requireNestedMap(arguments: arguments, key: "source")
                let profileMap = try requireNestedMap(arguments: arguments, key: "profile")
                let assetIndexMap = try requireNestedMap(arguments: arguments, key: "assetIndex")
                session.lastError = nil
                return session.manager.createTask(
                    assetId: assetId,
                    source: try sourceMap.toDownloadSource(),
                    profile: profileMap.toDownloadProfile(),
                    assetIndex: assetIndexMap.toDownloadAssetIndex()
                )
            }
        case "startDownloadTask":
            handleDownloadTaskAction(call, result: result) { session, taskId in
                session.manager.startTask(taskId)
            }
        case "pauseDownloadTask":
            handleDownloadTaskAction(call, result: result) { session, taskId in
                session.manager.pauseTask(taskId)
            }
        case "resumeDownloadTask":
            handleDownloadTaskAction(call, result: result) { session, taskId in
                session.manager.resumeTask(taskId)
            }
        case "removeDownloadTask":
            handleDownloadTaskAction(call, result: result) { session, taskId in
                session.manager.removeTask(taskId)
            }
        case "exportDownloadTask":
            handleDownloadExportTask(call, result: result)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    @MainActor
    fileprivate func bindSessionHost(playerId: String, host: PlayerSurfaceView) {
        guard let session = sessions[playerId] else { return }
        session.cancelPendingHostDetach()
        _ = session.advanceHostDetachGeneration()
        if session.hostView === host {
            session.controller.attachSurfaceHost(host)
            emitSnapshot(for: session)
            return
        }

        let previousHost = session.hostView
        session.hostView = host
        session.controller.attachSurfaceHost(host)
        previousHost?.detachBridgeIfNeeded()
        emitSnapshot(for: session)
    }

    @MainActor
    fileprivate func unbindSessionHost(playerId: String, host: PlayerSurfaceView) {
        guard let session = sessions[playerId], session.hostView === host else { return }
        session.cancelPendingHostDetach()
        let generation = session.advanceHostDetachGeneration()
        session.pendingHostDetachTask = Task { @MainActor [weak self, weak session, weak host] in
            do {
                try await Task.sleep(nanoseconds: Self.hostDetachGraceDelayNanoseconds)
            } catch {
                return
            }
            guard
                !Task.isCancelled,
                let self,
                let session,
                let host,
                self.sessions[playerId] === session,
                session.hostView === host,
                session.hostDetachGeneration == generation
            else {
                return
            }
            session.controller.detachSurfaceHost()
            session.hostView = nil
            session.pendingHostDetachTask = nil
            self.emitSnapshot(for: session)
        }
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
            let trackPreferencePolicy: VesperTrackPreferencePolicy
            if let trackPreferencePolicyMap = try nestedMap(arguments["trackPreferencePolicy"]) {
                trackPreferencePolicy = try trackPreferencePolicyMap.toTrackPreferencePolicy()
            } else {
                trackPreferencePolicy = VesperTrackPreferencePolicy()
            }
            let preloadBudgetPolicy: VesperPreloadBudgetPolicy
            if let preloadBudgetPolicyMap = try nestedMap(arguments["preloadBudgetPolicy"]) {
                preloadBudgetPolicy = preloadBudgetPolicyMap.toPreloadBudgetPolicy()
            } else {
                preloadBudgetPolicy = VesperPreloadBudgetPolicy()
            }
            let benchmarkConfiguration: VesperBenchmarkConfiguration
            if let benchmarkConfigurationMap = try nestedMap(arguments["benchmarkConfiguration"]) {
                benchmarkConfiguration = benchmarkConfigurationMap.toBenchmarkConfiguration()
            } else {
                benchmarkConfiguration = .disabled
            }

            let session = PlayerSession(
                id: UUID().uuidString,
                controller: VesperPlayerControllerFactory.makeDefault(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy,
                    benchmarkConfiguration: benchmarkConfiguration
                ),
                benchmarkConsoleLogging: benchmarkConfiguration.consoleLogging
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
    private func handleCreateDownloadManager(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        do {
            let arguments = arguments(of: call)
            let configurationMap = try requireNestedMap(arguments: arguments, key: "configuration")
            let session = DownloadSession(
                id: UUID().uuidString,
                manager: VesperDownloadManager(
                    configuration: configurationMap.toDownloadConfiguration()
                )
            )
            downloadSessions[session.id] = session
            observeDownloadSession(session)

            result([
                "downloadId": session.id,
                "snapshot": buildDownloadSnapshotMap(for: session),
            ])
        } catch {
            result(asDownloadFlutterError(error, code: "vesper_download_create_failed"))
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
    private func handleDownloadSessionCommand(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult,
        action: (DownloadSession) throws -> Any?
    ) {
        do {
            let arguments = arguments(of: call)
            guard let downloadId = arguments["downloadId"] as? String, !downloadId.isEmpty else {
                throw PluginError.missingArgument("downloadId")
            }
            guard let session = downloadSessions[downloadId] else {
                throw PluginError.unknownDownload(downloadId)
            }

            let value = try action(session)
            result(value)
        } catch {
            if
                let downloadId = arguments(of: call)["downloadId"] as? String,
                let session = downloadSessions[downloadId]
            {
                session.lastError = downloadErrorMap(from: error)
                emitDownloadError(for: session, error: error)
            }
            result(asDownloadFlutterError(error, code: "vesper_download_operation_failed"))
        }
    }

    @MainActor
    private func handleDownloadTaskAction(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult,
        action: (DownloadSession, VesperDownloadTaskId) throws -> Bool
    ) {
        handleDownloadSessionCommand(call, result: result) { session in
            let arguments = arguments(of: call)
            guard let taskId = (arguments["taskId"] as? NSNumber)?.uint64Value else {
                throw PluginError.missingArgument("taskId")
            }
            session.lastError = nil
            return try action(session, taskId)
        }
    }

    @MainActor
    private func handleDownloadExportTask(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        do {
            let arguments = arguments(of: call)
            guard let downloadId = arguments["downloadId"] as? String, !downloadId.isEmpty else {
                throw PluginError.missingArgument("downloadId")
            }
            guard let session = downloadSessions[downloadId] else {
                throw PluginError.unknownDownload(downloadId)
            }
            guard let taskId = (arguments["taskId"] as? NSNumber)?.uint64Value else {
                throw PluginError.missingArgument("taskId")
            }
            guard let outputPath = arguments["outputPath"] as? String, !outputPath.isEmpty else {
                throw PluginError.missingArgument("outputPath")
            }

            session.lastError = nil
            Task { @MainActor [weak self] in
                guard let self else { return }
                do {
                    try await session.manager.exportTaskOutput(
                        taskId: taskId,
                        outputPath: outputPath,
                        onProgress: { [weak self] ratio in
                            Task { @MainActor [weak self] in
                                self?.emitDownloadExportProgress(
                                    for: session,
                                    taskId: taskId,
                                    ratio: ratio
                                )
                            }
                        }
                    )
                    result(nil)
                } catch {
                    session.lastError = downloadErrorMap(from: error)
                    emitDownloadError(for: session, error: error)
                    result(asDownloadFlutterError(error, code: "vesper_download_operation_failed"))
                }
            }
        } catch {
            result(asDownloadFlutterError(error, code: "vesper_download_operation_failed"))
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
    private func observeDownloadSession(_ session: DownloadSession) {
        session.observation = session.manager.objectWillChange.sink { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.emitDownloadSnapshot(for: session)
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
        emitBenchmarkConsoleLog(for: session)
    }

    @MainActor
    private func emitError(for session: PlayerSession, error: Error) {
        emitEvent([
            "playerId": session.id,
            "type": "error",
            "error": resolvedPlayerErrorMap(for: session) ?? errorMap(from: error),
            "snapshot": buildSnapshotMap(for: session),
        ])
        emitBenchmarkConsoleLog(for: session, force: true)
    }

    @MainActor
    fileprivate func emitDownloadSnapshot(for session: DownloadSession) {
        downloadEventSink?([
            "downloadId": session.id,
            "type": "snapshot",
            "snapshot": buildDownloadSnapshotMap(for: session),
        ])
    }

    @MainActor
    private func emitDownloadError(for session: DownloadSession, error: Error) {
        downloadEventSink?([
            "downloadId": session.id,
            "type": "error",
            "error": session.lastError ?? downloadErrorMap(from: error),
            "snapshot": buildDownloadSnapshotMap(for: session),
        ])
    }

    @MainActor
    private func emitDownloadExportProgress(
        for session: DownloadSession,
        taskId: VesperDownloadTaskId,
        ratio: Float
    ) {
        downloadEventSink?([
            "downloadId": session.id,
            "type": "exportProgress",
            "taskId": NSNumber(value: taskId),
            "ratio": Double(max(0, min(1, ratio))),
        ])
    }

    @MainActor
    private func emitEvent(_ payload: [String: Any]) {
        eventSink?(payload)
    }

    @MainActor
    private func emitBenchmarkConsoleLog(for session: PlayerSession, force: Bool = false) {
        guard session.benchmarkConsoleLogging else {
            return
        }

        let events = session.controller.drainBenchmarkEvents()
        let summary = session.controller.benchmarkSummary()
        guard !events.isEmpty || summary.acceptedEvents > 0 else {
            return
        }
        guard force || !events.isEmpty else {
            return
        }

        let payload = BenchmarkConsolePayload(
            playerId: session.id,
            events: events,
            summary: summary
        )
        do {
            let data = try JSONEncoder().encode(payload)
            if let json = String(data: data, encoding: .utf8) {
                print("[VesperBenchmark] \(json)")
            }
        } catch {
            print("[VesperBenchmark] {\"error\":\"\(error.localizedDescription)\"}")
        }
    }

    @MainActor
    private func buildSnapshotMap(for session: PlayerSession) -> [String: Any] {
        let uiState = session.controller.uiState
        let trackCatalog = session.controller.trackCatalog
        let trackSelection = session.controller.trackSelection
        let resiliencePolicy = session.controller.resiliencePolicy
        let effectiveVideoTrackId = session.controller.effectiveVideoTrackId
        let videoVariantObservation = session.controller.videoVariantObservation
        let fixedTrackStatus = session.controller.fixedTrackStatus
        let lastError = resolvedPlayerErrorMap(for: session)

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
            "viewport": flutterValue(session.viewport?.toMap()),
            "viewportHint": session.viewportHint.toMap(),
            "backendFamily": session.controller.backend.toBackendFamilyWireName(),
            "capabilities": buildCapabilitiesMap(),
            "trackCatalog": trackCatalog.toMap(),
            "trackSelection": trackSelection.toMap(),
            "effectiveVideoTrackId": flutterValue(effectiveVideoTrackId),
            "videoVariantObservation": flutterValue(
                videoVariantObservation.map { observation in
                    [
                        "bitRate": observation.bitRate as Any,
                        "width": observation.width as Any,
                        "height": observation.height as Any,
                    ]
                }
            ),
            "fixedTrackStatus": flutterValue(fixedTrackStatus?.toWireName()),
            "resiliencePolicy": resiliencePolicy.toMap(),
            "lastError": flutterValue(lastError),
        ]
    }

    @MainActor
    private func resolvedPlayerErrorMap(for session: PlayerSession) -> [String: Any]? {
        session.controller.lastError?.toMap ?? session.lastError
    }

    @MainActor
    private func buildCapabilitiesMap() -> [String: Any] {
        let supportsBestEffortFixedTrackAbr: Bool
        if #available(iOS 15.0, *) {
            supportsBestEffortFixedTrackAbr = true
        } else {
            supportsBestEffortFixedTrackAbr = false
        }
        return [
            "supportsLocalFiles": true,
            "supportsRemoteUrls": true,
            "supportsHls": true,
            "supportsDash": true,
            "supportsDashStaticVod": true,
            "supportsDashDynamicLive": true,
            "supportsDashManifestTrackCatalog": true,
            "supportsDashTextTracks": true,
            "supportsTrackCatalog": true,
            "supportsTrackSelection": true,
            "supportsVideoTrackSelection": false,
            "supportsAudioTrackSelection": true,
            "supportsSubtitleTrackSelection": true,
            "supportsAbrPolicy": true,
            "supportsAbrConstrained": true,
            "supportsAbrFixedTrack": supportsBestEffortFixedTrackAbr,
            "supportsExactAbrFixedTrack": false,
            "supportsAbrMaxBitRate": true,
            "supportsAbrMaxResolution": true,
            "supportsResiliencePolicy": true,
            "supportsHolePunch": false,
            "supportsPlaybackRate": true,
            "supportsLiveEdgeSeeking": true,
            "isExperimental": true,
            "supportedPlaybackRates": VesperPlayerController.supportedPlaybackRates.map(Double.init),
        ]
    }

    @MainActor
    private func buildDownloadSnapshotMap(for session: DownloadSession) -> [String: Any] {
        [
            "tasks": session.manager.snapshot.tasks.map(\.toMap),
        ]
    }

    @MainActor
    private func disposeSession(_ session: PlayerSession) {
        session.cancelPendingHostDetach()
        _ = session.advanceHostDetachGeneration()
        session.observation?.cancel()
        session.controller.detachSurfaceHost()
        session.hostView = nil
        session.controller.dispose()
        emitBenchmarkConsoleLog(for: session, force: true)
        sessions.removeValue(forKey: session.id)
        emitEvent([
            "playerId": session.id,
            "type": "disposed",
        ])
    }

    @MainActor
    private func disposeDownloadSession(_ session: DownloadSession) {
        session.observation?.cancel()
        session.manager.dispose()
        downloadSessions.removeValue(forKey: session.id)
        downloadEventSink?([
            "downloadId": session.id,
            "type": "disposed",
        ])
    }
}

private final class PlayerSession {
    let id: String
    let controller: VesperPlayerController
    let benchmarkConsoleLogging: Bool
    var hostView: PlayerSurfaceView?
    var pendingHostDetachTask: Task<Void, Never>?
    var hostDetachGeneration: UInt64 = 0
    var observation: AnyCancellable?
    var lastError: [String: Any]?
    var viewport: FlutterViewport?
    var viewportHint: FlutterViewportHint = .hidden

    init(
        id: String,
        controller: VesperPlayerController,
        benchmarkConsoleLogging: Bool = false
    ) {
        self.id = id
        self.controller = controller
        self.benchmarkConsoleLogging = benchmarkConsoleLogging
    }

    func cancelPendingHostDetach() {
        pendingHostDetachTask?.cancel()
        pendingHostDetachTask = nil
    }

    @discardableResult
    func advanceHostDetachGeneration() -> UInt64 {
        hostDetachGeneration &+= 1
        return hostDetachGeneration
    }
}

private struct BenchmarkConsolePayload: Encodable {
    let playerId: String
    let events: [VesperBenchmarkEvent]
    let summary: VesperBenchmarkSummary
}

private final class DownloadSession {
    let id: String
    let manager: VesperDownloadManager
    var observation: AnyCancellable?
    var lastError: [String: Any]?

    init(id: String, manager: VesperDownloadManager) {
        self.id = id
        self.manager = manager
    }
}

private struct FlutterViewport {
    let left: Double
    let top: Double
    let width: Double
    let height: Double

    func toMap() -> [String: Any] {
        [
            "left": left,
            "top": top,
            "width": width,
            "height": height,
        ]
    }
}

private struct FlutterViewportHint {
    let kind: String
    let visibleFraction: Double

    static let hidden = FlutterViewportHint(kind: "hidden", visibleFraction: 0)

    func toMap() -> [String: Any] {
        [
            "kind": kind,
            "visibleFraction": visibleFraction,
        ]
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
        hostView.isUserInteractionEnabled = false

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

private final class AirPlayRouteButtonFactory: NSObject, FlutterPlatformViewFactory {
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
        let view = AVRoutePickerView(frame: frame)
        view.backgroundColor = .clear
        view.prioritizesVideoDevices = arguments["prioritizesVideoDevices"] as? Bool ?? true
        if let tintColor = (arguments["tintColor"] as? NSNumber)?.uint32Value {
            view.tintColor = UIColor(argb: tintColor)
        }
        if let activeTintColor = (arguments["activeTintColor"] as? NSNumber)?.uint32Value {
            view.activeTintColor = UIColor(argb: activeTintColor)
        }

        let routeView = AirPlayRoutePlatformView(routePickerView: view)
        if let playerId = arguments["playerId"] as? String {
            Task { @MainActor [weak plugin, weak routeView] in
                guard let plugin, let routeView, let session = plugin.sessions[playerId] else { return }
                routeView.bind(controller: session.controller)
            }
        }
        return routeView
    }
}

private final class AirPlayRoutePlatformView: NSObject, FlutterPlatformView {
    private let routePickerView: AVRoutePickerView

    init(routePickerView: AVRoutePickerView) {
        self.routePickerView = routePickerView
    }

    @MainActor
    func bind(controller: VesperPlayerController) {
        _ = controller.routePickerPlayer
    }

    func view() -> UIView {
        routePickerView
    }

    func dispose() {}
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

private final class DownloadEventStreamHandler: NSObject, FlutterStreamHandler {
    private weak var plugin: VesperPlayerIosPlugin?

    init(plugin: VesperPlayerIosPlugin) {
        self.plugin = plugin
    }

    func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        Task { @MainActor [weak plugin] in
            guard let plugin else { return }
            plugin.downloadEventSink = events
            plugin.downloadSessions.values.forEach { plugin.emitDownloadSnapshot(for: $0) }
        }
        return nil
    }

    func onCancel(withArguments arguments: Any?) -> FlutterError? {
        Task { @MainActor [weak plugin] in
            plugin?.downloadEventSink = nil
        }
        return nil
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
        case let .unknownPlayer(playerId):
            "Unknown playerId: \(playerId)"
        case let .unknownDownload(downloadId):
            "Unknown downloadId: \(downloadId)"
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

private func stringMap(_ value: Any?) -> [String: String] {
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
        let headers = stringMap(self["headers"])
        return try VesperPlayerSource(
            uri: uri,
            label: label,
            kind: kind,
            protocol: `protocol`,
            headers: headers
        )
        .validatedForIosBackend()
    }

    func toRawVesperPlayerSource() throws -> VesperPlayerSource {
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
        let headers = stringMap(self["headers"])
        return try VesperPlayerSource(
            uri: uri,
            label: label,
            kind: kind,
            protocol: `protocol`,
            headers: headers
        )
    }

    func toSystemPlaybackConfiguration() -> VesperSystemPlaybackConfiguration {
        let backgroundMode: VesperBackgroundPlaybackMode =
            (self["backgroundMode"] as? String) == "disabled" ? .disabled : .continueAudio
        return VesperSystemPlaybackConfiguration(
            enabled: self["enabled"] as? Bool ?? true,
            backgroundMode: backgroundMode,
            showSystemControls: self["showSystemControls"] as? Bool ?? true,
            showSeekActions: self["showSeekActions"] as? Bool ?? true,
            metadata: (try? nestedMap(self["metadata"]))?.toSystemPlaybackMetadata()
        )
    }

    func toSystemPlaybackMetadata() -> VesperSystemPlaybackMetadata {
        VesperSystemPlaybackMetadata(
            title: self["title"] as? String ?? "",
            artist: self["artist"] as? String,
            albumTitle: self["albumTitle"] as? String,
            artworkUri: self["artworkUri"] as? String,
            contentUri: self["contentUri"] as? String,
            durationMs: (self["durationMs"] as? NSNumber)?.int64Value,
            isLive: self["isLive"] as? Bool ?? false
        )
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

    func toTrackPreferencePolicy() throws -> VesperTrackPreferencePolicy {
        let audioSelection = try (nestedMap(self["audioSelection"])?.toTrackSelection()) ?? .auto()
        let subtitleSelection =
            try (nestedMap(self["subtitleSelection"])?.toTrackSelection()) ?? .disabled()
        let abrPolicy = try (nestedMap(self["abrPolicy"])?.toAbrPolicy()) ?? .auto()
        return VesperTrackPreferencePolicy(
            preferredAudioLanguage: self["preferredAudioLanguage"] as? String,
            preferredSubtitleLanguage: self["preferredSubtitleLanguage"] as? String,
            selectSubtitlesByDefault: self["selectSubtitlesByDefault"] as? Bool ?? false,
            selectUndeterminedSubtitleLanguage:
                self["selectUndeterminedSubtitleLanguage"] as? Bool ?? false,
            audioSelection: audioSelection,
            subtitleSelection: subtitleSelection,
            abrPolicy: abrPolicy
        )
    }

    func toPreloadBudgetPolicy() -> VesperPreloadBudgetPolicy {
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks: (self["maxConcurrentTasks"] as? NSNumber)?.intValue,
            maxMemoryBytes: (self["maxMemoryBytes"] as? NSNumber)?.int64Value,
            maxDiskBytes: (self["maxDiskBytes"] as? NSNumber)?.int64Value,
            warmupWindowMs: (self["warmupWindowMs"] as? NSNumber)?.int64Value
        )
    }

    func toBenchmarkConfiguration() -> VesperBenchmarkConfiguration {
        VesperBenchmarkConfiguration(
            enabled: self["enabled"] as? Bool ?? false,
            maxBufferedEvents: (self["maxBufferedEvents"] as? NSNumber)?.intValue ?? 2_048,
            includeRawEvents: self["includeRawEvents"] as? Bool ?? true,
            consoleLogging: self["consoleLogging"] as? Bool ?? false,
            pluginLibraryPaths:
                (self["pluginLibraryPaths"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? []
        )
    }

    func toDownloadConfiguration() -> VesperDownloadConfiguration {
        VesperDownloadConfiguration(
            autoStart: self["autoStart"] as? Bool ?? true,
            runPostProcessorsOnCompletion:
                self["runPostProcessorsOnCompletion"] as? Bool ?? true,
            baseDirectory: (self["baseDirectory"] as? String).map {
                URL(fileURLWithPath: $0, isDirectory: true)
            },
            pluginLibraryPaths:
                (self["pluginLibraryPaths"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? []
        )
    }

    func toDownloadSource() throws -> VesperDownloadSource {
        let contentFormat: VesperDownloadContentFormat
        switch self["contentFormat"] as? String {
        case "hlsSegments":
            contentFormat = .hlsSegments
        case "dashSegments":
            contentFormat = .dashSegments
        case "singleFile":
            contentFormat = .singleFile
        default:
            contentFormat = .unknown
        }
        return VesperDownloadSource(
            source: try requireNestedMap(arguments: self, key: "source").toRawVesperPlayerSource(),
            contentFormat: contentFormat,
            manifestUri: self["manifestUri"] as? String
        )
    }

    func toDownloadProfile() -> VesperDownloadProfile {
        VesperDownloadProfile(
            variantId: self["variantId"] as? String,
            preferredAudioLanguage: self["preferredAudioLanguage"] as? String,
            preferredSubtitleLanguage: self["preferredSubtitleLanguage"] as? String,
            selectedTrackIds:
                (self["selectedTrackIds"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? [],
            targetDirectory: (self["targetDirectory"] as? String).map {
                URL(fileURLWithPath: $0, isDirectory: true)
            },
            allowMeteredNetwork: self["allowMeteredNetwork"] as? Bool ?? false
        )
    }

    func toDownloadAssetIndex() -> VesperDownloadAssetIndex {
        let contentFormat: VesperDownloadContentFormat
        switch self["contentFormat"] as? String {
        case "hlsSegments":
            contentFormat = .hlsSegments
        case "dashSegments":
            contentFormat = .dashSegments
        case "singleFile":
            contentFormat = .singleFile
        default:
            contentFormat = .unknown
        }
        return VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: self["version"] as? String,
            etag: self["etag"] as? String,
            checksum: self["checksum"] as? String,
            totalSizeBytes: (self["totalSizeBytes"] as? NSNumber)?.uint64Value,
            resources:
                (self["resources"] as? [Any])?.compactMap { value in
                    stringKeyedMap(value)?.toDownloadResourceRecord()
                } ?? [],
            segments:
                (self["segments"] as? [Any])?.compactMap { value in
                    stringKeyedMap(value)?.toDownloadSegmentRecord()
                } ?? [],
            completedPath: self["completedPath"] as? String
        )
    }

    func toDownloadResourceRecord() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: self["resourceId"] as? String ?? "",
            uri: self["uri"] as? String ?? "",
            relativePath: self["relativePath"] as? String,
            sizeBytes: (self["sizeBytes"] as? NSNumber)?.uint64Value,
            etag: self["etag"] as? String,
            checksum: self["checksum"] as? String
        )
    }

    func toDownloadSegmentRecord() -> VesperDownloadSegmentRecord {
        VesperDownloadSegmentRecord(
            segmentId: self["segmentId"] as? String ?? "",
            uri: self["uri"] as? String ?? "",
            relativePath: self["relativePath"] as? String,
            sequence: (self["sequence"] as? NSNumber)?.uint64Value,
            sizeBytes: (self["sizeBytes"] as? NSNumber)?.uint64Value,
            checksum: self["checksum"] as? String
        )
    }

    func toFlutterViewport() -> FlutterViewport {
        FlutterViewport(
            left: (self["left"] as? NSNumber)?.doubleValue ?? 0,
            top: (self["top"] as? NSNumber)?.doubleValue ?? 0,
            width: (self["width"] as? NSNumber)?.doubleValue ?? 0,
            height: (self["height"] as? NSNumber)?.doubleValue ?? 0
        )
    }

    func toFlutterViewportHint() -> FlutterViewportHint {
        let kind: String
        switch self["kind"] as? String {
        case "visible":
            kind = "visible"
        case "nearVisible":
            kind = "nearVisible"
        case "prefetchOnly":
            kind = "prefetchOnly"
        default:
            kind = "hidden"
        }

        let visibleFraction = Swift.max(
            0,
            Swift.min((self["visibleFraction"] as? NSNumber)?.doubleValue ?? 0, 1)
        )
        return FlutterViewportHint(kind: kind, visibleFraction: visibleFraction)
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
        let maxAttempts: Int?
        if keys.contains("maxAttempts") {
            if self["maxAttempts"] is NSNull {
                maxAttempts = nil
            } else {
                maxAttempts = (self["maxAttempts"] as? NSNumber)?.intValue
            }
        } else {
            maxAttempts = 3
        }
        return VesperRetryPolicy(
            maxAttempts: maxAttempts,
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

private extension VesperPlaybackResiliencePolicy {
    func toMap() -> [String: Any] {
        [
            "buffering": buffering.toMap(),
            "retry": retry.toMap(),
            "cache": cache.toMap(),
        ]
    }
}

private extension VesperBufferingPolicy {
    func toMap() -> [String: Any] {
        [
            "preset": preset.toWireName(),
            "minBufferMs": flutterValue(minBufferMs),
            "maxBufferMs": flutterValue(maxBufferMs),
            "bufferForPlaybackMs": flutterValue(bufferForPlaybackMs),
            "bufferForPlaybackAfterRebufferMs": flutterValue(bufferForPlaybackAfterRebufferMs),
        ]
    }
}

private extension VesperRetryPolicy {
    func toMap() -> [String: Any] {
        [
            "maxAttempts": flutterValue(maxAttempts),
            "baseDelayMs": baseDelayMs,
            "maxDelayMs": maxDelayMs,
            "backoff": backoff.toWireName(),
        ]
    }
}

private extension VesperCachePolicy {
    func toMap() -> [String: Any] {
        [
            "preset": preset.toWireName(),
            "maxMemoryBytes": flutterValue(maxMemoryBytes),
            "maxDiskBytes": flutterValue(maxDiskBytes),
        ]
    }
}

private extension VesperPlayerSource {
    func toMap() -> [String: Any] {
        [
            "uri": uri,
            "label": label,
            "kind": kind.rawValue,
            "protocol": `protocol`.rawValue,
            "headers": headers,
        ]
    }
}

private extension VesperDownloadTaskSnapshot {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "assetId": assetId,
            "source": source.toMap,
            "profile": profile.toMap,
            "state": state.toWireName(),
            "progress": progress.toMap,
            "assetIndex": assetIndex.toMap,
            "error": flutterValue(error?.toMap),
        ]
    }
}

private extension VesperDownloadSource {
    var toMap: [String: Any] {
        [
            "source": source.toMap(),
            "contentFormat": contentFormat.toWireName(),
            "manifestUri": flutterValue(manifestUri),
        ]
    }
}

private extension VesperDownloadProfile {
    var toMap: [String: Any] {
        [
            "variantId": flutterValue(variantId),
            "preferredAudioLanguage": flutterValue(preferredAudioLanguage),
            "preferredSubtitleLanguage": flutterValue(preferredSubtitleLanguage),
            "selectedTrackIds": selectedTrackIds,
            "targetDirectory": flutterValue(targetDirectory?.path),
            "allowMeteredNetwork": allowMeteredNetwork,
        ]
    }
}

private extension VesperDownloadProgressSnapshot {
    var toMap: [String: Any] {
        [
            "receivedBytes": receivedBytes,
            "totalBytes": flutterValue(totalBytes),
            "receivedSegments": receivedSegments,
            "totalSegments": flutterValue(totalSegments),
        ]
    }
}

private extension VesperDownloadAssetIndex {
    var toMap: [String: Any] {
        [
            "contentFormat": contentFormat.toWireName(),
            "version": flutterValue(version),
            "etag": flutterValue(etag),
            "checksum": flutterValue(checksum),
            "totalSizeBytes": flutterValue(totalSizeBytes),
            "resources": resources.map(\.toMap),
            "segments": segments.map(\.toMap),
            "completedPath": flutterValue(completedPath),
        ]
    }
}

private extension VesperDownloadResourceRecord {
    var toMap: [String: Any] {
        [
            "resourceId": resourceId,
            "uri": uri,
            "relativePath": flutterValue(relativePath),
            "sizeBytes": flutterValue(sizeBytes),
            "etag": flutterValue(etag),
            "checksum": flutterValue(checksum),
        ]
    }
}

private extension VesperDownloadSegmentRecord {
    var toMap: [String: Any] {
        [
            "segmentId": segmentId,
            "uri": uri,
            "relativePath": flutterValue(relativePath),
            "sequence": flutterValue(sequence),
            "sizeBytes": flutterValue(sizeBytes),
            "checksum": flutterValue(checksum),
        ]
    }
}

private extension VesperDownloadError {
    var toMap: [String: Any] {
        [
            "codeOrdinal": codeOrdinal,
            "categoryOrdinal": categoryOrdinal,
            "retriable": retriable,
            "message": message,
        ]
    }
}

private extension VesperPlayerError {
    var toMap: [String: Any] {
        [
            "message": message,
            "category": category.rawValue,
            "retriable": retriable,
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

private func downloadErrorMap(from error: Error) -> [String: Any] {
    [
        "codeOrdinal": 0,
        "categoryOrdinal": 0,
        "retriable": false,
        "message": error.localizedDescription,
    ]
}

private func asFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: errorMap(from: error)
    )
}

private func asDownloadFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: downloadErrorMap(from: error)
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

private extension VesperSystemPlaybackPermissionStatus {
    func toWireName() -> String {
        switch self {
        case .notRequired:
            "notRequired"
        case .granted:
            "granted"
        case .denied:
            "denied"
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

private extension VesperFixedTrackStatus {
    func toWireName() -> String {
        switch self {
        case .pending:
            "pending"
        case .locked:
            "locked"
        case .fallback:
            "fallback"
        }
    }
}

private extension VesperBufferingPreset {
    func toWireName() -> String {
        switch self {
        case .default:
            "defaultPreset"
        case .balanced:
            "balanced"
        case .streaming:
            "streaming"
        case .resilient:
            "resilient"
        case .lowLatency:
            "lowLatency"
        }
    }
}

private extension VesperRetryBackoff {
    func toWireName() -> String {
        switch self {
        case .fixed:
            "fixed"
        case .linear:
            "linear"
        case .exponential:
            "exponential"
        }
    }
}

private extension VesperCachePreset {
    func toWireName() -> String {
        switch self {
        case .default:
            "defaultPreset"
        case .disabled:
            "disabled"
        case .streaming:
            "streaming"
        case .resilient:
            "resilient"
        }
    }
}

private extension VesperDownloadState {
    func toWireName() -> String {
        switch self {
        case .queued:
            "queued"
        case .preparing:
            "preparing"
        case .downloading:
            "downloading"
        case .paused:
            "paused"
        case .completed:
            "completed"
        case .failed:
            "failed"
        case .removed:
            "removed"
        }
    }
}

private extension VesperDownloadContentFormat {
    func toWireName() -> String {
        switch self {
        case .hlsSegments:
            "hlsSegments"
        case .dashSegments:
            "dashSegments"
        case .singleFile:
            "singleFile"
        case .unknown:
            "unknown"
        }
    }
}

private extension UIColor {
    convenience init(argb: UInt32) {
        let alpha = CGFloat((argb >> 24) & 0xff) / 255.0
        let red = CGFloat((argb >> 16) & 0xff) / 255.0
        let green = CGFloat((argb >> 8) & 0xff) / 255.0
        let blue = CGFloat(argb & 0xff) / 255.0
        self.init(red: red, green: green, blue: blue, alpha: alpha)
    }
}

private let methodChannelName = "io.github.ikaros.vesper_player"
private let eventChannelName = "io.github.ikaros.vesper_player/events"
private let downloadEventChannelName = "io.github.ikaros.vesper_player/download_events"
private let playerViewType = "io.github.ikaros.vesper_player/platform_view"
private let airPlayRouteButtonViewType = "io.github.ikaros.vesper_player/airplay_route_button"
