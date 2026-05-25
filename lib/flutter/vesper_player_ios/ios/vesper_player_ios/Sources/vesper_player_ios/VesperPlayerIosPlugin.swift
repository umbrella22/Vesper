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
    @MainActor var eventSink: FlutterEventSink?
    @MainActor var downloadEventSink: FlutterEventSink?
    @MainActor var sessions: [String: PlayerSession] = [:]
    @MainActor var downloadSessions: [String: DownloadSession] = [:]

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
                emitDownloadRuntimeEvents(for: session)
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
        case "setKeepScreenOnDuringPlayback":
            handleSessionCommand(call, result: result) { session in
                let arguments = arguments(of: call)
                guard let enabled = arguments["enabled"] as? Bool else {
                    throw PluginError.missingArgument("enabled")
                }
                session.lastError = nil
                session.controller.setKeepScreenOnDuringPlayback(enabled)
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
        case "shareDownloadTask":
            handleDownloadShareTask(call, result: result)
        case "saveDownloadTask":
            handleDownloadSaveTask(call, result: result)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    @MainActor
    func bindSessionHost(playerId: String, host: PlayerSurfaceView) {
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
    func unbindSessionHost(playerId: String, host: PlayerSurfaceView) {
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
            let sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration
            if let sourceNormalizerMap = try nestedMap(arguments["sourceNormalizer"]) {
                sourceNormalizerConfiguration =
                    sourceNormalizerMap.toSourceNormalizerConfiguration()
            } else {
                sourceNormalizerConfiguration = VesperSourceNormalizerConfiguration()
            }
            let frameProcessorConfiguration: VesperFrameProcessorConfiguration
            if let frameProcessorMap = try nestedMap(arguments["frameProcessor"]) {
                frameProcessorConfiguration = frameProcessorMap.toFrameProcessorConfiguration()
            } else {
                frameProcessorConfiguration = VesperFrameProcessorConfiguration()
            }
            let keepScreenOnDuringPlayback =
                (arguments["keepScreenOnDuringPlayback"] as? Bool) ?? true

            let session = PlayerSession(
                id: UUID().uuidString,
                controller: VesperPlayerControllerFactory.makeDefault(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy,
                    keepScreenOnDuringPlayback: keepScreenOnDuringPlayback,
                    benchmarkConfiguration: benchmarkConfiguration,
                    sourceNormalizerConfiguration: sourceNormalizerConfiguration,
                    frameProcessorConfiguration: frameProcessorConfiguration
                ),
                benchmarkConsoleLogging: benchmarkConfiguration.consoleLogging
            )
            sessions[session.id] = session
            observeSession(session)

            result([
                "playerId": session.id,
                "snapshot": buildSnapshotMap(for: session),
                "pluginDiagnostics": session.controller.pluginDiagnostics,
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
            let downloadId = UUID().uuidString
            let hasStaleResourceRecovery = arguments["hasStaleResourceRecovery"] as? Bool ?? false
            let configuration = configurationMap.toDownloadConfiguration()
            let recoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler?
            if hasStaleResourceRecovery {
                recoveryHandler = { [weak self] task, staleResource in
                    await self?.recoverDownloadTaskPlan(
                        downloadId: downloadId,
                        task: task,
                        staleResource: staleResource
                    )
                }
            } else {
                recoveryHandler = nil
            }
            let manager = VesperDownloadManager(
                configuration: configuration,
                staleResourcePlanRecoveryHandler: recoveryHandler
            )
            let session = DownloadSession(
                id: downloadId,
                manager: manager
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
    private func recoverDownloadTaskPlan(
        downloadId: String,
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource
    ) async -> VesperDownloadRecoveredTaskPlan? {
        guard let methodChannel else {
            return nil
        }
        let payload: [String: Any] = [
            "downloadId": downloadId,
            "task": task.toMap,
            "staleResource": staleResource.toMap,
        ]
        return await withCheckedContinuation { continuation in
            methodChannel.invokeMethod("recoverDownloadTaskPlan", arguments: payload) { value in
                guard let map = value as? [String: Any] else {
                    continuation.resume(returning: nil)
                    return
                }
                continuation.resume(returning: try? map.toDownloadRecoveredTaskPlan())
            }
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
    private func handleDownloadShareTask(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        do {
            let (session, taskId, arguments) = try resolveDownloadOutputRequest(call)
            guard let presenter = topViewController() else {
                throw PluginError.operationFailed("No view controller is available for sharing.")
            }
            try session.manager.shareTaskOutput(
                taskId: taskId,
                fileName: arguments["fileName"] as? String,
                mimeType: arguments["mimeType"] as? String,
                from: presenter
            )
            result(nil)
        } catch {
            result(asDownloadFlutterError(error, code: "vesper_download_operation_failed"))
        }
    }

    @MainActor
    private func handleDownloadSaveTask(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        do {
            let (session, taskId, arguments) = try resolveDownloadOutputRequest(call)
            guard let presenter = topViewController() else {
                throw PluginError.operationFailed("No view controller is available for saving.")
            }
            _ = try session.manager.saveTaskOutput(
                taskId: taskId,
                fileName: arguments["fileName"] as? String,
                from: presenter
            )
            result(nil)
        } catch {
            result(asDownloadFlutterError(error, code: "vesper_download_operation_failed"))
        }
    }

    @MainActor
    private func resolveDownloadOutputRequest(
        _ call: FlutterMethodCall
    ) throws -> (DownloadSession, VesperDownloadTaskId, [String: Any]) {
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
        return (session, taskId, arguments)
    }

    @MainActor
    private func topViewController() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let window = scenes
            .flatMap(\.windows)
            .first(where: { $0.isKeyWindow })
        var controller = window?.rootViewController
        while let presented = controller?.presentedViewController {
            controller = presented
        }
        return controller
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
                self.emitDownloadRuntimeEvents(for: session)
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
    func emitDownloadSnapshot(for session: DownloadSession) {
        downloadEventSink?([
            "downloadId": session.id,
            "type": "initialSnapshot",
            "snapshot": buildDownloadSnapshotMap(for: session),
        ])
    }

    @MainActor
    func emitDownloadRuntimeEvents(for session: DownloadSession) {
        for event in session.manager.drainEvents() {
            switch event {
            case let .created(task):
                downloadEventSink?([
                    "downloadId": session.id,
                    "type": "taskCreated",
                    "task": task.toMap,
                ])
            case let .assetIndexUpdated(task):
                downloadEventSink?([
                    "downloadId": session.id,
                    "type": "taskUpdated",
                    "task": task.toMap,
                ])
            case let .stateChanged(patch):
                if patch.state == .removed {
                    downloadEventSink?([
                        "downloadId": session.id,
                        "type": "taskRemoved",
                        "taskId": NSNumber(value: patch.taskId),
                    ])
                } else {
                    downloadEventSink?([
                        "downloadId": session.id,
                        "type": "taskUpdated",
                        "patch": patch.toMap,
                    ])
                }
            case let .progressUpdated(patch):
                downloadEventSink?([
                    "downloadId": session.id,
                    "type": "taskUpdated",
                    "progressPatch": patch.toMap,
                ])
            }
        }
    }

    @MainActor
    private func emitDownloadError(for session: DownloadSession, error: Error) {
        downloadEventSink?([
            "downloadId": session.id,
            "type": "downloadError",
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
