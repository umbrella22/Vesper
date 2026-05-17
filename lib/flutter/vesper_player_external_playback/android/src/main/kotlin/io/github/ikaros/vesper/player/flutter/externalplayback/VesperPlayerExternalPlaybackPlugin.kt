package io.github.ikaros.vesper.player.flutter.externalplayback

import android.content.Context
import android.view.View
import androidx.mediarouter.app.MediaRouteButton
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
import io.github.ikaros.vesper.player.android.external.VesperExternalFallbackFormat
import io.github.ikaros.vesper.player.android.external.VesperExternalFormatAdaptationConfig
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackController
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackEvent
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackEventKind
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackMediaItem
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackResult
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRoute
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRouteKind
import io.github.ikaros.vesper.player.android.external.VesperExternalProxyPolicy
import io.github.ikaros.vesper.player.android.external.VesperExternalRouteButton
import io.github.ikaros.vesper.player.android.external.VesperExternalRouteButtonBrightness
import io.github.ikaros.vesper.player.android.external.internal.relay.DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

class VesperPlayerExternalPlaybackPlugin :
    PlatformViewFactory(StandardMessageCodec.INSTANCE),
    FlutterPlugin,
    MethodChannel.MethodCallHandler {
    private lateinit var applicationContext: Context
    private lateinit var methodChannel: MethodChannel
    private lateinit var routesEventChannel: EventChannel
    private lateinit var sessionEventChannel: EventChannel
    private lateinit var controller: VesperExternalPlaybackController
    private lateinit var scope: CoroutineScope

    private var routesSink: EventChannel.EventSink? = null
    private var sessionSink: EventChannel.EventSink? = null

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        applicationContext = binding.applicationContext
        controller = VesperExternalPlaybackController(applicationContext)
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        methodChannel = MethodChannel(binding.binaryMessenger, METHOD_CHANNEL_NAME)
        routesEventChannel = EventChannel(binding.binaryMessenger, ROUTES_EVENT_CHANNEL_NAME)
        sessionEventChannel = EventChannel(binding.binaryMessenger, SESSION_EVENT_CHANNEL_NAME)
        methodChannel.setMethodCallHandler(this)
        routesEventChannel.setStreamHandler(
            object : EventChannel.StreamHandler {
                override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
                    routesSink = events
                    emitRoutes(controller.routes.value)
                }

                override fun onCancel(arguments: Any?) {
                    routesSink = null
                }
            },
        )
        sessionEventChannel.setStreamHandler(
            object : EventChannel.StreamHandler {
                override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
                    sessionSink = events
                }

                override fun onCancel(arguments: Any?) {
                    sessionSink = null
                }
            },
        )
        scope.launch {
            controller.routes.collect { routes -> emitRoutes(routes) }
        }
        scope.launch {
            controller.events.collect { event -> sessionSink?.success(event.toMap()) }
        }
        binding.platformViewRegistry.registerViewFactory(ROUTE_BUTTON_VIEW_TYPE, this)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        scope.cancel()
        controller.release()
        routesSink = null
        sessionSink = null
        routesEventChannel.setStreamHandler(null)
        sessionEventChannel.setStreamHandler(null)
        methodChannel.setMethodCallHandler(null)
    }

    override fun create(context: Context, viewId: Int, args: Any?): PlatformView {
        val brightness = (args as? Map<*, *>)
            ?.get(ROUTE_BUTTON_BRIGHTNESS_KEY)
            ?.toString()
            ?.toRouteButtonBrightness()
        return RouteButtonPlatformView(VesperExternalRouteButton.create(context, brightness))
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        scope.launch {
            runCatching {
                when (call.method) {
                    "startDiscovery" -> {
                        controller.startDiscovery()
                        result.success(null)
                    }
                    "stopDiscovery" -> {
                        controller.stopDiscovery()
                        result.success(null)
                    }
                    "connect" -> {
                        val routeId = call.argumentMap()["routeId"] as? String
                            ?: return@launch result.success(
                                VesperExternalPlaybackResult.Failed("Missing routeId.").toMap(),
                            )
                        result.success(controller.connect(routeId).toMap())
                    }
                    "load" -> {
                        val arguments = call.argumentMap()
                        val item = requireNestedMap(arguments, "item").toMediaItem()
                        val startPositionMs = (arguments["startPositionMs"] as? Number)?.toLong() ?: 0L
                        val autoplay = arguments["autoplay"] as? Boolean ?: true
                        result.success(controller.loadAsync(item, startPositionMs, autoplay).toMap())
                    }
                    "play" -> result.success(controller.playAsync().toMap())
                    "pause" -> result.success(controller.pauseAsync().toMap())
                    "stop" -> result.success(controller.stopAsync().toMap())
                    "seekTo" -> {
                        val positionMs = (call.argumentMap()["positionMs"] as? Number)?.toLong() ?: 0L
                        result.success(controller.seekToAsync(positionMs).toMap())
                    }
                    "disconnect" -> result.success(controller.disconnectAsync().toMap())
                    else -> result.notImplemented()
                }
            }.onFailure { error ->
                val message = error.message ?: "External playback operation failed."
                result.error(
                    "vesper_external_playback_error",
                    message,
                    mapOf(
                        "message" to message,
                        "category" to "platform",
                        "exception" to error.javaClass.name,
                        "retriable" to false,
                    ),
                )
            }
        }
    }

    private fun emitRoutes(routes: List<VesperExternalPlaybackRoute>) {
        routesSink?.success(routes.map { route -> route.toMap() })
    }
}

