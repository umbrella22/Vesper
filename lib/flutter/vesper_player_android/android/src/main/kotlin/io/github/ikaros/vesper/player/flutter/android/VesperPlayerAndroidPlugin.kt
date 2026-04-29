package io.github.ikaros.vesper.player.flutter.android

import android.content.Context
import android.graphics.Color
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.StandardMessageCodec
import io.flutter.plugin.platform.PlatformView
import io.flutter.plugin.platform.PlatformViewFactory
import io.github.ikaros.vesper.player.android.NativeVideoSurfaceKind
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerBridgeBackend
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperAbrPolicy
import io.github.ikaros.vesper.player.android.VesperBufferingPolicy
import io.github.ikaros.vesper.player.android.VesperBufferingPreset
import io.github.ikaros.vesper.player.android.VesperCachePolicy
import io.github.ikaros.vesper.player.android.VesperCachePreset
import io.github.ikaros.vesper.player.android.VesperDownloadAssetIndex
import io.github.ikaros.vesper.player.android.VesperDownloadConfiguration
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadError
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperDownloadProfile
import io.github.ikaros.vesper.player.android.VesperDownloadProgressSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadResourceRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSegmentRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperMediaTrackKind
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperRetryBackoff
import io.github.ikaros.vesper.player.android.VesperRetryPolicy
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackPreferencePolicy
import io.github.ikaros.vesper.player.android.VesperPreloadBudgetPolicy
import io.github.ikaros.vesper.player.android.VesperTrackSelection
import io.github.ikaros.vesper.player.android.VesperTrackSelectionMode
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch

