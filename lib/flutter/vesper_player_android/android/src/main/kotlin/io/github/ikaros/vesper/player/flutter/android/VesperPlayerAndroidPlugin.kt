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
import androidx.activity.ComponentActivity
import androidx.core.app.ActivityCompat
import androidx.core.app.PictureInPictureModeChangedInfo
import androidx.core.content.ContextCompat
import androidx.core.util.Consumer
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
import io.github.ikaros.vesper.player.android.VesperPictureInPictureError
import io.github.ikaros.vesper.player.android.VesperPictureInPictureErrorCode
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperMediaTrackKind
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityConfidence
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrKind
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeResult
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerBackendFamily
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperRetryBackoff
import io.github.ikaros.vesper.player.android.VesperRetryPolicy
import io.github.ikaros.vesper.player.android.VesperRecommendedPlaybackPath
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
import java.util.WeakHashMap
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONArray
import org.json.JSONObject

private const val PLAYER_SURFACE_TAG_PREFIX =
    "io.github.ikaros.vesper.player.surface."

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
    private var pictureInPictureModeChangedListener:
        Consumer<PictureInPictureModeChangedInfo>? = null
    private var pendingSystemPlaybackPermissionResult: MethodChannel.Result? = null

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val sessions = linkedMapOf<String, PlayerSession>()
    private val downloadSessions = linkedMapOf<String, DownloadSession>()
    private val surfaceHostLifecycle =
        SurfaceHostLifecycleCoordinator<PlayerSession, FrameLayout>(
            findSession = { playerId -> sessions[playerId] },
            getHost = { session -> session.hostView },
            setHost = { session, host -> session.hostView = host },
            cancelPendingDetach = { session -> session.cancelPendingHostDetach() },
            clearPendingDetach = { session -> session.clearPendingHostDetach() },
            advanceDetachGeneration = { session -> session.advanceHostDetachGeneration() },
            currentDetachGeneration = { session -> session.hostDetachGeneration },
            schedulePendingDetach = { session, generation, action ->
                session.pendingHostDetachJob = scope.launch {
                    delay(HOST_DETACH_GRACE_DELAY_MS)
                    action()
                }
            },
            attachHost = { session, host -> session.controller.attachSurfaceHost(host) },
            detachHost = { session, host -> session.controller.detachSurfaceHost(host) },
            clearHostView = { host -> host.removeAllViews() },
            emitSnapshot = { session -> emitSnapshot(session) },
        )

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
        registeredPlugins[binding.activity] = this
        binding.addRequestPermissionsResultListener(this)
        registerPictureInPictureModeChangedListener(binding.activity)
    }

    override fun onDetachedFromActivityForConfigChanges() {
        unregisterPictureInPictureModeChangedListener()
        emitPictureInPictureInactiveForDetachedActivity()
        activityBinding?.removeRequestPermissionsResultListener(this)
        activity?.let(registeredPlugins::remove)
        activityBinding = null
        activity = null
        pendingSystemPlaybackPermissionResult?.success("denied")
        pendingSystemPlaybackPermissionResult = null
    }

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
        onAttachedToActivity(binding)
    }

    override fun onDetachedFromActivity() {
        unregisterPictureInPictureModeChangedListener()
        emitPictureInPictureInactiveForDetachedActivity()
        activityBinding?.removeRequestPermissionsResultListener(this)
        activity?.let(registeredPlugins::remove)
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
            "probePlaybackCapability" -> handleProbePlaybackCapability(call, result)
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
            "initialize" -> handleSessionCommandAsync(call, result) { session ->
                session.lastError = null
                session.controller.initializeAsync()
                emitSnapshot(session)
                null
            }
            "selectSource" -> handleSessionCommandAsync(call, result) { session ->
                val sourceMap = requireNestedMap(call.argumentMap(), "source")
                session.lastError = null
                session.recentCapabilityProbe = null
                session.controller.selectSourceAsync(sourceMap.toVesperPlayerSource())
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
            "setSubtitleTrackSelection" -> handleSessionCommandAsync(call, result) { session ->
                val selectionMap = requireNestedMap(call.argumentMap(), "selection")
                session.lastError = null
                session.controller.setSubtitleTrackSelection(
                    selectionMap.toTrackSelection(isSubtitle = true),
                )
                emitSnapshot(session)
                null
            }
            "setSubtitleStyle" -> handleSessionCommand(call, result) { session ->
                val styleMap = requireNestedMap(call.argumentMap(), "style")
                session.lastError = null
                session.controller.setSubtitleStyle(styleMap.toVesperSubtitleStyle())
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
                null
            }
            "clearViewport" -> handleSessionCommand(call, result) { session ->
                session.lastError = null
                session.viewport = null
                session.viewportHint = FlutterViewportHint.hidden()
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
            "isPictureInPictureAvailable" -> handleSessionCommand(call, result) { session ->
                buildPictureInPictureAvailabilityMap(session)
            }
            "setPictureInPictureConfiguration" -> handleSessionCommand(call, result) { session ->
                val configurationMap =
                    (call.argumentMap()["configuration"] as? Map<*, *>)?.stringMap()
                session.pictureInPictureConfiguration =
                    configurationMap.toPictureInPictureConfiguration()
                applyPictureInPictureConfiguration(session)
                null
            }
            "requestPictureInPicture" -> handlePictureInPictureCommand(call, result) { session ->
                (call.argumentMap()["configuration"] as? Map<*, *>)
                    ?.stringMap()
                    ?.let { configurationMap ->
                        session.pictureInPictureConfiguration =
                            configurationMap.toPictureInPictureConfiguration()
                    }
                requestPictureInPicture(session)
            }
            "exitPictureInPicture" -> handlePictureInPictureCommand(call, result) { session ->
                exitPictureInPicture(session)
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
            host.tag = "$PLAYER_SURFACE_TAG_PREFIX$playerId"
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
        sessions.values.forEach { session ->
            session.lastEmittedSnapshot = null
        }
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

    private fun buildPictureInPictureAvailabilityMap(session: PlayerSession): Map<String, Any?> {
        val currentActivity = activity
        if (!session.pictureInPictureConfiguration.enabled) {
            val diagnostics =
                mapOf(
                    "platform" to "android",
                    "configurationEnabled" to false,
                    "sdkInt" to Build.VERSION.SDK_INT,
                )
            return mapOf(
                "isAvailable" to false,
                "isActive" to
                    (currentActivity?.isInPictureInPictureMode == true ||
                        session.pictureInPictureActive),
                "canAutoEnter" to false,
                "source" to "system",
                "error" to
                    VesperPictureInPictureError(
                        code =
                            VesperPictureInPictureErrorCode
                                .PictureInPictureDisabledByHost,
                        message = "Picture in Picture is disabled by host configuration.",
                        diagnostics = diagnostics,
                    ).toFlutterMap(),
                "diagnostics" to diagnostics,
            )
        }
        return session.controller.pictureInPictureReadiness().toFlutterMap(
            activity = currentActivity,
            platformSupportsPictureInPicture =
                currentActivity?.platformSupportsPictureInPicture()
                    ?: (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O),
            hostSupportsPictureInPicture =
                currentActivity?.runCatching { supportsPictureInPicture() }?.getOrDefault(false)
                    ?: false,
            isActive = currentActivity?.isInPictureInPictureMode == true ||
                session.pictureInPictureActive,
            canAutoEnter =
                session.pictureInPictureConfiguration.enabled &&
                    session.pictureInPictureConfiguration.autoEnter,
        )
    }

    private fun applyPictureInPictureConfiguration(session: PlayerSession) {
        val currentActivity = activity ?: return
        if (!currentActivity.platformSupportsPictureInPicture()) {
            return
        }
        if (!currentActivity.runCatching { supportsPictureInPicture() }.getOrDefault(false)) {
            return
        }
        runCatching {
            currentActivity.setPictureInPictureParams(session.buildPictureInPictureParams())
        }
    }

    private fun requestPictureInPicture(session: PlayerSession) {
        val currentActivity = activity
        val availability = buildPictureInPictureAvailabilityMap(session)
        if (availability["isAvailable"] != true || currentActivity == null) {
            val error =
                (availability["error"] as? Map<*, *>)
                    ?.stringMap()
                    ?.toPictureInPictureError()
                    ?: VesperPictureInPictureError(
                        code =
                            VesperPictureInPictureErrorCode
                                .PictureInPictureUnavailableForCurrentRoute,
                    )
            failPictureInPicture(
                session,
                error,
                (availability["diagnostics"] as? Map<*, *>)?.stringMap() ?: emptyMap(),
            )
            throw PictureInPictureRequestException(error)
        }

        session.pictureInPictureState = "entering"
        session.pictureInPictureActive = false
        emitPictureInPictureEvent(session)
        val entered =
            runCatching {
                currentActivity.enterPictureInPictureMode(session.buildPictureInPictureParams())
            }.getOrElse { error ->
                val pipError = error.toPictureInPictureRequestError()
                failPictureInPicture(session, pipError)
                throw PictureInPictureRequestException(pipError)
            }
        if (!entered && currentActivity.isInPictureInPictureMode != true) {
            val error =
                VesperPictureInPictureError(
                    code =
                        VesperPictureInPictureErrorCode
                            .PictureInPicturePlatformRequestRejected,
                    message = "Android rejected Picture in Picture request.",
            )
            failPictureInPicture(session, error)
            throw PictureInPictureRequestException(error)
        }
        if (currentActivity.isInPictureInPictureMode == true) {
            handlePictureInPictureModeChanged(true)
        }
    }

    private fun exitPictureInPicture(session: PlayerSession) {
        val currentActivity = activity
        if (currentActivity == null) {
            val error =
                VesperPictureInPictureError(
                    code =
                        VesperPictureInPictureErrorCode
                            .PictureInPicturePlatformRequestRejected,
                    message = "No Activity is attached for Picture in Picture restore.",
                )
            failPictureInPicture(session, error)
            throw PictureInPictureRequestException(error)
        }
        if (currentActivity.isInPictureInPictureMode != true && !session.pictureInPictureActive) {
            session.pictureInPictureState = "inactive"
            session.pictureInPictureActive = false
            emitPictureInPictureEvent(session)
            return
        }
        session.pictureInPictureState = "exiting"
        session.pictureInPictureActive = true
        emitPictureInPictureEvent(session)
        val restored =
            runCatching { currentActivity.requestPictureInPictureForegroundRestore() }
                .getOrElse { error ->
                    val pipError = error.toPictureInPictureRequestError()
                    failPictureInPicture(session, pipError)
                    throw PictureInPictureRequestException(pipError)
                }
        if (!restored) {
            val error =
                VesperPictureInPictureError(
                    code =
                        VesperPictureInPictureErrorCode
                            .PictureInPicturePlatformRequestRejected,
                    message = "Android rejected Picture in Picture foreground restore.",
                )
            failPictureInPicture(session, error)
            throw PictureInPictureRequestException(error)
        }
    }

    private fun failPictureInPicture(
        session: PlayerSession,
        error: VesperPictureInPictureError,
        diagnostics: Map<String, Any?> = emptyMap(),
    ) {
        session.pictureInPictureState = "failed"
        session.pictureInPictureActive = false
        emitPictureInPictureEvent(session, error, diagnostics)
    }

    private fun emitPictureInPictureInactiveForDetachedActivity() {
        sessions.values.forEach { session ->
            if (session.pictureInPictureActive || session.pictureInPictureState == "entering") {
                session.pictureInPictureActive = false
                session.pictureInPictureState = "inactive"
                emitPictureInPictureEvent(
                    session,
                    diagnostics = mapOf("reason" to "activityDetached"),
                )
            }
        }
    }

    private fun registerPictureInPictureModeChangedListener(currentActivity: Activity) {
        unregisterPictureInPictureModeChangedListener()
        val componentActivity = currentActivity as? ComponentActivity ?: return
        val listener =
            Consumer<PictureInPictureModeChangedInfo> { info ->
                handlePictureInPictureModeChanged(info.isInPictureInPictureMode)
            }
        componentActivity.addOnPictureInPictureModeChangedListener(listener)
        pictureInPictureModeChangedListener = listener
    }

    private fun unregisterPictureInPictureModeChangedListener() {
        val listener = pictureInPictureModeChangedListener ?: return
        (activity as? ComponentActivity)
            ?.removeOnPictureInPictureModeChangedListener(listener)
        pictureInPictureModeChangedListener = null
    }

    private fun handlePictureInPictureModeChanged(isInPictureInPictureMode: Boolean) {
        val targetSession =
            sessions.values.firstOrNull { session ->
                session.pictureInPictureState == "entering" ||
                    session.pictureInPictureState == "active" ||
                    session.pictureInPictureState == "exiting" ||
                    session.pictureInPictureActive
            } ?: return
        targetSession.pictureInPictureActive = isInPictureInPictureMode
        targetSession.pictureInPictureState =
            if (isInPictureInPictureMode) {
                "active"
            } else {
                "inactive"
            }
        emitPictureInPictureEvent(
            targetSession,
            diagnostics = mapOf("reason" to "activityModeChanged"),
        )
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
            val nativeFramePipelineConfiguration =
                (arguments["nativeFramePipeline"] as? Map<*, *>)
                    ?.stringMap()
                    .toNativeFramePipelineConfiguration()
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
                    nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
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

    private fun handleProbePlaybackCapability(call: MethodCall, result: MethodChannel.Result) {
        runCatching {
            val arguments = call.argumentMap()
            val request = arguments.toPlaybackCapabilityProbeRequest()
            val probeResult =
                VesperPlayerControllerFactory.probePlaybackCapability(applicationContext, request)
            (arguments["playerId"] as? String)
                ?.let(sessions::get)
                ?.let { session ->
                    session.recentCapabilityProbe = request.toSourceBoundProbe(probeResult)
                }
            emitCapabilityWarningIfNeeded(arguments["playerId"] as? String, probeResult)
            probeResult.toMap()
        }.fold(
            onSuccess = result::success,
            onFailure = { error -> result.error("invalid_probe_request", error.message, null) },
        )
    }

    private fun handleCreateDownloadManager(call: MethodCall, result: MethodChannel.Result) {
        val arguments = call.argumentMap()
        val configurationMap =
            runCatching { requireNestedMap(arguments, "configuration") }
                .getOrElse { error ->
                    result.error(
                        "vesper_download_create_failed",
                        error.message,
                        error.toDownloadErrorMap(),
                    )
                    return
                }
        val downloadId = UUID.randomUUID().toString()
        val hasStaleResourceRecovery = arguments["hasStaleResourceRecovery"] as? Boolean ?: false
        scope.launch {
            runCatching {
                val manager =
                    withContext(Dispatchers.IO) {
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
                        )
                    }
                val session =
                    DownloadSession(
                        id = downloadId,
                        manager = manager,
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
    }

    private suspend fun recoverDownloadTaskPlan(
        downloadId: String,
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource,
    ): VesperDownloadRecoveredTaskPlan? =
        withTimeoutOrNull(DOWNLOAD_RECOVERY_TIMEOUT_MS) {
            withContext(Dispatchers.Main) {
                val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
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
                            deferred.complete(plan)
                        }

                        override fun error(
                            errorCode: String,
                            errorMessage: String?,
                            errorDetails: Any?,
                        ) {
                            deferred.complete(null)
                        }

                        override fun notImplemented() {
                            deferred.complete(null)
                        }
                    },
                )
                deferred.await()
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

    private fun handleSessionCommandAsync(
        call: MethodCall,
        result: MethodChannel.Result,
        action: suspend (PlayerSession) -> Any?,
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

        scope.launch {
            runCatching {
                action(session)
            }.onSuccess { value ->
                if (!isCurrentSession(session)) {
                    result.success(null)
                    return@onSuccess
                }
                result.success(value)
                }
                .onFailure { error ->
                    routeAsyncSessionCommandFailure(
                        error = error,
                        isCurrentSession = isCurrentSession(session),
                        publishPlayerError = { eventErrorMap ->
                            session.lastError = eventErrorMap
                            emitError(session, error)
                        },
                        returnMethodError = { code, message, details ->
                            result.error(code, message, details)
                        },
                    )
                }
        }
    }

    private fun handlePictureInPictureCommand(
        call: MethodCall,
        result: MethodChannel.Result,
        action: (PlayerSession) -> Unit,
    ) {
        val sessionId = call.argumentMap()["playerId"] as? String
        if (sessionId.isNullOrBlank()) {
            result.error(
                "vesper_missing_player_id",
                "Missing playerId.",
                mapOf(
                    "message" to "Missing playerId.",
                    "code" to "pictureInPictureUnavailableForCurrentRoute",
                    "userMessage" to "Current playback cannot enter Picture in Picture.",
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
                    "code" to "pictureInPictureUnavailableForCurrentRoute",
                    "userMessage" to "Current playback cannot enter Picture in Picture.",
                ),
            )
            return
        }

        runCatching {
            action(session)
        }.onSuccess {
            result.success(null)
        }.onFailure { error ->
            val details = error.toPictureInPictureErrorMap()
            result.error(
                "vesper_picture_in_picture_failed",
                error.message,
                details,
            )
        }
    }

    companion object {
        private const val DOWNLOAD_RECOVERY_TIMEOUT_MS = 30_000L

        private val registeredPlugins =
            WeakHashMap<Activity, VesperPlayerAndroidPlugin>()

        @JvmStatic
        fun dispatchPictureInPictureModeChanged(
            activity: Activity,
            isInPictureInPictureMode: Boolean,
        ) {
            registeredPlugins[activity]
                ?.handlePictureInPictureModeChanged(isInPictureInPictureMode)
        }

        @JvmStatic
        fun dispatchPictureInPictureUserLeaveHint(activity: Activity) {
            registeredPlugins[activity]
                ?.handlePictureInPictureUserLeaveHint()
        }
    }

    private fun handlePictureInPictureUserLeaveHint() {
        val currentActivity = activity ?: return
        if (!currentActivity.platformSupportsPictureInPicture()) {
            return
        }
        val targetSession =
            sessions.values.firstOrNull { session ->
                session.pictureInPictureConfiguration.enabled &&
                    session.pictureInPictureConfiguration.autoEnter
            } ?: return
        if (targetSession.pictureInPictureActive ||
            targetSession.pictureInPictureState == "entering" ||
            targetSession.pictureInPictureState == "active" ||
            targetSession.pictureInPictureState == "exiting"
        ) {
            return
        }
        targetSession.pictureInPictureState = "entering"
        targetSession.pictureInPictureActive = false
        emitPictureInPictureEvent(
            targetSession,
            diagnostics = mapOf("reason" to "userLeaveHint"),
        )
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
        scope.launch {
            runCatching {
                resolved.session.lastError = null
                val context = activity ?: applicationContext
                val fileName = resolved.arguments["fileName"] as? String
                val mimeType = resolved.arguments["mimeType"] as? String
                val sharedFile =
                    withContext(Dispatchers.IO) {
                        resolved.session.manager.prepareTaskOutputForSharing(
                            context = context,
                            taskId = resolved.taskId,
                            fileName = fileName,
                        )
                    }
                resolved.session.manager.sharePreparedTaskOutput(
                    context = context,
                    sharedFile = sharedFile,
                    mimeType = mimeType,
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
    }

    private fun handleDownloadSaveTask(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val resolved = resolveDownloadOutputRequest(call, result) ?: return
        scope.launch {
            runCatching {
                resolved.session.lastError = null
                withContext(Dispatchers.IO) {
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
                }
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
        session.warningDrainJob?.cancel()
        // Runtime warnings have exactly one consumer: the snapshot observer
        // below. A separate drain job can consume warnings before the event
        // channel is listening (or race the observer), silently dropping them.
        session.warningDrainJob = null
        session.observerJob = scope.launch {
            val warningTicks = flow {
                while (currentCoroutineContext().isActive) {
                    emit(Unit)
                    delay(250L)
                }
            }
            val hostUpdates =
                combine(
                    session.controller.uiState,
                    session.controller.trackCatalog,
                    session.controller.trackSelection,
                    session.controller.subtitleState,
                ) { _, _, _, _ -> Unit }
            val subtitleSelectionUpdates =
                combine(
                    session.controller.requestedSubtitleSelection,
                    session.controller.confirmedSubtitleSelection,
                    session.controller.effectiveSubtitleTrackId,
                ) { _, _, _ -> Unit }
            combine(hostUpdates, subtitleSelectionUpdates, warningTicks) { _, _, _ ->
                // Drain runtime warnings once per snapshot tick and feed
                // them to the warning event stream. Subtitle state is now
                // first-class on the controller (read directly in
                // buildSnapshotMap), so warnings no longer need to
                // accumulate for state derivation. This fixes the
                // previous double-drain that starved the warning channel
                // and the unbounded accumulated list.
                // Keep warnings queued until an EventChannel listener exists;
                // draining while `eventSink` is null would make them
                // impossible to replay on the next listen.
                val drained =
                    if (eventSink == null) {
                        emptyList()
                    } else {
                        session.controller.drainRuntimeWarnings()
                    }
                buildSnapshotMap(session) to drained
            }.collect { (snapshot, drained) ->
                emitRuntimeWarnings(session, drained)
                emitHostTerminalErrorIfNeeded(session, snapshot)
                emitSnapshot(session, snapshot)
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
        emitSnapshot(session, buildSnapshotMap(session))
    }

    private fun emitSnapshot(
        session: PlayerSession,
        snapshot: Map<String, Any?>,
    ) {
        if (!isCurrentSession(session)) {
            return
        }
        val sink = eventSink
        if (sink == null) {
            emitBenchmarkConsoleLog(session)
            return
        }
        if (session.lastEmittedSnapshot == snapshot) {
            emitBenchmarkConsoleLog(session)
            return
        }
        session.lastEmittedSnapshot = snapshot
        sink.success(
            mapOf(
                "playerId" to session.id,
                "type" to "snapshot",
                "snapshot" to snapshot,
            ),
        )
        emitBenchmarkConsoleLog(session)
    }

    private fun isCurrentSession(session: PlayerSession): Boolean =
        sessions[session.id] === session

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

    private fun emitHostTerminalErrorIfNeeded(
        session: PlayerSession,
        snapshot: Map<String, Any?>,
    ) {
        val hostError = session.controller.uiState.value.lastError?.toMap()
        if (hostError == null) {
            session.lastEmittedTerminalError = null
            return
        }
        session.lastError = hostError
        if (session.lastEmittedTerminalError == hostError) {
            return
        }
        session.lastEmittedTerminalError = hostError
        emitEvent(
            mapOf(
                "playerId" to session.id,
                "type" to "error",
                "error" to hostError,
                "snapshot" to snapshot,
            ),
        )
        emitBenchmarkConsoleLog(session, force = true)
    }

    private fun emitPictureInPictureEvent(
        session: PlayerSession,
        error: VesperPictureInPictureError? = null,
        diagnostics: Map<String, Any?> = emptyMap(),
    ) {
        emitEvent(session.pictureInPictureEventMap(error = error, diagnostics = diagnostics))
    }

    private fun emitCapabilityWarningIfNeeded(
        playerId: String?,
        result: VesperPlaybackCapabilityProbeResult,
    ) {
        if (
            result.recommendedPlaybackPath != VesperRecommendedPlaybackPath.SystemPlayer ||
            result.hdrKind == VesperPlaybackCapabilityHdrKind.None
        ) {
            return
        }
        emitEvent(
            mapOf(
                "playerId" to (playerId ?: ""),
                "type" to "warning",
                "warning" to
                    mapOf(
                        "domain" to "capability",
                        "capability" to
                            mapOf(
                                "reason" to "hdrNativeFrameUnsupported",
                                "recommendedPlaybackPath" to "systemPlayer",
                                "hdrKind" to result.hdrKind.toWarningWireName(),
                                "likelyHdrCapabilityIssue" to true,
                                "confidence" to result.confidence.warningWireName,
                                "hdrMetadata" to result.toMap()["hdrMetadata"],
                                "message" to
                                    "HDR and Dolby Vision content uses system playback; SDK-managed native-frame presentation is SDR-only.",
                            ),
                    ),
            ),
        )
    }

    private fun emitRuntimeWarnings(
        session: PlayerSession,
        drained: List<io.github.ikaros.vesper.player.android.VesperRuntimeWarning> =
            session.controller.drainRuntimeWarnings(),
    ) {
        drained.forEach { warning ->
            val payload =
                if (warning.domain == "capability") {
                    warning.payload.withAppProbeConvergence(session.recentCapabilityProbe)
                } else {
                    warning.payload
                }
            emitEvent(
                mapOf(
                    "playerId" to session.id,
                    "type" to "warning",
                    "warning" to
                        mapOf(
                            "domain" to warning.domain,
                            warning.domain to payload,
                        ),
                ),
            )
        }
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
        val hostLastError = uiState.lastError?.toMap()
        if (hostLastError != null) {
            session.lastError = hostLastError
        }
        val resolvedLastError = hostLastError ?: session.lastError

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
            // Deprecated aliases are derived from the same immutable
            // snapshot so they cannot diverge from the canonical fields.
            "requestedSubtitleSelection" to trackSelection.subtitle.toMap(),
            "confirmedSubtitleSelection" to trackSelection.confirmedSubtitle.toMap(),
            "effectiveSubtitleTrackId" to trackSelection.effectiveSubtitleTrackId,
            "effectiveVideoTrackId" to effectiveVideoTrackId,
            "videoVariantObservation" to videoVariantObservation?.toMap(),
            "resiliencePolicy" to resiliencePolicy.toMap(),
            "pluginDiagnostics" to session.controller.pluginDiagnostics,
            "lastError" to resolvedLastError,
            // Read the first-class subtitle state directly from the
            // controller. This replaces the previous derive-from-catalog-
            // and-warnings logic that (a) could not produce `loading`,
            // (b) always set advertised == selectable, and (c) permanently
            // polluted the state across source switches.
            "subtitleState" to session.controller.subtitleState.value.toMap(),
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
        surfaceHostLifecycle.bind(playerId, host)
    }

    private fun unbindSessionHost(playerId: String, host: FrameLayout) {
        surfaceHostLifecycle.unbind(playerId, host)
    }

    private fun disposeSession(session: PlayerSession) {
        session.observerJob?.cancel()
        session.warningDrainJob?.cancel()
        surfaceHostLifecycle.detachSession(session)
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

private fun VesperPlaybackCapabilityHdrKind.toWarningWireName(): String =
    when (this) {
        VesperPlaybackCapabilityHdrKind.None -> "none"
        VesperPlaybackCapabilityHdrKind.Hdr10 -> "hdr10"
        VesperPlaybackCapabilityHdrKind.Hlg -> "hlg"
        VesperPlaybackCapabilityHdrKind.DolbyVision -> "dolbyVision"
        VesperPlaybackCapabilityHdrKind.Unknown -> "unknown"
    }

private val VesperPlaybackCapabilityConfidence.warningWireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityConfidence.CodecOnly -> "codecOnly"
            VesperPlaybackCapabilityConfidence.SourceMetadata -> "sourceMetadata"
            VesperPlaybackCapabilityConfidence.SessionProbe -> "sessionProbe"
        }
