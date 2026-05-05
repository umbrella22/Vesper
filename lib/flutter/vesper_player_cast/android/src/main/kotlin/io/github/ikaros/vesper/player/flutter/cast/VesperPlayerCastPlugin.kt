package io.github.ikaros.vesper.player.flutter.cast

import android.content.Context
import android.view.View
import androidx.mediarouter.app.MediaRouteButton
import com.google.android.gms.cast.framework.CastButtonFactory
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.SessionManagerListener
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.StandardMessageCodec
import io.flutter.plugin.platform.PlatformView
import io.flutter.plugin.platform.PlatformViewFactory
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import io.github.ikaros.vesper.player.android.cast.VesperCastController
import io.github.ikaros.vesper.player.android.cast.VesperCastLoadRequest
import io.github.ikaros.vesper.player.android.cast.VesperCastOperationResult

class VesperPlayerCastPlugin :
    PlatformViewFactory(StandardMessageCodec.INSTANCE),
    FlutterPlugin,
    MethodChannel.MethodCallHandler,
    EventChannel.StreamHandler {
    private lateinit var applicationContext: Context
    private lateinit var methodChannel: MethodChannel
    private lateinit var eventChannel: EventChannel
    private lateinit var castController: VesperCastController
    private var eventSink: EventChannel.EventSink? = null
    private val sessionListener = VesperCastSessionListener { event ->
        eventSink?.success(event)
    }

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        applicationContext = binding.applicationContext
        castController = VesperCastController(applicationContext)
        methodChannel = MethodChannel(binding.binaryMessenger, METHOD_CHANNEL_NAME)
        eventChannel = EventChannel(binding.binaryMessenger, EVENT_CHANNEL_NAME)
        methodChannel.setMethodCallHandler(this)
        eventChannel.setStreamHandler(this)
        binding.platformViewRegistry.registerViewFactory(CAST_BUTTON_VIEW_TYPE, this)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        eventSink = null
        eventChannel.setStreamHandler(null)
        methodChannel.setMethodCallHandler(null)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        runCatching {
            when (call.method) {
                "isCastSessionAvailable" -> result.success(castController.isCastSessionAvailable())
                "loadRemoteMedia" -> result.success(
                    castController.load(call.argumentMap().toCastLoadRequest()).toMap(),
                )
                "play" -> result.success(castController.play().toMap())
                "pause" -> result.success(castController.pause().toMap())
                "stop" -> result.success(castController.stop().toMap())
                "seekTo" -> {
                    val positionMs = (call.argumentMap()["positionMs"] as? Number)?.toLong() ?: 0L
                    result.success(castController.seekTo(positionMs).toMap())
                }
                else -> result.notImplemented()
            }
        }.onFailure { error ->
            result.error(
                "vesper_cast_error",
                error.message ?: "Cast operation failed.",
                mapOf(
                    "message" to (error.message ?: "Cast operation failed."),
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
        }
    }

    override fun create(context: Context, viewId: Int, args: Any?): PlatformView {
        val button = MediaRouteButton(context)
        runCatching {
            CastButtonFactory.setUpMediaRouteButton(context.applicationContext, button)
        }
        return CastButtonPlatformView(button)
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
        eventSink = events
        runCatching {
            com.google.android.gms.cast.framework.CastContext
                .getSharedInstance(applicationContext)
                .sessionManager
                .addSessionManagerListener(sessionListener, CastSession::class.java)
        }
    }

    override fun onCancel(arguments: Any?) {
        runCatching {
            com.google.android.gms.cast.framework.CastContext
                .getSharedInstance(applicationContext)
                .sessionManager
                .removeSessionManagerListener(sessionListener, CastSession::class.java)
        }
        eventSink = null
    }
}

private class CastButtonPlatformView(private val button: MediaRouteButton) : PlatformView {
    override fun getView(): View = button

    override fun dispose() = Unit
}

private class VesperCastSessionListener(
    private val emit: (Map<String, Any?>) -> Unit,
) : SessionManagerListener<CastSession> {
    override fun onSessionStarted(session: CastSession, sessionId: String) {
        emit(session.toEvent("started"))
    }

    override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) {
        emit(session.toEvent("resumed"))
    }

    override fun onSessionEnded(session: CastSession, error: Int) {
        emit(session.toEvent("ended"))
    }

    override fun onSessionSuspended(session: CastSession, reason: Int) {
        emit(session.toEvent("suspended"))
    }

    override fun onSessionStarting(session: CastSession) = Unit
    override fun onSessionStartFailed(session: CastSession, error: Int) = Unit
    override fun onSessionEnding(session: CastSession) = Unit
    override fun onSessionResuming(session: CastSession, sessionId: String) = Unit
    override fun onSessionResumeFailed(session: CastSession, error: Int) = Unit
}

private fun CastSession.toEvent(kind: String): Map<String, Any?> =
    mapOf(
        "kind" to kind,
        "routeName" to castDevice?.friendlyName,
        "positionMs" to remoteMediaClient?.approximateStreamPosition,
    )

private fun VesperCastOperationResult.toMap(): Map<String, Any?> =
    when (this) {
        VesperCastOperationResult.Success -> mapOf("status" to "success")
        is VesperCastOperationResult.Unavailable ->
            mapOf("status" to "unavailable", "message" to message)
        is VesperCastOperationResult.Unsupported ->
            mapOf("status" to "unsupported", "message" to message)
    }

private fun Map<String, Any?>.toCastLoadRequest(): VesperCastLoadRequest {
    val sourceMap = requireNestedMap(this, "source")
    val metadataMap = (this["metadata"] as? Map<*, *>)?.stringMap()
    return VesperCastLoadRequest(
        source = sourceMap.toVesperPlayerSource(),
        metadata = metadataMap?.toSystemPlaybackMetadata(),
        startPositionMs = (this["startPositionMs"] as? Number)?.toLong() ?: 0L,
        autoplay = this["autoplay"] as? Boolean ?: true,
    )
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
        headers = this["headers"].stringStringMap(),
    )
}

private fun Map<String, Any?>.toSystemPlaybackMetadata(): VesperSystemPlaybackMetadata =
    VesperSystemPlaybackMetadata(
        title = this["title"] as? String ?: "",
        artist = this["artist"] as? String,
        albumTitle = this["albumTitle"] as? String,
        artworkUri = this["artworkUri"] as? String,
        contentUri = this["contentUri"] as? String,
        durationMs = (this["durationMs"] as? Number)?.toLong(),
        isLive = this["isLive"] as? Boolean ?: false,
    )

private fun MethodCall.argumentMap(): Map<String, Any?> =
    (arguments as? Map<*, *>)?.stringMap() ?: emptyMap()

private fun requireNestedMap(map: Map<String, Any?>, key: String): Map<String, Any?> =
    (map[key] as? Map<*, *>)?.stringMap()
        ?: throw IllegalArgumentException("Missing $key.")

private fun Map<*, *>.stringMap(): Map<String, Any?> =
    entries.associate { (key, value) -> key.toString() to value }

private fun Any?.stringStringMap(): Map<String, String> =
    (this as? Map<*, *>)
        ?.mapNotNull { (key, value) ->
            val stringKey = key?.toString() ?: return@mapNotNull null
            val stringValue = value?.toString() ?: return@mapNotNull null
            stringKey to stringValue
        }
        ?.toMap()
        ?: emptyMap()

private const val METHOD_CHANNEL_NAME = "io.github.ikaros.vesper_player_cast"
private const val EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player_cast/events"
private const val CAST_BUTTON_VIEW_TYPE = "io.github.ikaros.vesper_player_cast/cast_button"