class VesperPlayerAndroidPlugin :
    PlatformViewFactory(StandardMessageCodec.INSTANCE),
    FlutterPlugin,
    MethodChannel.MethodCallHandler,
    EventChannel.StreamHandler,
    ActivityAware {
    private lateinit var methodChannel: MethodChannel
    private lateinit var eventChannel: EventChannel
    private lateinit var downloadEventChannel: EventChannel
    private lateinit var applicationContext: Context

    private var eventSink: EventChannel.EventSink? = null
    private var downloadEventSink: EventChannel.EventSink? = null

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val sessions = linkedMapOf<String, PlayerSession>()
    private val downloadSessions = linkedMapOf<String, DownloadSession>()

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        applicationContext = binding.applicationContext
        methodChannel = MethodChannel(binding.binaryMessenger, METHOD_CHANNEL_NAME)
        eventChannel = EventChannel(binding.binaryMessenger, EVENT_CHANNEL_NAME)
        downloadEventChannel =
            EventChannel(binding.binaryMessenger, DOWNLOAD_EVENT_CHANNEL_NAME)
        methodChannel.setMethodCallHandler(this)
        eventChannel.setStreamHandler(this)
        downloadEventChannel.setStreamHandler(
            object : EventChannel.StreamHandler {
                override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
                    downloadEventSink = events
                    downloadSessions.values.forEach(::emitDownloadSnapshot)
                }

                override fun onCancel(arguments: Any?) {
                    downloadEventSink = null
                }
            },
        )
        binding.platformViewRegistry.registerViewFactory(PLAYER_VIEW_TYPE, this)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        disposeAllSessions()
        disposeAllDownloadSessions()
        eventSink = null
        downloadEventSink = null
        eventChannel.setStreamHandler(null)
        downloadEventChannel.setStreamHandler(null)
        methodChannel.setMethodCallHandler(null)
        scope.cancel()
    }

    override fun onAttachedToActivity(binding: ActivityPluginBinding) = Unit

    override fun onDetachedFromActivityForConfigChanges() = Unit

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) = Unit

    override fun onDetachedFromActivity() = Unit

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "createPlayer" -> handleCreatePlayer(call, result)
            "createDownloadManager" -> handleCreateDownloadManager(call, result)
            "disposePlayer" -> handleSessionCommand(call, result) { session ->
                disposeSession(session)
                null
            }
            "refreshPlayer" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.refresh()
                emitSnapshot(session)
                null
            }
            "refreshDownloadManager" -> handleDownloadSessionCommand(call, result) { session ->
                session.lastError = null
                session.manager.refresh()
                emitDownloadSnapshot(session)
                null
            }
            "disposeDownloadManager" -> handleDownloadSessionCommand(call, result) { session ->
                disposeDownloadSession(session)
                null
            }
            "initialize" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.initialize()
                emitSnapshot(session)
                null
            }
            "selectSource" -> handleSessionCommand(call, result) { session ->
                val sourceMap = requireNestedMap(call.argumentMap(), "source")
                session.lastError = null
                session.controller.selectSource(sourceMap.toVesperPlayerSource())
                emitSnapshot(session)
                null
            }
            "play" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.play()
                emitSnapshot(session)
                null
            }
            "pause" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.pause()
                emitSnapshot(session)
                null
            }
            "togglePause" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.togglePause()
                emitSnapshot(session)
                null
            }
            "stop" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.stop()
                emitSnapshot(session)
                null
            }
            "seekBy" -> handleSessionCommand(call, result) { session ->
                val deltaMs = (call.argumentMap()["deltaMs"] as? Number)?.toLong()
                    ?: throw IllegalArgumentException("Missing deltaMs.")
                session.lastError = null
                session.controller.seekBy(deltaMs)
                emitSnapshot(session)
                null
            }
            "seekToRatio" -> handleSessionCommand(call, result) { session ->
                val ratio = (call.argumentMap()["ratio"] as? Number)?.toFloat()
                    ?: throw IllegalArgumentException("Missing ratio.")
                session.lastError = null
                session.controller.seekToRatio(ratio)
                emitSnapshot(session)
                null
            }
            "seekToLiveEdge" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.seekToLiveEdge()
                emitSnapshot(session)
                null
            }
            "setPlaybackRate" -> handleSessionCommand(call, result) { session ->
                val rate = (call.argumentMap()["rate"] as? Number)?.toFloat()
                    ?: throw IllegalArgumentException("Missing rate.")
                session.lastError = null
                session.controller.setPlaybackRate(rate)
                emitSnapshot(session)
                null
            }
            "setVideoTrackSelection" -> handleSessionCommand(call, result) { session ->
                val selectionMap = requireNestedMap(call.argumentMap(), "selection")
                session.lastError = null
                session.controller.setVideoTrackSelection(selectionMap.toTrackSelection())
                emitSnapshot(session)
                null
            }
            "setAudioTrackSelection" -> handleSessionCommand(call, result) { session ->
                val selectionMap = requireNestedMap(call.argumentMap(), "selection")
                session.lastError = null
                session.controller.setAudioTrackSelection(selectionMap.toTrackSelection())
                emitSnapshot(session)
                null
            }
            "setSubtitleTrackSelection" -> handleSessionCommand(call, result) { session ->
                val selectionMap = requireNestedMap(call.argumentMap(), "selection")
                session.lastError = null
                session.controller.setSubtitleTrackSelection(selectionMap.toTrackSelection())
                emitSnapshot(session)
                null
            }
            "setAbrPolicy" -> handleSessionCommand(call, result) { session ->
                val policyMap = requireNestedMap(call.argumentMap(), "policy")
                session.lastError = null
                session.controller.setAbrPolicy(policyMap.toAbrPolicy())
                emitSnapshot(session)
                null
            }
            "setResiliencePolicy" -> handleSessionCommand(call, result) { session ->
                val policyMap = requireNestedMap(call.argumentMap(), "policy")
                session.lastError = null
                session.controller.setResiliencePolicy(policyMap.toResiliencePolicy())
                emitSnapshot(session)
                null
            }
            "updateViewport" -> handleSessionCommand(call, result) { session ->
                val viewportMap = requireNestedMap(call.argumentMap(), "viewport")
                val viewportHintMap =
                    (call.argumentMap()["viewportHint"] as? Map<*, *>)?.stringMap()
                session.lastError = null
                session.viewport = viewportMap.toFlutterViewport()
                session.viewportHint =
                    viewportHintMap?.toFlutterViewportHint() ?: FlutterViewportHint.hidden()
                emitSnapshot(session)
                null
            }
            "clearViewport" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.viewport = null
                session.viewportHint = FlutterViewportHint.hidden()
                emitSnapshot(session)
                null
            }
            "createDownloadTask" -> handleDownloadSessionCommand(call, result) { session ->
                val arguments = call.argumentMap()
                val assetId = arguments["assetId"] as? String
                    ?: throw IllegalArgumentException("Missing assetId.")
                val sourceMap = requireNestedMap(arguments, "source")
                val profileMap = requireNestedMap(arguments, "profile")
                val assetIndexMap = requireNestedMap(arguments, "assetIndex")
                session.lastError = null
                session.manager.createTask(
                    assetId = assetId,
                    source = sourceMap.toDownloadSource(),
                    profile = profileMap.toDownloadProfile(),
                    assetIndex = assetIndexMap.toDownloadAssetIndex(),
                )
            }
            "startDownloadTask" -> handleDownloadTaskAction(call, result) { session, taskId ->
                session.manager.startTask(taskId)
            }
            "pauseDownloadTask" -> handleDownloadTaskAction(call, result) { session, taskId ->
                session.manager.pauseTask(taskId)
            }
            "resumeDownloadTask" -> handleDownloadTaskAction(call, result) { session, taskId ->
                session.manager.resumeTask(taskId)
            }
            "removeDownloadTask" -> handleDownloadTaskAction(call, result) { session, taskId ->
                session.manager.removeTask(taskId)
            }
            "exportDownloadTask" -> handleDownloadExportTask(call, result)
            else -> result.notImplemented()
        }
    }

    override fun create(context: Context, viewId: Int, args: Any?): PlatformView {
        val arguments = (args as? Map<*, *>)?.stringMap() ?: emptyMap()
        val playerId = arguments["playerId"] as? String
        val host = FrameLayout(context).apply {
            layoutParams = FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            )
            setBackgroundColor(Color.TRANSPARENT)
            clipChildren = false
            clipToPadding = false
        }

        if (!playerId.isNullOrBlank()) {
            bindSessionHost(playerId, host)
        }

        return VesperPlayerPlatformView(host) {
            if (!playerId.isNullOrBlank()) {
                unbindSessionHost(playerId, host)
            }
        }
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
        eventSink = events
        sessions.values.forEach(::emitSnapshot)
    }

    override fun onCancel(arguments: Any?) {
        eventSink = null
    }

    private fun handleCreatePlayer(call: MethodCall, result: MethodChannel.Result) {
        runCatching {
            val arguments = call.argumentMap()
            val initialSourceMap = arguments["initialSource"] as? Map<*, *>
            val resiliencePolicyMap = arguments["resiliencePolicy"] as? Map<*, *>
            val trackPreferencePolicyMap = arguments["trackPreferencePolicy"] as? Map<*, *>
            val preloadBudgetPolicyMap = arguments["preloadBudgetPolicy"] as? Map<*, *>

            val session = PlayerSession(
                id = UUID.randomUUID().toString(),
                controller = VesperPlayerControllerFactory.createDefault(
                    context = applicationContext,
                    initialSource = initialSourceMap?.stringMap()?.toVesperPlayerSource(),
                    resiliencePolicy = resiliencePolicyMap?.stringMap()?.toResiliencePolicy()
                        ?: VesperPlaybackResiliencePolicy(),
                    trackPreferencePolicy =
                        trackPreferencePolicyMap?.stringMap()?.toTrackPreferencePolicy()
                            ?: VesperTrackPreferencePolicy(),
                    preloadBudgetPolicy =
                        preloadBudgetPolicyMap?.stringMap()?.toPreloadBudgetPolicy()
                            ?: VesperPreloadBudgetPolicy(),
                    surfaceKind = NativeVideoSurfaceKind.SurfaceView,
                ),
            )

            sessions[session.id] = session
            observeSession(session)

            mapOf(
                "playerId" to session.id,
                "snapshot" to buildSnapshotMap(session),
            )
        }.onSuccess(result::success)
            .onFailure { error ->
                result.error(
                    "vesper_create_failed",
                    error.message,
                    error.toErrorMap(),
                )
            }
    }

    private fun handleCreateDownloadManager(call: MethodCall, result: MethodChannel.Result) {
        runCatching {
            val arguments = call.argumentMap()
            val configurationMap = requireNestedMap(arguments, "configuration")
            val session =
                DownloadSession(
                    id = UUID.randomUUID().toString(),
                    manager =
                        VesperDownloadManager(
                            context = applicationContext,
                            configuration = configurationMap.toDownloadConfiguration(),
                        ),
                )
            downloadSessions[session.id] = session
            observeDownloadSession(session)
            mapOf(
                "downloadId" to session.id,
                "snapshot" to buildDownloadSnapshotMap(session),
            )
        }.onSuccess(result::success)
            .onFailure { error ->
                result.error(
                    "vesper_download_create_failed",
                    error.message,
                    error.toDownloadErrorMap(),
                )
            }
    }

    private fun handleSessionCommand(
        call: MethodCall,
        result: MethodChannel.Result,
        action: (PlayerSession) -> Any?,
    ) {
        val sessionId = call.argumentMap()["playerId"] as? String
        if (sessionId.isNullOrBlank()) {
            result.error(
                "vesper_missing_player_id",
                "Missing playerId.",
                mapOf("message" to "Missing playerId.", "category" to "platform", "retriable" to false),
            )
            return
        }

        val session = sessions[sessionId]
        if (session == null) {
            result.error(
                "vesper_unknown_player",
                "Unknown playerId: $sessionId",
                mapOf(
                    "message" to "Unknown playerId: $sessionId",
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
            return
        }

        runCatching {
            action(session)
        }.onSuccess(result::success)
            .onFailure { error ->
                session.lastError = error.toErrorMap()
                emitError(session, error)
                result.error(
                    "vesper_operation_failed",
                    error.message,
                    session.lastError,
                )
            }
    }

    private fun handleDownloadSessionCommand(
        call: MethodCall,
        result: MethodChannel.Result,
        action: (DownloadSession) -> Any?,
    ) {
        val sessionId = call.argumentMap()["downloadId"] as? String
        if (sessionId.isNullOrBlank()) {
            result.error(
                "vesper_missing_download_id",
                "Missing downloadId.",
                mapOf(
                    "message" to "Missing downloadId.",
                    "codeOrdinal" to 0,
                    "categoryOrdinal" to 0,
                    "retriable" to false,
                ),
            )
            return
        }

        val session = downloadSessions[sessionId]
        if (session == null) {
            result.error(
                "vesper_unknown_download",
                "Unknown downloadId: $sessionId",
                mapOf(
                    "message" to "Unknown downloadId: $sessionId",
                    "codeOrdinal" to 0,
                    "categoryOrdinal" to 0,
                    "retriable" to false,
                ),
            )
            return
        }

        runCatching {
            action(session)
        }.onSuccess(result::success)
            .onFailure { error ->
                session.lastError = error.toDownloadErrorMap()
                emitDownloadError(session, error)
                result.error(
                    "vesper_download_operation_failed",
                    error.message,
                    session.lastError,
                )
            }
    }

    private fun handleDownloadTaskAction(
        call: MethodCall,
        result: MethodChannel.Result,
        action: (DownloadSession, Long) -> Boolean,
    ) {
        handleDownloadSessionCommand(call, result) { session ->
            val taskId = (call.argumentMap()["taskId"] as? Number)?.toLong()
                ?: throw IllegalArgumentException("Missing taskId.")
            session.lastError = null
            action(session, taskId)
        }
    }

    private fun handleDownloadExportTask(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val sessionId = call.argumentMap()["downloadId"] as? String
        if (sessionId.isNullOrBlank()) {
            result.error(
                "vesper_missing_download_id",
                "Missing downloadId.",
                mapOf(
                    "message" to "Missing downloadId.",
                    "codeOrdinal" to 0,
                    "categoryOrdinal" to 0,
                    "retriable" to false,
                ),
            )
            return
        }

        val session = downloadSessions[sessionId]
        if (session == null) {
            result.error(
                "vesper_unknown_download",
                "Unknown downloadId: $sessionId",
                mapOf(
                    "message" to "Unknown downloadId: $sessionId",
                    "codeOrdinal" to 0,
                    "categoryOrdinal" to 0,
                    "retriable" to false,
                ),
            )
            return
        }

        val arguments = call.argumentMap()
        val taskId =
            (arguments["taskId"] as? Number)?.toLong()
                ?: run {
                    result.error(
                        "vesper_missing_task_id",
                        "Missing taskId.",
                        mapOf(
                            "message" to "Missing taskId.",
                            "codeOrdinal" to 0,
                            "categoryOrdinal" to 0,
                            "retriable" to false,
                        ),
                    )
                    return
                }
        val outputPath =
            arguments["outputPath"] as? String
                ?: run {
                    result.error(
                        "vesper_missing_output_path",
                        "Missing outputPath.",
                        mapOf(
                            "message" to "Missing outputPath.",
                            "codeOrdinal" to 0,
                            "categoryOrdinal" to 0,
                            "retriable" to false,
                        ),
                    )
                    return
                }

        scope.launch {
            runCatching {
                session.lastError = null
                session.manager.exportTaskOutput(
                    taskId = taskId,
                    outputPath = outputPath,
                    onProgress = { ratio ->
                        scope.launch {
                            emitDownloadExportProgress(session, taskId, ratio)
                        }
                    },
                )
            }.onSuccess {
                result.success(null)
            }.onFailure { error ->
                session.lastError = error.toDownloadErrorMap()
                emitDownloadError(session, error)
                result.error(
                    "vesper_download_operation_failed",
                    error.message,
                    session.lastError,
                )
            }
        }
    }

    private fun observeSession(session: PlayerSession) {
        session.observerJob = scope.launch {
            combine(
                session.controller.uiState,
                session.controller.trackCatalog,
                session.controller.trackSelection,
            ) { _, _, _ ->
                buildSnapshotMap(session)
            }.collect { snapshot ->
                emitEvent(
                    mapOf(
                        "playerId" to session.id,
                        "type" to "snapshot",
                        "snapshot" to snapshot,
                    ),
                )
            }
        }
    }

    private fun observeDownloadSession(session: DownloadSession) {
        session.observerJob = scope.launch {
            session.manager.snapshot.collect {
                emitDownloadSnapshot(session)
            }
        }
    }

    private fun emitSnapshot(session: PlayerSession) {
        emitEvent(
            mapOf(
                "playerId" to session.id,
                "type" to "snapshot",
                "snapshot" to buildSnapshotMap(session),
            ),
        )
    }

    private fun emitError(session: PlayerSession, error: Throwable) {
        emitEvent(
            mapOf(
                "playerId" to session.id,
                "type" to "error",
                "error" to (session.lastError ?: error.toErrorMap()),
                "snapshot" to buildSnapshotMap(session),
            ),
        )
    }

    private fun emitDownloadSnapshot(session: DownloadSession) {
        downloadEventSink?.success(
            mapOf(
                "downloadId" to session.id,
                "type" to "snapshot",
                "snapshot" to buildDownloadSnapshotMap(session),
            ),
        )
    }

    private fun emitDownloadError(session: DownloadSession, error: Throwable) {
        downloadEventSink?.success(
            mapOf(
                "downloadId" to session.id,
                "type" to "error",
                "error" to (session.lastError ?: error.toDownloadErrorMap()),
                "snapshot" to buildDownloadSnapshotMap(session),
            ),
        )
    }

    private fun emitDownloadExportProgress(
        session: DownloadSession,
        taskId: Long,
        ratio: Float,
    ) {
        downloadEventSink?.success(
            mapOf(
                "downloadId" to session.id,
                "type" to "exportProgress",
                "taskId" to taskId,
                "ratio" to ratio.coerceIn(0f, 1f).toDouble(),
            ),
        )
    }

    private fun emitEvent(payload: Map<String, Any?>) {
        eventSink?.success(payload)
    }

    private fun buildSnapshotMap(session: PlayerSession): Map<String, Any?> {
        val uiState = session.controller.uiState.value
        val trackCatalog = session.controller.trackCatalog.value
        val trackSelection = session.controller.trackSelection.value
        val effectiveVideoTrackId = session.controller.effectiveVideoTrackId.value
        val videoVariantObservation = session.controller.videoVariantObservation.value
        val resiliencePolicy = session.controller.resiliencePolicy.value

        return mapOf(
            "title" to uiState.title,
            "subtitle" to uiState.subtitle,
            "sourceLabel" to uiState.sourceLabel,
            "playbackState" to uiState.playbackState.toWireName(),
            "playbackRate" to uiState.playbackRate.toDouble(),
            "isBuffering" to uiState.isBuffering,
            "isInterrupted" to uiState.isInterrupted,
            "hasVideoSurface" to session.hasAttachedHost(),
            "timeline" to uiState.timeline.toMap(),
            "viewport" to session.viewport?.toMap(),
            "viewportHint" to session.viewportHint.toMap(),
            "backendFamily" to session.controller.backend.toBackendFamilyWireName(),
            "capabilities" to buildCapabilitiesMap(),
            "trackCatalog" to trackCatalog.toMap(),
            "trackSelection" to trackSelection.toMap(),
            "effectiveVideoTrackId" to effectiveVideoTrackId,
            "videoVariantObservation" to videoVariantObservation?.toMap(),
            "resiliencePolicy" to resiliencePolicy.toMap(),
            "lastError" to session.lastError,
        )
    }

    private fun buildCapabilitiesMap(): Map<String, Any?> {
        return mapOf(
            "supportsLocalFiles" to true,
            "supportsRemoteUrls" to true,
            "supportsHls" to true,
            "supportsDash" to true,
            "supportsTrackCatalog" to true,
            "supportsTrackSelection" to true,
            "supportsVideoTrackSelection" to true,
            "supportsAudioTrackSelection" to true,
            "supportsSubtitleTrackSelection" to true,
            "supportsAbrPolicy" to true,
            "supportsAbrConstrained" to true,
            "supportsAbrFixedTrack" to true,
            "supportsAbrMaxBitRate" to true,
            "supportsAbrMaxResolution" to true,
            "supportsResiliencePolicy" to true,
            "supportsHolePunch" to false,
            "supportsPlaybackRate" to true,
            "supportsLiveEdgeSeeking" to true,
            "isExperimental" to false,
            "supportedPlaybackRates" to VesperPlayerController.supportedPlaybackRates
                .map { rate -> rate.toDouble() },
        )
    }

    private fun buildDownloadSnapshotMap(session: DownloadSession): Map<String, Any?> =
        mapOf(
            "tasks" to session.manager.snapshot.value.tasks
                .map(VesperDownloadTaskSnapshot::toMap),
        )

    private fun bindSessionHost(playerId: String, host: FrameLayout) {
        val session = sessions[playerId] ?: return
        if (session.hostView === host) {
            session.controller.attachSurfaceHost(host)
            emitSnapshot(session)
            return
        }

        session.hostView?.let(session.controller::detachSurfaceHost)
        session.hostView = host
        session.controller.attachSurfaceHost(host)
        emitSnapshot(session)
    }

    private fun unbindSessionHost(playerId: String, host: FrameLayout) {
        val session = sessions[playerId] ?: return
        if (session.hostView !== host) {
            return
        }
        session.controller.detachSurfaceHost(host)
        session.hostView = null
        emitSnapshot(session)
    }

    private fun disposeSession(session: PlayerSession) {
        session.observerJob?.cancel()
        session.hostView?.let(session.controller::detachSurfaceHost)
        session.hostView = null
        session.controller.dispose()
        sessions.remove(session.id)
        emitEvent(
            mapOf(
                "playerId" to session.id,
                "type" to "disposed",
            ),
        )
    }

    private fun disposeDownloadSession(session: DownloadSession) {
        session.observerJob?.cancel()
        session.manager.dispose()
        downloadSessions.remove(session.id)
        downloadEventSink?.success(
            mapOf(
                "downloadId" to session.id,
                "type" to "disposed",
            ),
        )
    }

    private fun disposeAllSessions() {
        sessions.values.toList().forEach(::disposeSession)
        sessions.clear()
    }

    private fun disposeAllDownloadSessions() {
        downloadSessions.values.toList().forEach(::disposeDownloadSession)
        downloadSessions.clear()
    }
}

private data class PlayerSession(
    val id: String,
    val controller: VesperPlayerController,
    var hostView: FrameLayout? = null,
    var observerJob: Job? = null,
    var lastError: Map<String, Any?>? = null,
    var viewport: FlutterViewport? = null,
    var viewportHint: FlutterViewportHint = FlutterViewportHint.hidden(),
) {
    fun hasAttachedHost(): Boolean = hostView != null
}

private data class DownloadSession(
    val id: String,
    val manager: VesperDownloadManager,
    var observerJob: Job? = null,
    var lastError: Map<String, Any?>? = null,
)

private data class FlutterViewport(
    val left: Double,
    val top: Double,
    val width: Double,
    val height: Double,
) {
    fun toMap(): Map<String, Any> =
        mapOf(
            "left" to left,
            "top" to top,
            "width" to width,
            "height" to height,
        )
}

private data class FlutterViewportHint(
    val kind: String,
    val visibleFraction: Double,
) {
    fun toMap(): Map<String, Any> =
        mapOf(
            "kind" to kind,
            "visibleFraction" to visibleFraction,
        )

    companion object {
        fun hidden(): FlutterViewportHint = FlutterViewportHint("hidden", 0.0)
    }
}

private class VesperPlayerPlatformView(
    private val hostView: FrameLayout,
    private val onDispose: () -> Unit,
) : PlatformView {
    override fun getView(): View = hostView

    override fun dispose() {
        onDispose()
    }
}

private fun MethodCall.argumentMap(): Map<String, Any?> =
    (arguments as? Map<*, *>)?.stringMap() ?: emptyMap()

private fun Map<*, *>.stringMap(): Map<String, Any?> =
    entries.associate { (key, value) -> key.toString() to value }

private fun requireNestedMap(
    arguments: Map<String, Any?>,
    key: String,
): Map<String, Any?> {
    val raw = arguments[key] as? Map<*, *>
    return raw?.stringMap() ?: throw IllegalArgumentException("Missing $key.")
}

private fun Map<String, Any?>.toVesperPlayerSource(): VesperPlayerSource {
    val uri = this["uri"] as? String ?: throw IllegalArgumentException("Missing source uri.")
    val label = this["label"] as? String ?: uri
    return VesperPlayerSource(
        uri = uri,
        label = label,
        kind = when (this["kind"] as? String) {
            "remote" -> VesperPlayerSourceKind.Remote
            else -> VesperPlayerSourceKind.Local
        },
        protocol = when (this["protocol"] as? String) {
            "file" -> VesperPlayerSourceProtocol.File
            "content" -> VesperPlayerSourceProtocol.Content
            "progressive" -> VesperPlayerSourceProtocol.Progressive
            "hls" -> VesperPlayerSourceProtocol.Hls
            "dash" -> VesperPlayerSourceProtocol.Dash
            else -> VesperPlayerSourceProtocol.Unknown
        },
    )
}

private fun Map<String, Any?>.toDownloadConfiguration(): VesperDownloadConfiguration =
    VesperDownloadConfiguration(
        autoStart = this["autoStart"] as? Boolean ?: true,
        runPostProcessorsOnCompletion =
            this["runPostProcessorsOnCompletion"] as? Boolean ?: true,
        pluginLibraryPaths =
            (this["pluginLibraryPaths"] as? List<*>)
                ?.mapNotNull { value -> value?.toString() }
                ?: emptyList(),
    )

private fun Map<String, Any?>.toDownloadSource(): VesperDownloadSource =
    VesperDownloadSource(
        source = requireNestedMap(this, "source").toVesperPlayerSource(),
        contentFormat =
            when (this["contentFormat"] as? String) {
                "hlsSegments" -> VesperDownloadContentFormat.HlsSegments
                "dashSegments" -> VesperDownloadContentFormat.DashSegments
                "singleFile" -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        manifestUri = this["manifestUri"] as? String,
    )

private fun Map<String, Any?>.toDownloadProfile(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = this["variantId"] as? String,
        preferredAudioLanguage = this["preferredAudioLanguage"] as? String,
        preferredSubtitleLanguage = this["preferredSubtitleLanguage"] as? String,
        selectedTrackIds =
            (this["selectedTrackIds"] as? List<*>)
                ?.mapNotNull { value -> value?.toString() }
                ?: emptyList(),
        targetDirectory = this["targetDirectory"] as? String,
        allowMeteredNetwork = this["allowMeteredNetwork"] as? Boolean ?: false,
    )

private fun Map<String, Any?>.toDownloadAssetIndex(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat =
            when (this["contentFormat"] as? String) {
                "hlsSegments" -> VesperDownloadContentFormat.HlsSegments
                "dashSegments" -> VesperDownloadContentFormat.DashSegments
                "singleFile" -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        version = this["version"] as? String,
        etag = this["etag"] as? String,
        checksum = this["checksum"] as? String,
        totalSizeBytes = (this["totalSizeBytes"] as? Number)?.toLong(),
        resources =
            (this["resources"] as? List<*>)
                ?.mapNotNull { value ->
                    (value as? Map<*, *>)?.stringMap()?.toDownloadResourceRecord()
                }
                ?: emptyList(),
        segments =
            (this["segments"] as? List<*>)
                ?.mapNotNull { value ->
                    (value as? Map<*, *>)?.stringMap()?.toDownloadSegmentRecord()
                }
                ?: emptyList(),
        completedPath = this["completedPath"] as? String,
    )

private fun Map<String, Any?>.toDownloadResourceRecord(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = this["resourceId"] as? String ?: "",
        uri = this["uri"] as? String ?: "",
        relativePath = this["relativePath"] as? String,
        sizeBytes = (this["sizeBytes"] as? Number)?.toLong(),
        etag = this["etag"] as? String,
        checksum = this["checksum"] as? String,
    )

private fun Map<String, Any?>.toDownloadSegmentRecord(): VesperDownloadSegmentRecord =
    VesperDownloadSegmentRecord(
        segmentId = this["segmentId"] as? String ?: "",
        uri = this["uri"] as? String ?: "",
        relativePath = this["relativePath"] as? String,
        sequence = (this["sequence"] as? Number)?.toLong(),
        sizeBytes = (this["sizeBytes"] as? Number)?.toLong(),
        checksum = this["checksum"] as? String,
    )

private fun Map<String, Any?>.toTrackSelection(): VesperTrackSelection =
    when (this["mode"] as? String) {
        "disabled" -> VesperTrackSelection.disabled()
        "track" -> {
            val trackId = this["trackId"] as? String
                ?: throw IllegalArgumentException("Missing trackId for track selection.")
            VesperTrackSelection.track(trackId)
        }
        else -> VesperTrackSelection.auto()
    }

private fun Map<String, Any?>.toAbrPolicy(): VesperAbrPolicy =
    when (this["mode"] as? String) {
        "constrained" -> VesperAbrPolicy.constrained(
            maxBitRate = (this["maxBitRate"] as? Number)?.toLong(),
            maxWidth = (this["maxWidth"] as? Number)?.toInt(),
            maxHeight = (this["maxHeight"] as? Number)?.toInt(),
        )
        "fixedTrack" -> {
            val trackId = this["trackId"] as? String
                ?: throw IllegalArgumentException("Missing trackId for fixed track policy.")
            VesperAbrPolicy.fixedTrack(trackId)
        }
        else -> VesperAbrPolicy.auto()
    }

private fun Map<String, Any?>.toResiliencePolicy(): VesperPlaybackResiliencePolicy {
    val buffering = (this["buffering"] as? Map<*, *>)?.stringMap()?.toBufferingPolicy()
        ?: VesperBufferingPolicy()
    val retry = (this["retry"] as? Map<*, *>)?.stringMap()?.toRetryPolicy()
        ?: VesperRetryPolicy()
    val cache = (this["cache"] as? Map<*, *>)?.stringMap()?.toCachePolicy()
        ?: VesperCachePolicy()
    return VesperPlaybackResiliencePolicy(
        buffering = buffering,
        retry = retry,
        cache = cache,
    )
}

private fun Map<String, Any?>.toTrackPreferencePolicy(): VesperTrackPreferencePolicy {
    val audioSelection =
        (this["audioSelection"] as? Map<*, *>)?.stringMap()?.toTrackSelection()
            ?: VesperTrackSelection.auto()
    val subtitleSelection =
        (this["subtitleSelection"] as? Map<*, *>)?.stringMap()?.toTrackSelection()
            ?: VesperTrackSelection.disabled()
    val abrPolicy =
        (this["abrPolicy"] as? Map<*, *>)?.stringMap()?.toAbrPolicy()
            ?: VesperAbrPolicy.auto()
    return VesperTrackPreferencePolicy(
        preferredAudioLanguage = this["preferredAudioLanguage"] as? String,
        preferredSubtitleLanguage = this["preferredSubtitleLanguage"] as? String,
        selectSubtitlesByDefault = this["selectSubtitlesByDefault"] as? Boolean ?: false,
        selectUndeterminedSubtitleLanguage =
            this["selectUndeterminedSubtitleLanguage"] as? Boolean ?: false,
        audioSelection = audioSelection,
        subtitleSelection = subtitleSelection,
        abrPolicy = abrPolicy,
    )
}

private fun Map<String, Any?>.toPreloadBudgetPolicy(): VesperPreloadBudgetPolicy =
    VesperPreloadBudgetPolicy(
        maxConcurrentTasks = (this["maxConcurrentTasks"] as? Number)?.toInt(),
        maxMemoryBytes = (this["maxMemoryBytes"] as? Number)?.toLong(),
        maxDiskBytes = (this["maxDiskBytes"] as? Number)?.toLong(),
        warmupWindowMs = (this["warmupWindowMs"] as? Number)?.toLong(),
    )

private fun Map<String, Any?>.toFlutterViewport(): FlutterViewport =
    FlutterViewport(
        left = (this["left"] as? Number)?.toDouble() ?: 0.0,
        top = (this["top"] as? Number)?.toDouble() ?: 0.0,
        width = (this["width"] as? Number)?.toDouble() ?: 0.0,
        height = (this["height"] as? Number)?.toDouble() ?: 0.0,
    )

private fun Map<String, Any?>.toFlutterViewportHint(): FlutterViewportHint =
    FlutterViewportHint(
        kind =
            when (this["kind"] as? String) {
                "visible" -> "visible"
                "nearVisible" -> "nearVisible"
                "prefetchOnly" -> "prefetchOnly"
                else -> "hidden"
            },
        visibleFraction =
            ((this["visibleFraction"] as? Number)?.toDouble() ?: 0.0).coerceIn(0.0, 1.0),
    )

private fun Map<String, Any?>.toBufferingPolicy(): VesperBufferingPolicy =
    VesperBufferingPolicy(
        preset = when (this["preset"] as? String) {
            "balanced" -> VesperBufferingPreset.Balanced
            "streaming" -> VesperBufferingPreset.Streaming
            "resilient" -> VesperBufferingPreset.Resilient
            "lowLatency" -> VesperBufferingPreset.LowLatency
            else -> VesperBufferingPreset.Default
        },
        minBufferMs = (this["minBufferMs"] as? Number)?.toInt(),
        maxBufferMs = (this["maxBufferMs"] as? Number)?.toInt(),
        bufferForPlaybackMs = (this["bufferForPlaybackMs"] as? Number)?.toInt(),
        bufferForPlaybackAfterRebufferMs =
            (this["bufferForPlaybackAfterRebufferMs"] as? Number)?.toInt(),
    )

private fun Map<String, Any?>.toRetryPolicy(): VesperRetryPolicy =
    VesperRetryPolicy(
        maxAttempts =
            when {
                !containsKey("maxAttempts") -> 3
                this["maxAttempts"] == null -> null
                else -> (this["maxAttempts"] as? Number)?.toInt() ?: 3
            },
        baseDelayMs = (this["baseDelayMs"] as? Number)?.toLong(),
        maxDelayMs = (this["maxDelayMs"] as? Number)?.toLong(),
        backoff = when (this["backoff"] as? String) {
            "fixed" -> VesperRetryBackoff.Fixed
            "linear" -> VesperRetryBackoff.Linear
            "exponential" -> VesperRetryBackoff.Exponential
            else -> null
        },
    )

private fun Map<String, Any?>.toCachePolicy(): VesperCachePolicy =
    VesperCachePolicy(
        preset = when (this["preset"] as? String) {
            "disabled" -> VesperCachePreset.Disabled
            "streaming" -> VesperCachePreset.Streaming
            "resilient" -> VesperCachePreset.Resilient
            else -> VesperCachePreset.Default
        },
        maxMemoryBytes = (this["maxMemoryBytes"] as? Number)?.toLong(),
        maxDiskBytes = (this["maxDiskBytes"] as? Number)?.toLong(),
    )

private fun TimelineUiState.toMap(): Map<String, Any?> =
    mapOf(
        "kind" to kind.toWireName(),
        "isSeekable" to isSeekable,
        "seekableRange" to seekableRange?.let { range ->
            mapOf(
                "startMs" to range.startMs,
                "endMs" to range.endMs,
            )
        },
        "liveEdgeMs" to liveEdgeMs,
        "positionMs" to positionMs,
        "durationMs" to durationMs,
    )

private fun VesperTrackCatalog.toMap(): Map<String, Any?> =
    mapOf(
        "tracks" to tracks.map(VesperMediaTrack::toMap),
        "adaptiveVideo" to adaptiveVideo,
        "adaptiveAudio" to adaptiveAudio,
    )

private fun VesperMediaTrack.toMap(): Map<String, Any?> =
    mapOf(
        "id" to id,
        "kind" to kind.toWireName(),
        "label" to label,
        "language" to language,
        "codec" to codec,
        "bitRate" to bitRate,
        "width" to width,
        "height" to height,
        "frameRate" to frameRate?.toDouble(),
        "channels" to channels,
        "sampleRate" to sampleRate,
        "isDefault" to isDefault,
        "isForced" to isForced,
    )

private fun VesperTrackSelectionSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "video" to video.toMap(),
        "audio" to audio.toMap(),
        "subtitle" to subtitle.toMap(),
        "abrPolicy" to abrPolicy.toMap(),
    )

private fun VesperTrackSelection.toMap(): Map<String, Any?> =
    mapOf(
        "mode" to mode.toWireName(),
        "trackId" to trackId,
    )

private fun VesperAbrPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "mode" to mode.toWireName(),
        "trackId" to trackId,
        "maxBitRate" to maxBitRate,
        "maxWidth" to maxWidth,
        "maxHeight" to maxHeight,
    )

private fun VesperPlaybackResiliencePolicy.toMap(): Map<String, Any?> =
    mapOf(
        "buffering" to buffering.toMap(),
        "retry" to retry.toMap(),
        "cache" to cache.toMap(),
    )

private fun VesperBufferingPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "preset" to preset.toWireName(),
        "minBufferMs" to minBufferMs,
        "maxBufferMs" to maxBufferMs,
        "bufferForPlaybackMs" to bufferForPlaybackMs,
        "bufferForPlaybackAfterRebufferMs" to bufferForPlaybackAfterRebufferMs,
    )

