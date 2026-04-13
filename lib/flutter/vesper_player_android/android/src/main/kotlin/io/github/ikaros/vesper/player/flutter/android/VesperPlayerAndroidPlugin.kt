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
    private lateinit var applicationContext: Context

    private var eventSink: EventChannel.EventSink? = null

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val sessions = linkedMapOf<String, PlayerSession>()

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        applicationContext = binding.applicationContext
        methodChannel = MethodChannel(binding.binaryMessenger, METHOD_CHANNEL_NAME)
        eventChannel = EventChannel(binding.binaryMessenger, EVENT_CHANNEL_NAME)
        methodChannel.setMethodCallHandler(this)
        eventChannel.setStreamHandler(this)
        binding.platformViewRegistry.registerViewFactory(PLAYER_VIEW_TYPE, this)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        disposeAllSessions()
        eventSink = null
        eventChannel.setStreamHandler(null)
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
            "disposePlayer" -> handleSessionCommand(call, result) { session ->
                disposeSession(session)
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
                emitSnapshot(session)
                null
            }
            "clearViewport" -> handleSessionCommand(call, result) { session ->
                emitSnapshot(session)
                null
            }
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

            val session = PlayerSession(
                id = UUID.randomUUID().toString(),
                controller = VesperPlayerControllerFactory.createDefault(
                    context = applicationContext,
                    initialSource = initialSourceMap?.stringMap()?.toVesperPlayerSource(),
                    resiliencePolicy = resiliencePolicyMap?.stringMap()?.toResiliencePolicy()
                        ?: VesperPlaybackResiliencePolicy(),
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

    private fun emitEvent(payload: Map<String, Any?>) {
        eventSink?.success(payload)
    }

    private fun buildSnapshotMap(session: PlayerSession): Map<String, Any?> {
        val uiState = session.controller.uiState.value
        val trackCatalog = session.controller.trackCatalog.value
        val trackSelection = session.controller.trackSelection.value

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
            "backendFamily" to session.controller.backend.toBackendFamilyWireName(),
            "capabilities" to buildCapabilitiesMap(),
            "trackCatalog" to trackCatalog.toMap(),
            "trackSelection" to trackSelection.toMap(),
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
            "supportsAbrPolicy" to true,
            "supportsResiliencePolicy" to true,
            "supportsHolePunch" to false,
            "supportsPlaybackRate" to true,
            "supportsLiveEdgeSeeking" to true,
            "isExperimental" to false,
            "supportedPlaybackRates" to VesperPlayerController.supportedPlaybackRates
                .map { rate -> rate.toDouble() },
        )
    }

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

    private fun disposeAllSessions() {
        sessions.values.toList().forEach(::disposeSession)
        sessions.clear()
    }
}

private data class PlayerSession(
    val id: String,
    val controller: VesperPlayerController,
    var hostView: FrameLayout? = null,
    var observerJob: Job? = null,
    var lastError: Map<String, Any?>? = null,
) {
    fun hasAttachedHost(): Boolean = hostView != null
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
        maxAttempts = (this["maxAttempts"] as? Number)?.toInt(),
        baseDelayMs = (this["baseDelayMs"] as? Number)?.toLong() ?: 1_000L,
        maxDelayMs = (this["maxDelayMs"] as? Number)?.toLong() ?: 5_000L,
        backoff = when (this["backoff"] as? String) {
            "fixed" -> VesperRetryBackoff.Fixed
            "exponential" -> VesperRetryBackoff.Exponential
            else -> VesperRetryBackoff.Linear
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

private fun Throwable.toErrorMap(): Map<String, Any?> =
    mapOf(
        "message" to (message ?: toString()),
        "category" to "platform",
        "retriable" to false,
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

private const val METHOD_CHANNEL_NAME = "io.github.ikaros.vesper_player"
private const val EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player/events"
private const val PLAYER_VIEW_TYPE = "io.github.ikaros.vesper_player/platform_view"