private class RouteButtonPlatformView(private val button: MediaRouteButton) : PlatformView {
    override fun getView(): View = button

    override fun dispose() = Unit
}

private fun VesperExternalPlaybackRoute.toMap(): Map<String, Any?> =
    mapOf(
        "routeId" to routeId,
        "name" to name,
        "kind" to when (kind) {
            VesperExternalPlaybackRouteKind.Cast -> "cast"
            VesperExternalPlaybackRouteKind.Dlna -> "dlna"
        },
        "manufacturer" to manufacturer,
        "modelName" to modelName,
        "active" to active,
        "available" to available,
    )

private fun VesperExternalPlaybackEvent.toMap(): Map<String, Any?> =
    mapOf(
        "kind" to when (kind) {
            VesperExternalPlaybackEventKind.RouteConnected -> "routeConnected"
            VesperExternalPlaybackEventKind.RouteDisconnected -> "routeDisconnected"
            VesperExternalPlaybackEventKind.Loaded -> "loaded"
            VesperExternalPlaybackEventKind.Playing -> "playing"
            VesperExternalPlaybackEventKind.Paused -> "paused"
            VesperExternalPlaybackEventKind.Stopped -> "stopped"
            VesperExternalPlaybackEventKind.Suspended -> "suspended"
            VesperExternalPlaybackEventKind.Error -> "error"
            VesperExternalPlaybackEventKind.DiscoveryDiagnostic -> "discoveryDiagnostic"
        },
        "routeId" to routeId,
        "routeName" to routeName,
        "message" to message,
        "positionMs" to positionMs,
        "code" to code,
        "details" to details,
    )

private fun VesperExternalPlaybackResult.toMap(): Map<String, Any?> =
    when (this) {
        is VesperExternalPlaybackResult.Success -> mapOf(
            "status" to "success",
            "routeId" to routeId,
            "relayEnabled" to relayEnabled,
        )
        is VesperExternalPlaybackResult.Unavailable ->
            mapOf("status" to "unavailable", "message" to message)
        is VesperExternalPlaybackResult.Unsupported ->
            mapOf("status" to "unsupported", "message" to message)
        is VesperExternalPlaybackResult.Failed ->
            mapOf("status" to "failed", "message" to message)
    }

private fun Map<String, Any?>.toMediaItem(): VesperExternalPlaybackMediaItem {
    val rawSources = this["sources"] as? List<*> ?: emptyList<Any?>()
    val sources = rawSources
        .mapNotNull { (it as? Map<*, *>)?.stringMap()?.toVesperPlayerSource() }
    val metadata = (this["metadata"] as? Map<*, *>)?.stringMap()?.toSystemPlaybackMetadata()
        ?: VesperSystemPlaybackMetadata(title = "")
    val proxyPolicy = when (this["proxyPolicy"] as? String) {
        "always" -> VesperExternalProxyPolicy.Always
        "never" -> VesperExternalProxyPolicy.Never
        else -> VesperExternalProxyPolicy.Auto
    }
    val formatAdaptation =
        (this["formatAdaptation"] as? Map<*, *>)?.stringMap()?.toFormatAdaptationConfig()
            ?: VesperExternalFormatAdaptationConfig()
    return VesperExternalPlaybackMediaItem(sources, metadata, proxyPolicy, formatAdaptation)
}

private fun Map<String, Any?>.toFormatAdaptationConfig(): VesperExternalFormatAdaptationConfig =
    VesperExternalFormatAdaptationConfig(
        enabled = this["enabled"] as? Boolean ?: false,
        preferredFallback = when (this["preferredFallback"] as? String) {
            "hls" -> VesperExternalFallbackFormat.Hls
            else -> VesperExternalFallbackFormat.MpegTs
        },
        allowHls = this["allowHls"] as? Boolean ?: true,
        enableRangeCache = this["enableRangeCache"] as? Boolean ?: true,
        allowRemoteDashMediaReferences = this["allowRemoteDashMediaReferences"] as? Boolean ?: false,
        allowPrivateRemoteDashMediaAddresses = this["allowPrivateRemoteDashMediaAddresses"] as? Boolean ?: false,
        remoteDashMediaRequestHeaders =
            this["remoteDashMediaRequestHeaders"].stringSet(DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS),
        debugDiagnostics = this["debugDiagnostics"] as? Boolean ?: false,
    )

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

private fun Any?.stringSet(fallback: Set<String> = emptySet()): Set<String> =
    (this as? Iterable<*>)
        ?.mapNotNull { value -> value as? String }
        ?.filter(String::isNotBlank)
        ?.toSet()
        ?: fallback

private fun String.toRouteButtonBrightness(): VesperExternalRouteButtonBrightness? =
    when (this) {
        ROUTE_BUTTON_BRIGHTNESS_DARK -> VesperExternalRouteButtonBrightness.Dark
        ROUTE_BUTTON_BRIGHTNESS_LIGHT -> VesperExternalRouteButtonBrightness.Light
        else -> null
    }

private const val ROUTE_BUTTON_BRIGHTNESS_KEY = "brightness"
private const val ROUTE_BUTTON_BRIGHTNESS_DARK = "dark"
private const val ROUTE_BUTTON_BRIGHTNESS_LIGHT = "light"
private const val METHOD_CHANNEL_NAME = "io.github.ikaros.vesper_player_external_playback"
private const val ROUTES_EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player_external_playback/routes"
private const val SESSION_EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player_external_playback/events"
private const val ROUTE_BUTTON_VIEW_TYPE =
    "io.github.ikaros.vesper_player_external_playback/route_button"
