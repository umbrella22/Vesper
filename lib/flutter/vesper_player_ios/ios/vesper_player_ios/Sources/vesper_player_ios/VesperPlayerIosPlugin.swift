import AVFoundation
import AVKit
import Combine
import Flutter
import UIKit
import VesperPlayerKit

private let runtimeHdrCapabilityDiagnosticKeys: [String] = [
    "assetVideoTrackCount",
    "assetVideoCodec",
    "assetVideoWidth",
    "assetVideoHeight",
    "assetVideoFrameRate",
    "assetVideoEstimatedDataRate",
    "avPlayerItemErrorLogEventCount",
    "avPlayerItemErrorStatusCode",
    "avPlayerItemErrorDomain",
    "avPlayerItemErrorComment",
]

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
        registrar.register(
            AirPlayRouteButtonFactory(plugin: instance), withId: airPlayRouteButtonViewType)
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

    public func detachFromEngine(for registrar: any FlutterPluginRegistrar) {
        Task { @MainActor [weak self] in
            guard let self else { return }
            for session in sessions.values {
                disposeSession(session)
            }
            for session in downloadSessions.values {
                disposeDownloadSession(session)
            }
            sessions.removeAll()
            downloadSessions.removeAll()
            eventSink = nil
            downloadEventSink = nil
            methodChannel?.setMethodCallHandler(nil)
            eventChannel?.setStreamHandler(nil)
            downloadEventChannel?.setStreamHandler(nil)
        }
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
        case "probePlaybackCapability":
            handleProbePlaybackCapability(call, result: result)
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
                let source = try sourceMap.toVesperPlayerSource()
                session.lastError = nil
                session.currentSourceFingerprint = VesperSourceFingerprint(source: source)
                session.recentHdrProbeEvidence = nil
                session.controller.selectSource(source)
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
                let selectionMap = try requireNestedMap(
                    arguments: arguments(of: call), key: "selection")
                session.lastError = nil
                session.controller.setVideoTrackSelection(try selectionMap.toTrackSelection())
                emitSnapshot(for: session)
                return nil
            }
        case "setAudioTrackSelection":
            handleSessionCommand(call, result: result) { session in
                let selectionMap = try requireNestedMap(
                    arguments: arguments(of: call), key: "selection")
                session.lastError = nil
                session.controller.setAudioTrackSelection(try selectionMap.toTrackSelection())
                emitSnapshot(for: session)
                return nil
            }
        case "setSubtitleTrackSelection":
            handleSessionCommand(call, result: result) { session in
                let selectionMap = try requireNestedMap(
                    arguments: arguments(of: call), key: "selection")
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
                let viewportMap = try requireNestedMap(
                    arguments: arguments(of: call), key: "viewport")
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
                let metadataMap = try requireNestedMap(
                    arguments: arguments(of: call), key: "metadata")
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
        case "isPictureInPictureAvailable":
            handleSessionCommand(call, result: result) { session in
                pictureInPictureAvailabilityMap(for: session)
            }
        case "setPictureInPictureConfiguration":
            handleSessionCommand(call, result: result) { session in
                let configuration =
                    (try nestedMap(arguments(of: call)["configuration"]))?
                    .toPictureInPictureConfiguration()
                    ?? FlutterPictureInPictureConfiguration()
                session.pictureInPictureConfiguration = configuration
                return nil
            }
        case "requestPictureInPicture":
            handlePictureInPictureCommand(call, result: result) { session in
                if let configuration =
                    (try nestedMap(arguments(of: call)["configuration"]))?
                    .toPictureInPictureConfiguration()
                {
                    session.pictureInPictureConfiguration = configuration
                }
                try requestPictureInPicture(for: session)
            }
        case "exitPictureInPicture":
            handlePictureInPictureCommand(call, result: result) { session in
                exitPictureInPicture(for: session)
            }
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
                let source = try sourceMap.toDownloadSource()
                if let drmConfiguration = source.source.drmConfiguration {
                    throw VesperPlayerDrmUnsupportedError(
                        route: "download",
                        keySystem: drmConfiguration.keySystem,
                        reason: "drmUnsupportedRoute"
                    )
                }
                return try session.manager.createTask(
                    assetId: assetId,
                    source: source,
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
            let nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration
            if let nativeFramePipelineMap = try nestedMap(arguments["nativeFramePipeline"]) {
                nativeFramePipelineConfiguration =
                    nativeFramePipelineMap.toNativeFramePipelineConfiguration()
            } else {
                nativeFramePipelineConfiguration = VesperNativeFramePipelineConfiguration()
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
                    frameProcessorConfiguration: frameProcessorConfiguration,
                    nativeFramePipelineConfiguration: nativeFramePipelineConfiguration
                ),
                benchmarkConsoleLogging: benchmarkConfiguration.consoleLogging
            )
            session.currentSourceFingerprint = initialSource.map(VesperSourceFingerprint.init(source:))
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
    private func handleProbePlaybackCapability(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult
    ) {
        let args = arguments(of: call)
        let request = VesperPlaybackCapabilityProbeRequest(
            source: (try? nestedMap(args["source"])?.toRawVesperPlayerSource()) ?? nil,
            codec: args["codec"] as? String,
            width: (args["width"] as? NSNumber)?.intValue,
            height: (args["height"] as? NSNumber)?.intValue,
            frameRate: (args["frameRate"] as? NSNumber)?.doubleValue,
            requiresNativeFrame: args["requiresNativeFrame"] as? Bool ?? false,
            sourceNormalizerConfiguration: (try? nestedMap(args["sourceNormalizer"])?
                .toSourceNormalizerConfiguration())
                ?? VesperSourceNormalizerConfiguration(),
            frameProcessorConfiguration: (try? nestedMap(args["frameProcessor"])?
                .toFrameProcessorConfiguration())
                ?? VesperFrameProcessorConfiguration(),
            nativeFramePipelineConfiguration: (try? nestedMap(args["nativeFramePipeline"])?
                .toNativeFramePipelineConfiguration())
                ?? VesperNativeFramePipelineConfiguration()
        )
        let probeResult = VesperPlayerControllerFactory.probePlaybackCapability(request)
        Task { @MainActor in
            let enrichedResult = await enrichPlaybackCapabilityProbeResult(
                probeResult,
                request: request
            )
            if let source = request.source {
                let sourceFingerprint = VesperSourceFingerprint(source: source)
                let evidence = VesperHdrProbeEvidence(source: source, result: enrichedResult)
                sessions.values
                    .filter { $0.currentSourceFingerprint == sourceFingerprint }
                    .forEach { $0.recentHdrProbeEvidence = evidence }
            }
            emitCapabilityWarningIfNeeded(
                playerId: args["playerId"] as? String,
                result: enrichedResult
            )
            result(flutterPlaybackCapabilityResultMap(enrichedResult))
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
            if let playerId = arguments(of: call)["playerId"] as? String,
                let session = sessions[playerId]
            {
                session.lastError = errorMap(from: error)
                emitError(for: session, error: error)
            }
            result(asFlutterError(error, code: "vesper_operation_failed"))
        }
    }

    @MainActor
    private func handlePictureInPictureCommand(
        _ call: FlutterMethodCall,
        result: @escaping FlutterResult,
        action: (PlayerSession) throws -> Void
    ) {
        do {
            let arguments = arguments(of: call)
            guard let playerId = arguments["playerId"] as? String, !playerId.isEmpty else {
                throw VesperIosPictureInPictureError(
                    code: "pictureInPictureUnavailableForCurrentRoute",
                    message: "Missing playerId."
                )
            }
            guard let session = sessions[playerId] else {
                throw VesperIosPictureInPictureError(
                    code: "pictureInPictureUnavailableForCurrentRoute",
                    message: "Unknown playerId: \(playerId)"
                )
            }

            try action(session)
            result(nil)
        } catch {
            result(asFlutterError(error, code: "vesper_picture_in_picture_failed"))
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
            if let downloadId = arguments(of: call)["downloadId"] as? String,
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
        let window =
            scenes
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
        let snapshot = buildSnapshotMap(for: session)
        emitHostTerminalErrorIfNeeded(for: session, snapshot: snapshot)
        emitEvent([
            "playerId": session.id,
            "type": "snapshot",
            "snapshot": snapshot,
        ])
        emitBenchmarkConsoleLog(for: session)
    }

    @MainActor
    private func emitHostTerminalErrorIfNeeded(
        for session: PlayerSession,
        snapshot: [String: Any]
    ) {
        guard let hostError = session.controller.lastError?.toMap else {
            session.lastEmittedTerminalError = nil
            return
        }
        session.lastError = hostError
        if let previous = session.lastEmittedTerminalError,
           NSDictionary(dictionary: previous).isEqual(to: hostError) {
            return
        }
        session.lastEmittedTerminalError = hostError
        emitEvent([
            "playerId": session.id,
            "type": "error",
            "error": hostError,
            "snapshot": snapshot,
        ])
        emitRuntimeHdrCapabilityWarningIfNeeded(for: session, errorMap: hostError)
        emitBenchmarkConsoleLog(for: session, force: true)
    }

    @MainActor
    private func emitError(for session: PlayerSession, error: Error) {
        let resolvedErrorMap = resolvedPlayerErrorMap(for: session) ?? errorMap(from: error)
        emitEvent([
            "playerId": session.id,
            "type": "error",
            "error": resolvedErrorMap,
            "snapshot": buildSnapshotMap(for: session),
        ])
        emitRuntimeHdrCapabilityWarningIfNeeded(for: session, errorMap: resolvedErrorMap)
        emitBenchmarkConsoleLog(for: session, force: true)
    }

    @MainActor
    private func pictureInPictureAvailabilityMap(for session: PlayerSession) -> [String: Any] {
        let error = pictureInPicturePreflightError(for: session)
        return [
            "isAvailable": error == nil,
            "isActive": session.pictureInPictureActive,
            "canAutoEnter": session.pictureInPictureConfiguration.enabled
                && session.pictureInPictureConfiguration.autoEnter,
            "source": "system",
            "error": flutterValue(error?.toMap()),
            "diagnostics": pictureInPictureDiagnostics(for: session),
        ]
    }

    @MainActor
    private func requestPictureInPicture(for session: PlayerSession) throws {
        if let error = pictureInPicturePreflightError(for: session) {
            failPictureInPicture(for: session, error: error)
            throw error
        }
        guard let layer = session.hostView?.pictureInPicturePlayerLayer else {
            let error = VesperIosPictureInPictureError(
                code: "pictureInPictureSurfaceUnavailable",
                message: "AVPlayerLayer is unavailable for Picture in Picture.",
                diagnostics: pictureInPictureDiagnostics(for: session)
            )
            failPictureInPicture(for: session, error: error)
            throw error
        }
        let coordinator =
            session.pictureInPictureCoordinator
            ?? VesperIosPictureInPictureCoordinator(plugin: self, session: session)
        session.pictureInPictureCoordinator = coordinator
        guard coordinator.configure(with: layer) else {
            let error = VesperIosPictureInPictureError(
                code: "pictureInPictureNotSupported",
                message: "AVPictureInPictureController is not supported.",
                diagnostics: pictureInPictureDiagnostics(for: session)
            )
            failPictureInPicture(for: session, error: error)
            throw error
        }
        session.pictureInPictureState = "entering"
        session.pictureInPictureActive = false
        emitPictureInPictureEvent(for: session)
        coordinator.start()
    }

    @MainActor
    private func exitPictureInPicture(for session: PlayerSession) {
        if session.pictureInPictureCoordinator?.isActive != true && !session.pictureInPictureActive {
            session.pictureInPictureState = "inactive"
            session.pictureInPictureActive = false
            emitPictureInPictureEvent(for: session)
            return
        }
        session.pictureInPictureState = "exiting"
        session.pictureInPictureActive = true
        emitPictureInPictureEvent(for: session)
        session.pictureInPictureCoordinator?.stop()
    }

    @MainActor
    private func pictureInPicturePreflightError(
        for session: PlayerSession
    ) -> VesperIosPictureInPictureError? {
        var diagnostics = pictureInPictureDiagnostics(for: session)
        guard session.pictureInPictureConfiguration.enabled else {
            return VesperIosPictureInPictureError(
                code: "pictureInPictureDisabledByHost",
                message: "Picture in Picture is disabled by host configuration.",
                diagnostics: diagnostics
            )
        }
        guard AVPictureInPictureController.isPictureInPictureSupported() else {
            return VesperIosPictureInPictureError(
                code: "pictureInPictureNotSupported",
                message: "AVPictureInPictureController is not supported.",
                diagnostics: diagnostics
            )
        }
        guard let hostView = session.hostView else {
            return VesperIosPictureInPictureError(
                code: "pictureInPictureSurfaceUnavailable",
                message: "No PlayerSurfaceView is attached for Picture in Picture.",
                diagnostics: diagnostics
            )
        }
        diagnostics["hasHostView"] = true
        if hostView.isNativeFramePresentationActive {
            return VesperIosPictureInPictureError(
                code: "pictureInPictureNativeFrameRouteCannotHandOff",
                message: "Native-frame route cannot hand off to AVPlayerLayer.",
                diagnostics: diagnostics
            )
        }
        guard hostView.pictureInPicturePlayerLayer != nil else {
            return VesperIosPictureInPictureError(
                code: "pictureInPictureSystemPlayerUnavailable",
                message: "AVPlayerLayer is not ready for Picture in Picture.",
                diagnostics: diagnostics
            )
        }
        return nil
    }

    @MainActor
    private func failPictureInPicture(
        for session: PlayerSession,
        error: VesperIosPictureInPictureError
    ) {
        session.pictureInPictureState = "failed"
        session.pictureInPictureActive = false
        emitPictureInPictureEvent(for: session, error: error)
    }

    @MainActor
    func emitPictureInPictureEvent(
        for session: PlayerSession,
        error: VesperIosPictureInPictureError? = nil
    ) {
        emitEvent([
            "playerId": session.id,
            "type": "pictureInPicture",
            "state": session.pictureInPictureState,
            "isActive": session.pictureInPictureActive,
            "source": "system",
            "canAutoEnter": session.pictureInPictureConfiguration.enabled
                && session.pictureInPictureConfiguration.autoEnter,
            "error": flutterValue(error?.toMap()),
            "diagnostics": pictureInPictureDiagnostics(for: session),
        ])
    }

    @MainActor
    private func pictureInPictureDiagnostics(for session: PlayerSession) -> [String: Any] {
        [
            "platform": "ios",
            "configurationEnabled": session.pictureInPictureConfiguration.enabled,
            "isSupported": AVPictureInPictureController.isPictureInPictureSupported(),
            "hasHostView": session.hostView != nil,
            "hasPlayerLayer": session.hostView?.pictureInPicturePlayerLayer != nil,
            "nativeFramePresentationActive":
                session.hostView?.isNativeFramePresentationActive == true,
        ]
    }

    @MainActor
    private func emitCapabilityWarningIfNeeded(
        playerId: String?,
        result: VesperPlaybackCapabilityProbeResult
    ) {
        guard result.recommendedPlaybackPath == .systemPlayer,
            result.hdrKind != .none,
            result.hdrKind != .unknown
        else {
            return
        }
        emitEvent([
            "playerId": playerId ?? "",
            "type": "warning",
            "warning": [
                "domain": "capability",
                "capability": [
                    "reason": "hdrNativeFrameUnsupported",
                    "recommendedPlaybackPath": "systemPlayer",
                    "hdrKind": result.hdrKind.rawValue,
                    "likelyHdrCapabilityIssue": true,
                    "confidence": result.confidence.rawValue,
                    "hdrMetadata": flutterHdrMetadataMap(from: result),
                    "message":
                        "HDR and Dolby Vision content uses system playback; SDK-managed native-frame presentation is SDR-only.",
                ],
            ],
        ])
    }

    private func enrichPlaybackCapabilityProbeResult(
        _ result: VesperPlaybackCapabilityProbeResult,
        request: VesperPlaybackCapabilityProbeRequest
    ) async -> VesperPlaybackCapabilityProbeResult {
        guard let assetProbeResult = await probeAssetPlaybackCapability(request) else {
            return result
        }
        return mergeAssetProbeResult(
            result,
            assetProbeResult: assetProbeResult
        )
    }

    private func probeAssetPlaybackCapability(
        _ request: VesperPlaybackCapabilityProbeRequest
    ) async -> IosFlutterAssetProbeResult? {
        guard let source = request.source,
            source.protocol == .file || source.protocol == .progressive || source.protocol == .hls
        else {
            return nil
        }
        guard let url = URL(string: source.uri) else {
            return IosFlutterAssetProbeResult(
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetProbeError": "invalidSourceUrl",
                ]
            )
        }

        let asset = AVURLAsset(url: url)
        var diagnostics: [String: String] = [
            "assetProbe": "iosAVAsset",
            "assetProbeAvailable": "true",
        ]

        do {
            let isPlayable = try await asset.load(.isPlayable)
            diagnostics["assetPlayable"] = String(isPlayable)

            let videoTracks = try await asset.loadTracks(withMediaType: .video)
            diagnostics["assetVideoTrackCount"] = String(videoTracks.count)
            if let firstVideoTrack = videoTracks.first {
                diagnostics.merge(await videoDiagnostics(for: firstVideoTrack)) { _, new in new }
            }

            return IosFlutterAssetProbeResult(
                isPlayable: isPlayable,
                metadataHdrKind: detectMetadataHdrKind(diagnostics),
                diagnostics: diagnostics
            )
        } catch {
            diagnostics["assetProbeError"] = String(describing: type(of: error))
            diagnostics["assetProbeErrorMessage"] = error.localizedDescription
            return IosFlutterAssetProbeResult(diagnostics: diagnostics)
        }
    }

    private func mergeAssetProbeResult(
        _ result: VesperPlaybackCapabilityProbeResult,
        assetProbeResult: IosFlutterAssetProbeResult
    ) -> VesperPlaybackCapabilityProbeResult {
        var missing = result.missingCapabilities
        var diagnostics = result.diagnostics
        diagnostics.merge(assetProbeResult.diagnostics) { _, new in new }
        let metadataHdrKind = assetProbeResult.metadataHdrKind
        let effectiveHdrKind =
            result.hdrKind == .none || result.hdrKind == .unknown
            ? (metadataHdrKind ?? result.hdrKind)
            : result.hdrKind
        let isHdrOrDolbyVision = effectiveHdrKind != .none && effectiveHdrKind != .unknown
        if assetProbeResult.isPlayable == false, !missing.contains("assetPlayable") {
            missing.append("assetPlayable")
        }
        if isHdrOrDolbyVision, !missing.contains("hdrProgrammableProcessingNotSupported") {
            missing.append("hdrProgrammableProcessingNotSupported")
            diagnostics["playbackPathPolicy"] = "hdrSystemPlaybackOnly"
            diagnostics["recommendedPlaybackPathReason"] = "hdrNativeFrameUnsupported"
            if let metadataHdrKind {
                diagnostics["hdrKindSource"] = "assetMetadata"
                diagnostics["assetVideoMetadataHdrKind"] = metadataHdrKind.rawValue
            }
        }

        let status: VesperPlaybackCapabilityProbeStatus
        if result.status == .unsupported || result.status == .unknown {
            status = result.status
        } else if assetProbeResult.isPlayable == false {
            status = .unsupported
        } else if isHdrOrDolbyVision && result.status == .supported {
            status = .fallbackRequired
        } else {
            status = result.status
        }
        let recommendedPlaybackPath: VesperRecommendedPlaybackPath =
            isHdrOrDolbyVision ? .systemPlayer : result.recommendedPlaybackPath
        let outputFormat: VesperPlaybackCapabilityOutputFormat =
            recommendedPlaybackPath == .systemPlayer && isHdrOrDolbyVision
            ? .surfaceOpaque
            : result.outputFormat
        let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode =
            effectiveHdrKind == .dolbyVision && result.dolbyVisionMode == .none
            ? .unsupported
            : result.dolbyVisionMode
        let confidence = confidenceAfterAssetProbe(
            baseConfidence: result.confidence,
            metadataHdrKind: metadataHdrKind
        )

        return VesperPlaybackCapabilityProbeResult(
            status: status,
            codecFamily: result.codecFamily,
            systemPlaybackSupported: result.systemPlaybackSupported &&
                assetProbeResult.isPlayable != false,
            hardwareDecodeSupported: result.hardwareDecodeSupported,
            sdkManagedNativeFrameSupported: result.sdkManagedNativeFrameSupported,
            recommendedPlaybackPath: recommendedPlaybackPath,
            outputFormat: outputFormat,
            hdrKind: effectiveHdrKind,
            dolbyVisionMode: dolbyVisionMode,
            confidence: confidence,
            missingCapabilities: missing,
            diagnostics: diagnostics
        )
    }

    private func confidenceAfterAssetProbe(
        baseConfidence: VesperPlaybackCapabilityConfidence,
        metadataHdrKind: VesperPlaybackCapabilityHdrKind?
    ) -> VesperPlaybackCapabilityConfidence {
        guard baseConfidence != .sessionProbe,
            let metadataHdrKind,
            metadataHdrKind != .none,
            metadataHdrKind != .unknown
        else {
            return baseConfidence
        }
        return .sourceMetadata
    }

    private func videoDiagnostics(for track: AVAssetTrack) async -> [String: String] {
        var diagnostics: [String: String] = [:]

        if let naturalSize = try? await track.load(.naturalSize) {
            let width = abs(Int(naturalSize.width.rounded()))
            let height = abs(Int(naturalSize.height.rounded()))
            if width > 0 {
                diagnostics["assetVideoWidth"] = String(width)
            }
            if height > 0 {
                diagnostics["assetVideoHeight"] = String(height)
            }
        }

        if let nominalFrameRate = try? await track.load(.nominalFrameRate),
            nominalFrameRate.isFinite,
            nominalFrameRate > 0
        {
            diagnostics["assetVideoFrameRate"] = String(Double(nominalFrameRate))
        }

        if let estimatedDataRate = try? await track.load(.estimatedDataRate),
            estimatedDataRate.isFinite,
            estimatedDataRate > 0
        {
            diagnostics["assetVideoEstimatedDataRate"] = String(Int(estimatedDataRate.rounded()))
        }

        if let formatDescription = (try? await track.load(.formatDescriptions))?.first {
            diagnostics["assetVideoCodec"] = iosFlutterFourCharCodeString(
                CMFormatDescriptionGetMediaSubType(formatDescription)
            )
            diagnostics.merge(formatDescriptionColorDiagnostics(formatDescription)) { _, new in new }
        }

        return diagnostics
    }

    private func formatDescriptionColorDiagnostics(
        _ formatDescription: CMFormatDescription
    ) -> [String: String] {
        guard let extensions = CMFormatDescriptionGetExtensions(formatDescription) as? [String: Any]
        else {
            return [:]
        }

        var diagnostics: [String: String] = [:]
        copyExtension(
            kCMFormatDescriptionExtension_ColorPrimaries,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoColorPrimaries"
        )
        copyExtension(
            kCMFormatDescriptionExtension_TransferFunction,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoTransferFunction"
        )
        copyExtension(
            kCMFormatDescriptionExtension_YCbCrMatrix,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoYCbCrMatrix"
        )
        diagnostics.merge(iosFlutterHdrStaticMetadataDiagnostics(from: extensions)) { _, new in
            new
        }
        if diagnostics["assetVideoTransferFunction"] != nil ||
            diagnostics["assetVideoAlternativeTransferCharacteristics"] != nil ||
            diagnostics["assetVideoMasteringDisplayColorVolumePresent"] == "true" ||
            diagnostics["assetVideoContentLightLevelInfoPresent"] == "true"
        {
            diagnostics["assetVideoHdrMetadataProbe"] = "formatDescription"
        }
        return diagnostics
    }

    private func copyExtension(
        _ key: CFString,
        from extensions: [String: Any],
        into diagnostics: inout [String: String],
        diagnosticKey: String
    ) {
        guard let value = extensions[key as String] else {
            return
        }
        diagnostics[diagnosticKey] = String(describing: value)
    }

    private func detectMetadataHdrKind(
        _ diagnostics: [String: String]
    ) -> VesperPlaybackCapabilityHdrKind? {
        if let codec = diagnostics["assetVideoCodec"]?.lowercased(),
            codec.hasPrefix("dvh1") || codec.hasPrefix("dvhe") || codec == "dolbyvision"
        {
            return .dolbyVision
        }
        guard let transferFunction = diagnostics["assetVideoTransferFunction"]?.lowercased() else {
            return nil
        }
        if transferFunction.contains("hlg") ||
            transferFunction.contains("arib") ||
            transferFunction.contains("std-b67") ||
            transferFunction.contains("std_b67")
        {
            return .hlg
        }
        if transferFunction.contains("pq") ||
            transferFunction.contains("2084") ||
            transferFunction.contains("st2084") ||
            transferFunction.contains("st_2084")
        {
            return .hdr10
        }
        return nil
    }

    @MainActor
    private func emitRuntimeHdrCapabilityWarningIfNeeded(
        for session: PlayerSession,
        errorMap: [String: Any]
    ) {
        guard let evidence = session.recentHdrProbeEvidence,
            isRuntimeCapabilityLikeError(errorMap),
            evidence.sourceFingerprint == session.currentSourceFingerprint
        else {
            return
        }

        let errorDetails = errorMap["details"] as? [String: Any]
        var capability: [String: Any] = [
            "reason": "hdrNativeFrameUnsupported",
            "recommendedPlaybackPath": "systemPlayer",
            "hdrKind": evidence.hdrKind.rawValue,
            "likelyHdrCapabilityIssue": true,
            "confidence": evidence.confidence.rawValue,
            "errorCode": errorMap["code"] as? String ?? "unknown",
            "message":
                "Playback failed after an HDR/Dolby Vision capability probe; device or display HDR capability may be involved.",
        ]
        if let capabilityFailureCause = errorDetails?["capabilityFailureCause"] as? String,
           !capabilityFailureCause.isEmpty {
            capability["capabilityFailureCause"] = capabilityFailureCause
        }
        copyRuntimeHdrCapabilityDiagnostics(from: errorDetails, into: &capability)
        if let hdrMetadata = evidence.hdrMetadata {
            capability["hdrMetadata"] = hdrMetadata
        }

        emitEvent([
            "playerId": session.id,
            "type": "warning",
            "warning": [
                "domain": "capability",
                "capability": capability,
            ],
        ])
    }

    private func isRuntimeCapabilityLikeError(_ errorMap: [String: Any]) -> Bool {
        let category = errorMap["category"] as? String
        let code = errorMap["code"] as? String
        return category == "capability" || category == "decode" || code == "unsupported"
            || code == "decodeFailure"
    }

    private func copyRuntimeHdrCapabilityDiagnostics(
        from errorDetails: [String: Any]?,
        into capability: inout [String: Any]
    ) {
        guard let errorDetails else { return }
        for key in runtimeHdrCapabilityDiagnosticKeys {
            if let value = errorDetails[key] {
                capability[key] = value
            }
        }
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
            case .created(let task):
                downloadEventSink?([
                    "downloadId": session.id,
                    "type": "taskCreated",
                    "task": task.toMap,
                ])
            case .assetIndexUpdated(let task):
                downloadEventSink?([
                    "downloadId": session.id,
                    "type": "taskUpdated",
                    "task": task.toMap,
                ])
            case .stateChanged(let patch):
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
            case .progressUpdated(let patch):
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
            "supportedPlaybackRates": VesperPlayerController.supportedPlaybackRates.map(
                Double.init),
        ]
    }

    @MainActor
    private func buildDownloadSnapshotMap(for session: DownloadSession) -> [String: Any] {
        [
            "tasks": session.manager.snapshot.tasks.map(\.toMap)
        ]
    }

    @MainActor
    private func disposeSession(_ session: PlayerSession) {
        session.cancelPendingHostDetach()
        _ = session.advanceHostDetachGeneration()
        session.observation?.cancel()
        session.pictureInPictureCoordinator?.stop()
        session.pictureInPictureCoordinator = nil
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

private struct IosFlutterAssetProbeResult: Equatable {
    let isPlayable: Bool?
    let metadataHdrKind: VesperPlaybackCapabilityHdrKind?
    let diagnostics: [String: String]

    init(
        isPlayable: Bool? = nil,
        metadataHdrKind: VesperPlaybackCapabilityHdrKind? = nil,
        diagnostics: [String: String] = [:]
    ) {
        self.isPlayable = isPlayable
        self.metadataHdrKind = metadataHdrKind
        self.diagnostics = diagnostics
    }
}

private func iosFlutterFourCharCodeString(_ value: UInt32) -> String {
    let scalarValues = [
        UInt8((value >> 24) & 0xFF),
        UInt8((value >> 16) & 0xFF),
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
    let printable = scalarValues.allSatisfy { (0x20 ... 0x7E).contains($0) }
    guard printable else {
        return String(format: "0x%08X", value)
    }
    return String(bytes: scalarValues, encoding: .ascii) ?? String(format: "0x%08X", value)
}

private func iosFlutterHdrStaticMetadataDiagnostics(from extensions: [String: Any]) -> [String: String] {
    var diagnostics: [String: String] = [:]
    if let alternativeTransfer =
        extensions[kCMFormatDescriptionExtension_AlternativeTransferCharacteristics as String]
    {
        diagnostics["assetVideoAlternativeTransferCharacteristics"] = String(describing: alternativeTransfer)
    }
    appendIosFlutterMasteringDisplayColorVolume(from: extensions, into: &diagnostics)
    appendIosFlutterContentLightLevelInfo(from: extensions, into: &diagnostics)
    return diagnostics
}

private func appendIosFlutterMasteringDisplayColorVolume(
    from extensions: [String: Any],
    into diagnostics: inout [String: String]
) {
    guard let data = iosFlutterDataValue(
        extensions[kCMFormatDescriptionExtension_MasteringDisplayColorVolume as String]
    ) else {
        return
    }
    diagnostics["assetVideoMasteringDisplayColorVolumePresent"] = "true"
    diagnostics["assetVideoMasteringDisplayColorVolumeByteLength"] = String(data.count)
    guard data.count >= 24 else {
        diagnostics["assetVideoMasteringDisplayColorVolumeParseError"] = "tooShort"
        return
    }

    diagnostics["assetVideoMasteringDisplayPrimary0"] = iosFlutterChromaticityPair(
        iosFlutterReadUInt16(data, offset: 0),
        iosFlutterReadUInt16(data, offset: 2)
    )
    diagnostics["assetVideoMasteringDisplayPrimary1"] = iosFlutterChromaticityPair(
        iosFlutterReadUInt16(data, offset: 4),
        iosFlutterReadUInt16(data, offset: 6)
    )
    diagnostics["assetVideoMasteringDisplayPrimary2"] = iosFlutterChromaticityPair(
        iosFlutterReadUInt16(data, offset: 8),
        iosFlutterReadUInt16(data, offset: 10)
    )
    diagnostics["assetVideoMasteringDisplayWhitePoint"] = iosFlutterChromaticityPair(
        iosFlutterReadUInt16(data, offset: 12),
        iosFlutterReadUInt16(data, offset: 14)
    )
    diagnostics["assetVideoMasteringDisplayMaxLuminanceNits"] = String(
        iosFlutterReadUInt32(data, offset: 16)
    )
    diagnostics["assetVideoMasteringDisplayMinLuminanceNits"] = iosFlutterDecimalString(
        Double(iosFlutterReadUInt32(data, offset: 20)) / 10_000,
        digits: 4
    )
}

private func appendIosFlutterContentLightLevelInfo(
    from extensions: [String: Any],
    into diagnostics: inout [String: String]
) {
    guard let data = iosFlutterDataValue(
        extensions[kCMFormatDescriptionExtension_ContentLightLevelInfo as String]
    ) else {
        return
    }
    diagnostics["assetVideoContentLightLevelInfoPresent"] = "true"
    diagnostics["assetVideoContentLightLevelInfoByteLength"] = String(data.count)
    guard data.count >= 4 else {
        diagnostics["assetVideoContentLightLevelInfoParseError"] = "tooShort"
        return
    }

    diagnostics["assetVideoMaxContentLightLevelNits"] = String(iosFlutterReadUInt16(data, offset: 0))
    diagnostics["assetVideoMaxFrameAverageLightLevelNits"] = String(
        iosFlutterReadUInt16(data, offset: 2)
    )
}

private func iosFlutterDataValue(_ value: Any?) -> Data? {
    if let data = value as? Data {
        return data
    }
    return (value as? NSData).map(Data.init)
}

private func iosFlutterReadUInt16(_ data: Data, offset: Int) -> UInt16 {
    (UInt16(data[offset]) << 8) | UInt16(data[offset + 1])
}

private func iosFlutterReadUInt32(_ data: Data, offset: Int) -> UInt32 {
    (UInt32(data[offset]) << 24) |
        (UInt32(data[offset + 1]) << 16) |
        (UInt32(data[offset + 2]) << 8) |
        UInt32(data[offset + 3])
}

private func iosFlutterChromaticityPair(_ x: UInt16, _ y: UInt16) -> String {
    "\(iosFlutterDecimalString(Double(x) / 50_000, digits: 5)),\(iosFlutterDecimalString(Double(y) / 50_000, digits: 5))"
}

private func iosFlutterDecimalString(_ value: Double, digits: Int) -> String {
    String(format: "%.\(digits)f", locale: Locale(identifier: "en_US_POSIX"), value)
}