private fun VesperRetryPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "maxAttempts" to maxAttempts,
        "baseDelayMs" to baseDelayMs,
        "maxDelayMs" to maxDelayMs,
        "backoff" to backoff.toWireName(),
    )

private fun VesperCachePolicy.toMap(): Map<String, Any?> =
    mapOf(
        "preset" to preset.toWireName(),
        "maxMemoryBytes" to maxMemoryBytes,
        "maxDiskBytes" to maxDiskBytes,
    )

private fun Throwable.toErrorMap(): Map<String, Any?> =
    mapOf(
        "message" to (message ?: toString()),
        "category" to "platform",
        "retriable" to false,
    )

private fun Throwable.toDownloadErrorMap(): Map<String, Any?> =
    mapOf(
        "codeOrdinal" to 0,
        "categoryOrdinal" to 0,
        "retriable" to false,
        "message" to (message ?: toString()),
    )

private fun PlaybackStateUi.toWireName(): String =
    when (this) {
        PlaybackStateUi.Ready -> "ready"
        PlaybackStateUi.Playing -> "playing"
        PlaybackStateUi.Paused -> "paused"
        PlaybackStateUi.Finished -> "finished"
    }

private fun TimelineKind.toWireName(): String =
    when (this) {
        TimelineKind.Vod -> "vod"
        TimelineKind.Live -> "live"
        TimelineKind.LiveDvr -> "liveDvr"
    }

