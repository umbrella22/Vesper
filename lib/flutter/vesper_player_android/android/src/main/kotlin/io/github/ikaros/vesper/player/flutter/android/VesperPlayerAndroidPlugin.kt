package io.github.ikaros.vesper.player.flutter.android

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.PluginRegistry
import io.flutter.plugin.common.StandardMessageCodec
import io.flutter.plugin.platform.PlatformView
import io.flutter.plugin.platform.PlatformViewFactory
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.VesperBackgroundPlaybackMode
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperAbrPolicy
import io.github.ikaros.vesper.player.android.VesperBenchmarkConfiguration
import io.github.ikaros.vesper.player.android.VesperBenchmarkEvent
import io.github.ikaros.vesper.player.android.VesperBenchmarkMetricSummary
import io.github.ikaros.vesper.player.android.VesperBenchmarkSummary
import io.github.ikaros.vesper.player.android.VesperBufferingPolicy
import io.github.ikaros.vesper.player.android.VesperBufferingPreset
import io.github.ikaros.vesper.player.android.VesperCachePolicy
import io.github.ikaros.vesper.player.android.VesperCachePreset
import io.github.ikaros.vesper.player.android.VesperDownloadAssetIndex
import io.github.ikaros.vesper.player.android.VesperDownloadAssetStream
import io.github.ikaros.vesper.player.android.VesperDownloadByteRange
import io.github.ikaros.vesper.player.android.VesperDownloadConfiguration
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadError
import io.github.ikaros.vesper.player.android.VesperDownloadEvent
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperDownloadOutputFormat
import io.github.ikaros.vesper.player.android.VesperDownloadProfile
import io.github.ikaros.vesper.player.android.VesperDownloadProgressSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadPublicCollection
import io.github.ikaros.vesper.player.android.VesperDownloadRecoveredTaskPlan
import io.github.ikaros.vesper.player.android.VesperDownloadResourceRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSegmentRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadStreamKind
import io.github.ikaros.vesper.player.android.VesperDownloadStaleResource
import io.github.ikaros.vesper.player.android.VesperDownloadStaleResourcePlanRecoverer
import io.github.ikaros.vesper.player.android.VesperDownloadTaskProgressPatch
import io.github.ikaros.vesper.player.android.VesperDownloadTaskStatePatch
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperMediaTrackKind
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerBackendFamily
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperRetryBackoff
import io.github.ikaros.vesper.player.android.VesperRetryPolicy
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackControlButton
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackControlKind
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackControls
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackConfiguration
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackPreferencePolicy
import io.github.ikaros.vesper.player.android.VesperPreloadBudgetPolicy
import io.github.ikaros.vesper.player.android.VesperTrackSelection
import io.github.ikaros.vesper.player.android.VesperTrackSelectionMode
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot
import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import java.io.File
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine

class VesperPlayerAndroidPlugin :
    PlatformViewFactory(StandardMessageCodec.INSTANCE),
    FlutterPlugin,
    MethodChannel.MethodCallHandler,
    EventChannel.StreamHandler,
    ActivityAware,
    PluginRegistry.RequestPermissionsResultListener {
    private lateinit var methodChannel: MethodChannel
    private lateinit var eventChannel: EventChannel
    private lateinit var downloadEventChannel: EventChannel
    private lateinit var applicationContext: Context

    private var eventSink: EventChannel.EventSink? = null
    private var downloadEventSink: EventChannel.EventSink? = null
    private var activityBinding: ActivityPluginBinding? = null
    private var activity: Activity? = null
    private var pendingSystemPlaybackPermissionResult: MethodChannel.Result? = null

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
                    downloadSessions.values.forEach { session ->
                        emitDownloadSnapshot(session)
                        emitDownloadRuntimeEvents(session)
                    }
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

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        activityBinding = binding
        activity = binding.activity
        binding.addRequestPermissionsResultListener(this)
    }

    override fun onDetachedFromActivityForConfigChanges() {
        activityBinding?.removeRequestPermissionsResultListener(this)
        activityBinding = null
        activity = null
        pendingSystemPlaybackPermissionResult?.success("denied")
        pendingSystemPlaybackPermissionResult = null
    }

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
        onAttachedToActivity(binding)
    }

    override fun onDetachedFromActivity() {
        activityBinding?.removeRequestPermissionsResultListener(this)
        activityBinding = null
        activity = null
        pendingSystemPlaybackPermissionResult?.success("denied")
        pendingSystemPlaybackPermissionResult = null
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ): Boolean {
        if (requestCode != NOTIFICATION_PERMISSION_REQUEST_CODE) {
            return false
        }
        val pending = pendingSystemPlaybackPermissionResult ?: return true
        pendingSystemPlaybackPermissionResult = null
        val granted = grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED
        pending.success(if (granted) "granted" else "denied")
        return true
    }

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
                emitDownloadRuntimeEvents(session)
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
            "setKeepScreenOnDuringPlayback" -> handleSessionCommand(call, result) { session ->
                val enabled = call.argumentMap()["enabled"] as? Boolean
                    ?: throw IllegalArgumentException("Missing enabled.")
                session.lastError = null
                session.controller.setKeepScreenOnDuringPlayback(enabled)
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
            "configureSystemPlayback" -> handleSessionCommand(call, result) { session ->
                val configurationMap = requireNestedMap(call.argumentMap(), "configuration")
                session.lastError = null
                session.controller.configureSystemPlayback(
                    configurationMap.toSystemPlaybackConfiguration(),
                )
                emitSnapshot(session)
                null
            }
            "updateSystemPlaybackMetadata" -> handleSessionCommand(call, result) { session ->
                val metadataMap = requireNestedMap(call.argumentMap(), "metadata")
                session.lastError = null
                session.controller.updateSystemPlaybackMetadata(
                    metadataMap.toSystemPlaybackMetadata(),
                )
                emitSnapshot(session)
                null
            }
            "clearSystemPlayback" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.controller.clearSystemPlayback()
                emitSnapshot(session)
                null
            }
            "requestSystemPlaybackPermissions" -> handleRequestSystemPlaybackPermissions(result)
            "getSystemPlaybackPermissionStatus" ->
                result.success(currentSystemPlaybackPermissionStatus())
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
            "shareDownloadTask" -> handleDownloadShareTask(call, result)
            "saveDownloadTask" -> handleDownloadSaveTask(call, result)
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

    private fun handleRequestSystemPlaybackPermissions(result: MethodChannel.Result) {
        when (val status = currentSystemPlaybackPermissionStatus()) {
            "notRequired", "granted" -> {
                result.success(status)
                return
            }
        }

        if (Build.VERSION.SDK_INT < 33) {
            result.success("notRequired")
            return
        }

        val currentActivity = activity
        if (currentActivity == null) {
            result.success("denied")
            return
        }
        if (pendingSystemPlaybackPermissionResult != null) {
            result.error(
                "vesper_permission_request_pending",
                "A system playback permission request is already in progress.",
                mapOf(
                    "message" to "A system playback permission request is already in progress.",
                    "code" to "backendFailure",
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
            return
        }

        pendingSystemPlaybackPermissionResult = result
        ActivityCompat.requestPermissions(
            currentActivity,
            arrayOf(Manifest.permission.POST_NOTIFICATIONS),
            NOTIFICATION_PERMISSION_REQUEST_CODE,
        )
    }

    private fun currentSystemPlaybackPermissionStatus(): String {
        if (Build.VERSION.SDK_INT < 33) {
            return "notRequired"
        }
        return if (
            ContextCompat.checkSelfPermission(
                applicationContext,
                Manifest.permission.POST_NOTIFICATIONS,
            ) == PackageManager.PERMISSION_GRANTED
        ) {
            "granted"
        } else {
            "denied"
        }
    }

    private fun handleCreatePlayer(call: MethodCall, result: MethodChannel.Result) {
        runCatching {
            val arguments = call.argumentMap()
            val initialSourceMap = arguments["initialSource"] as? Map<*, *>
            val resiliencePolicyMap = arguments["resiliencePolicy"] as? Map<*, *>
            val trackPreferencePolicyMap = arguments["trackPreferencePolicy"] as? Map<*, *>
            val preloadBudgetPolicyMap = arguments["preloadBudgetPolicy"] as? Map<*, *>
            val sourceNormalizerConfiguration =
                (arguments["sourceNormalizer"] as? Map<*, *>)
                    ?.stringMap()
                    .toSourceNormalizerConfiguration()
            val frameProcessorConfiguration =
                (arguments["frameProcessor"] as? Map<*, *>)
                    ?.stringMap()
                    .toFrameProcessorConfiguration()
            val benchmarkConfiguration =
                (arguments["benchmarkConfiguration"] as? Map<*, *>)
                    ?.stringMap()
                    ?.toBenchmarkConfiguration()
                    ?: VesperBenchmarkConfiguration.Disabled
            val surfaceKind = arguments["renderSurfaceKind"].toVesperVideoSurfaceKind()
            val keepScreenOnDuringPlayback =
                arguments["keepScreenOnDuringPlayback"] as? Boolean ?: true

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
                    keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
                    benchmarkConfiguration = benchmarkConfiguration,
                    surfaceKind = surfaceKind,
                    sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                    frameProcessorConfiguration = frameProcessorConfiguration,
                ),
                benchmarkConsoleLogging = benchmarkConfiguration.consoleLogging,
            )

            sessions[session.id] = session
            observeSession(session)

            mapOf(
                "playerId" to session.id,
                "snapshot" to buildSnapshotMap(session),
                "pluginDiagnostics" to session.controller.pluginDiagnostics,
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
            val downloadId = UUID.randomUUID().toString()
            val hasStaleResourceRecovery = arguments["hasStaleResourceRecovery"] as? Boolean ?: false
            val session =
                DownloadSession(
                    id = downloadId,
                    manager =
                        VesperDownloadManager(
                            context = applicationContext,
                            configuration = configurationMap.toDownloadConfiguration(),
                            staleResourcePlanRecoverer =
                                if (hasStaleResourceRecovery) {
                                    object : VesperDownloadStaleResourcePlanRecoverer {
                                        override suspend fun recoverPlan(
                                            task: VesperDownloadTaskSnapshot,
                                            staleResource: VesperDownloadStaleResource,
                                        ): VesperDownloadRecoveredTaskPlan? =
                                            recoverDownloadTaskPlan(downloadId, task, staleResource)
                                    }
                                } else {
                                    null
                                },
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

    private suspend fun recoverDownloadTaskPlan(
        downloadId: String,
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource,
    ): VesperDownloadRecoveredTaskPlan? =
        withContext(Dispatchers.Main) {
            suspendCoroutine { continuation ->
                methodChannel.invokeMethod(
                    "recoverDownloadTaskPlan",
                    mapOf(
                        "downloadId" to downloadId,
                        "task" to task.toMap(),
                        "staleResource" to staleResource.toMap(),
                    ),
                    object : MethodChannel.Result {
                        override fun success(result: Any?) {
                            val plan =
                                (result as? Map<*, *>)
                                    ?.entries
                                    ?.associate { (key, value) -> key.toString() to value }
                                    ?.toDownloadRecoveredTaskPlan()
                            continuation.resume(plan)
                        }

                        override fun error(
                            errorCode: String,
                            errorMessage: String?,
                            errorDetails: Any?,
                        ) {
                            continuation.resume(null)
                        }

                        override fun notImplemented() {
                            continuation.resume(null)
                        }
                    },
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
                mapOf(
                    "message" to "Missing playerId.",
                    "code" to "backendFailure",
                    "category" to "platform",
                    "retriable" to false,
                ),
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
                    "code" to "backendFailure",
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
                    "code" to "backendFailure",
                    "category" to "platform",
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
                    "code" to "backendFailure",
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
                    "code" to "backendFailure",
                    "category" to "platform",
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
                    "code" to "backendFailure",
                    "category" to "platform",
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
                            "code" to "backendFailure",
                            "category" to "platform",
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
                            "code" to "backendFailure",
                            "category" to "platform",
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

    private fun handleDownloadShareTask(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val resolved = resolveDownloadOutputRequest(call, result) ?: return
        runCatching {
            resolved.session.lastError = null
            resolved.session.manager.shareTaskOutput(
                context = activity ?: applicationContext,
                taskId = resolved.taskId,
                fileName = resolved.arguments["fileName"] as? String,
                mimeType = resolved.arguments["mimeType"] as? String,
            )
        }.onSuccess {
            result.success(null)
        }.onFailure { error ->
            resolved.session.lastError = error.toDownloadErrorMap()
            emitDownloadError(resolved.session, error)
            result.error(
                "vesper_download_operation_failed",
                error.message,
                resolved.session.lastError,
            )
        }
    }

    private fun handleDownloadSaveTask(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val resolved = resolveDownloadOutputRequest(call, result) ?: return
        runCatching {
            resolved.session.lastError = null
            resolved.session.manager.saveTaskOutput(
                context = applicationContext,
                taskId = resolved.taskId,
                fileName = resolved.arguments["fileName"] as? String,
                collection =
                    when (resolved.arguments["collection"] as? String) {
                        "movies" -> VesperDownloadPublicCollection.Movies
                        else -> VesperDownloadPublicCollection.Downloads
                    },
            ).toString()
        }.onSuccess(result::success)
            .onFailure { error ->
                resolved.session.lastError = error.toDownloadErrorMap()
                emitDownloadError(resolved.session, error)
                result.error(
                    "vesper_download_operation_failed",
                    error.message,
                    resolved.session.lastError,
                )
            }
    }

    private data class ResolvedDownloadOutputRequest(
        val session: DownloadSession,
        val taskId: Long,
        val arguments: Map<String, Any?>,
    )

    private fun resolveDownloadOutputRequest(
        call: MethodCall,
        result: MethodChannel.Result,
    ): ResolvedDownloadOutputRequest? {
        val arguments = call.argumentMap()
        val sessionId = arguments["downloadId"] as? String
        if (sessionId.isNullOrBlank()) {
            result.error(
                "vesper_missing_download_id",
                "Missing downloadId.",
                mapOf(
                    "message" to "Missing downloadId.",
                    "code" to "backendFailure",
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
            return null
        }
        val session = downloadSessions[sessionId]
        if (session == null) {
            result.error(
                "vesper_unknown_download",
                "Unknown downloadId: $sessionId",
                mapOf(
                    "message" to "Unknown downloadId: $sessionId",
                    "code" to "backendFailure",
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
            return null
        }
        val taskId = (arguments["taskId"] as? Number)?.toLong()
        if (taskId == null) {
            result.error(
                "vesper_missing_task_id",
                "Missing taskId.",
                mapOf(
                    "message" to "Missing taskId.",
                    "code" to "backendFailure",
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
            return null
        }
        return ResolvedDownloadOutputRequest(session, taskId, arguments)
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
                emitBenchmarkConsoleLog(session)
            }
        }
    }

    private fun observeDownloadSession(session: DownloadSession) {
        session.observerJob = scope.launch {
            session.manager.snapshot.collect {
                emitDownloadRuntimeEvents(session)
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
        emitBenchmarkConsoleLog(session)
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
        emitBenchmarkConsoleLog(session, force = true)
    }

    private fun emitDownloadSnapshot(session: DownloadSession) {
        downloadEventSink?.success(
            mapOf(
                "downloadId" to session.id,
                "type" to "initialSnapshot",
                "snapshot" to buildDownloadSnapshotMap(session),
            ),
        )
    }

    private fun emitDownloadRuntimeEvents(session: DownloadSession) {
        session.manager.drainEvents().forEach { event ->
            when (event) {
                is VesperDownloadEvent.Created -> {
                    downloadEventSink?.success(
                        mapOf(
                            "downloadId" to session.id,
                            "type" to "taskCreated",
                            "task" to event.task.toMap(),
                        ),
                    )
                }
                is VesperDownloadEvent.AssetIndexUpdated -> {
                    downloadEventSink?.success(
                        mapOf(
                            "downloadId" to session.id,
                            "type" to "taskUpdated",
                            "task" to event.task.toMap(),
                        ),
                    )
                }
                is VesperDownloadEvent.StateChanged -> {
                    if (event.patch.state == VesperDownloadState.Removed) {
                        downloadEventSink?.success(
                            mapOf(
                                "downloadId" to session.id,
                                "type" to "taskRemoved",
                                "taskId" to event.patch.taskId,
                            ),
                        )
                    } else {
                        downloadEventSink?.success(
                            mapOf(
                                "downloadId" to session.id,
                                "type" to "taskUpdated",
                                "patch" to event.patch.toMap(),
                            ),
                        )
                    }
                }
                is VesperDownloadEvent.ProgressUpdated -> {
                    downloadEventSink?.success(
                        mapOf(
                            "downloadId" to session.id,
                            "type" to "taskUpdated",
                            "progressPatch" to event.patch.toMap(),
                        ),
                    )
                }
            }
        }
    }

    private fun emitDownloadError(session: DownloadSession, error: Throwable) {
        downloadEventSink?.success(
            mapOf(
                "downloadId" to session.id,
                "type" to "downloadError",
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

    private fun emitBenchmarkConsoleLog(
        session: PlayerSession,
        force: Boolean = false,
    ) {
        if (!session.benchmarkConsoleLogging) {
            return
        }

        val events = session.controller.drainBenchmarkEvents()
        val summary = session.controller.benchmarkSummary()
        if (events.isEmpty() && summary.acceptedEvents == 0L) {
            return
        }
        if (events.isEmpty() && !force) {
            return
        }

        logBenchmarkJson(
            JSONObject()
                .put("playerId", session.id)
                .put("events", events.toBenchmarkJsonArray())
                .put("summary", summary.toBenchmarkJsonObject())
                .toString(),
        )
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
            "backendFamily" to session.controller.backendFamily.toBackendFamilyWireName(),
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
            "supportsDashStaticVod" to true,
            "supportsDashDynamicLive" to true,
            "supportsDashManifestTrackCatalog" to true,
            "supportsDashTextTracks" to true,
            "supportsTrackCatalog" to true,
            "supportsTrackSelection" to true,
            "supportsVideoTrackSelection" to true,
            "supportsAudioTrackSelection" to true,
            "supportsSubtitleTrackSelection" to true,
            "supportsAbrPolicy" to true,
            "supportsAbrConstrained" to true,
            "supportsAbrFixedTrack" to true,
            "supportsExactAbrFixedTrack" to true,
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
        session.cancelPendingHostDetach()
        session.advanceHostDetachGeneration()
        if (session.hostView === host) {
            session.controller.attachSurfaceHost(host)
            emitSnapshot(session)
            return
        }

        val previousHost = session.hostView
        session.hostView = host
        session.controller.attachSurfaceHost(host)
        previousHost?.removeAllViews()
        emitSnapshot(session)
    }

    private fun unbindSessionHost(playerId: String, host: FrameLayout) {
        val session = sessions[playerId] ?: return
        if (session.hostView !== host) {
            return
        }
        session.cancelPendingHostDetach()
        val generation = session.advanceHostDetachGeneration()
        session.pendingHostDetachJob = scope.launch {
            delay(HOST_DETACH_GRACE_DELAY_MS)
            val currentSession = sessions[playerId] ?: return@launch
            if (
                currentSession !== session ||
                currentSession.hostView !== host ||
                currentSession.hostDetachGeneration != generation
            ) {
                return@launch
            }
            currentSession.controller.detachSurfaceHost(host)
            currentSession.hostView = null
            currentSession.pendingHostDetachJob = null
            emitSnapshot(currentSession)
        }
    }

    private fun disposeSession(session: PlayerSession) {
        session.observerJob?.cancel()
        session.cancelPendingHostDetach()
        session.advanceHostDetachGeneration()
        session.hostView?.let(session.controller::detachSurfaceHost)
        session.hostView = null
        session.controller.dispose()
        emitBenchmarkConsoleLog(session, force = true)
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