private fun PlayerBridgeBackend.toBackendFamilyWireName(): String =
    when (this) {
        PlayerBridgeBackend.FakeDemo -> "fakeDemo"
        PlayerBridgeBackend.VesperNativeStub -> "androidHostKit"
    }

private fun VesperPlayerSourceKind.toWireName(): String =
    when (this) {
        VesperPlayerSourceKind.Local -> "local"
        VesperPlayerSourceKind.Remote -> "remote"
    }

private fun VesperPlayerSourceProtocol.toWireName(): String =
    when (this) {
        VesperPlayerSourceProtocol.Unknown -> "unknown"
        VesperPlayerSourceProtocol.File -> "file"
        VesperPlayerSourceProtocol.Content -> "content"
        VesperPlayerSourceProtocol.Progressive -> "progressive"
        VesperPlayerSourceProtocol.Hls -> "hls"
        VesperPlayerSourceProtocol.Dash -> "dash"
    }

private fun VesperBufferingPreset.toWireName(): String =
    when (this) {
        VesperBufferingPreset.Default -> "defaultPreset"
        VesperBufferingPreset.Balanced -> "balanced"
        VesperBufferingPreset.Streaming -> "streaming"
        VesperBufferingPreset.Resilient -> "resilient"
        VesperBufferingPreset.LowLatency -> "lowLatency"
    }

private fun VesperRetryBackoff.toWireName(): String =
    when (this) {
        VesperRetryBackoff.Fixed -> "fixed"
        VesperRetryBackoff.Linear -> "linear"
        VesperRetryBackoff.Exponential -> "exponential"
    }

private fun VesperCachePreset.toWireName(): String =
    when (this) {
        VesperCachePreset.Default -> "defaultPreset"
        VesperCachePreset.Disabled -> "disabled"
        VesperCachePreset.Streaming -> "streaming"
        VesperCachePreset.Resilient -> "resilient"
    }

private fun VesperMediaTrackKind.toWireName(): String =
    when (this) {
        VesperMediaTrackKind.Video -> "video"
        VesperMediaTrackKind.Audio -> "audio"
        VesperMediaTrackKind.Subtitle -> "subtitle"
    }

private fun VesperTrackSelectionMode.toWireName(): String =
    when (this) {
        VesperTrackSelectionMode.Auto -> "auto"
        VesperTrackSelectionMode.Disabled -> "disabled"
        VesperTrackSelectionMode.Track -> "track"
    }

private fun VesperAbrMode.toWireName(): String =
    when (this) {
        VesperAbrMode.Auto -> "auto"
        VesperAbrMode.Constrained -> "constrained"
        VesperAbrMode.FixedTrack -> "fixedTrack"
    }

private fun VesperPlayerSource.toMap(): Map<String, Any?> =
    mapOf(
        "uri" to uri,
        "label" to label,
        "kind" to kind.toWireName(),
        "protocol" to protocol.toWireName(),
    )

private fun VesperDownloadTaskSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "taskId" to taskId,
        "assetId" to assetId,
        "source" to source.toMap(),
        "profile" to profile.toMap(),
        "state" to state.toWireName(),
        "progress" to progress.toMap(),
        "assetIndex" to assetIndex.toMap(),
        "error" to error?.toMap(),
    )

private fun VesperDownloadSource.toMap(): Map<String, Any?> =
    mapOf(
        "source" to source.toMap(),
        "contentFormat" to contentFormat.toWireName(),
        "manifestUri" to manifestUri,
    )

private fun VesperDownloadProfile.toMap(): Map<String, Any?> =
    mapOf(
        "variantId" to variantId,
        "preferredAudioLanguage" to preferredAudioLanguage,
        "preferredSubtitleLanguage" to preferredSubtitleLanguage,
        "selectedTrackIds" to selectedTrackIds,
        "targetDirectory" to targetDirectory,
        "allowMeteredNetwork" to allowMeteredNetwork,
    )

private fun VesperDownloadProgressSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "receivedBytes" to receivedBytes,
        "totalBytes" to totalBytes,
        "receivedSegments" to receivedSegments,
        "totalSegments" to totalSegments,
    )

private fun VesperDownloadAssetIndex.toMap(): Map<String, Any?> =
    mapOf(
        "contentFormat" to contentFormat.toWireName(),
        "version" to version,
        "etag" to etag,
        "checksum" to checksum,
        "totalSizeBytes" to totalSizeBytes,
        "resources" to resources.map(VesperDownloadResourceRecord::toMap),
        "segments" to segments.map(VesperDownloadSegmentRecord::toMap),
        "completedPath" to completedPath,
    )

private fun VesperDownloadResourceRecord.toMap(): Map<String, Any?> =
    mapOf(
        "resourceId" to resourceId,
        "uri" to uri,
        "relativePath" to relativePath,
        "sizeBytes" to sizeBytes,
        "etag" to etag,
        "checksum" to checksum,
    )

private fun VesperDownloadSegmentRecord.toMap(): Map<String, Any?> =
    mapOf(
        "segmentId" to segmentId,
        "uri" to uri,
        "relativePath" to relativePath,
        "sequence" to sequence,
        "sizeBytes" to sizeBytes,
        "checksum" to checksum,
    )

private fun VesperDownloadError.toMap(): Map<String, Any?> =
    mapOf(
        "codeOrdinal" to codeOrdinal,
        "categoryOrdinal" to categoryOrdinal,
        "retriable" to retriable,
        "message" to message,
    )

private fun VesperDownloadState.toWireName(): String =
    when (this) {
        VesperDownloadState.Queued -> "queued"
        VesperDownloadState.Preparing -> "preparing"
        VesperDownloadState.Downloading -> "downloading"
        VesperDownloadState.Paused -> "paused"
        VesperDownloadState.Completed -> "completed"
        VesperDownloadState.Failed -> "failed"
        VesperDownloadState.Removed -> "removed"
    }

private fun VesperDownloadContentFormat.toWireName(): String =
    when (this) {
        VesperDownloadContentFormat.HlsSegments -> "hlsSegments"
        VesperDownloadContentFormat.DashSegments -> "dashSegments"
        VesperDownloadContentFormat.SingleFile -> "singleFile"
        VesperDownloadContentFormat.Unknown -> "unknown"
    }

private const val METHOD_CHANNEL_NAME = "io.github.ikaros.vesper_player"
private const val EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player/events"
private const val DOWNLOAD_EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player/download_events"
private const val PLAYER_VIEW_TYPE = "io.github.ikaros.vesper_player/platform_view"
