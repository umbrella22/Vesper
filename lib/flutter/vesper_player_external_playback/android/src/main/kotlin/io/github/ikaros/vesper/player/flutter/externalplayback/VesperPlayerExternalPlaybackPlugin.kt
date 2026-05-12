package io.github.ikaros.vesper.player.flutter.externalplayback

import android.content.Context
import android.content.res.Configuration
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.ContextThemeWrapper
import android.view.View
import androidx.mediarouter.app.MediaRouteButton
import androidx.mediarouter.app.MediaRouteChooserDialog
import androidx.mediarouter.app.MediaRouteChooserDialogFragment
import androidx.mediarouter.app.MediaRouteControllerDialog
import androidx.mediarouter.app.MediaRouteControllerDialogFragment
import androidx.mediarouter.app.MediaRouteDialogFactory
import androidx.mediarouter.app.MediaRouteDynamicChooserDialog
import androidx.mediarouter.app.MediaRouteDynamicControllerDialog
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
import io.github.ikaros.vesper.player.android.dlna.VesperDlnaDevice
import io.github.ikaros.vesper.player.android.dlna.VesperDlnaDiscovery
import io.github.ikaros.vesper.player.android.dlna.VesperDlnaDiscoveryDiagnostic
import io.github.ikaros.vesper.player.android.dlna.VesperDlnaOperationResult
import io.github.ikaros.vesper.player.android.dlna.VesperDlnaProtocolInfoParser
import io.github.ikaros.vesper.player.android.dlna.VesperDlnaSession
import io.github.ikaros.vesper.player.android.relay.VesperExternalPlaybackSourcePreparer
import io.github.ikaros.vesper.player.android.relay.VesperExternalPlaybackTarget
import io.github.ikaros.vesper.player.android.relay.VesperExternalProxyPolicy
import io.github.ikaros.vesper.player.android.relay.VesperExternalRouteCapabilities
import io.github.ikaros.vesper.player.android.relay.VesperExternalSourcePreparationRequest
import io.github.ikaros.vesper.player.android.relay.VesperExternalSourcePreparationResult
import io.github.ikaros.vesper.player.android.relay.VesperRelayServer

class VesperPlayerExternalPlaybackPlugin :
    PlatformViewFactory(StandardMessageCodec.INSTANCE),
    FlutterPlugin,
    MethodChannel.MethodCallHandler {
    private lateinit var applicationContext: Context
    private lateinit var methodChannel: MethodChannel
    private lateinit var routesEventChannel: EventChannel
    private lateinit var sessionEventChannel: EventChannel
    private lateinit var relayServer: VesperRelayServer
    private lateinit var sourcePreparer: VesperExternalPlaybackSourcePreparer
    private lateinit var castController: VesperCastController

    private var routesSink: EventChannel.EventSink? = null
    private var sessionSink: EventChannel.EventSink? = null
    private var dlnaDiscovery: VesperDlnaDiscovery? = null
    private val dlnaDevices = linkedMapOf<String, VesperDlnaDevice>()
    private val activeRelayTokens = mutableSetOf<String>()
    private var activeRouteId: String? = null
    private var activeCastRouteName: String? = null
    private var dlnaSession: VesperDlnaSession? = null
    private val mainHandler = Handler(Looper.getMainLooper())

    private val castSessionListener = VesperExternalCastSessionListener(
        onActive = { session ->
            activeRouteId = CAST_ROUTE_ID
            activeCastRouteName = session.castDevice?.friendlyName
            emitRoutes()
            emitSessionEvent(
                "routeConnected",
                CAST_ROUTE_ID,
                activeCastRouteName,
                positionMs = session.remoteMediaClient?.approximateStreamPosition,
            )
        },
        onEnded = { session ->
            if (activeRouteId == CAST_ROUTE_ID) {
                invalidateActiveRelay()
                activeRouteId = null
            }
            activeCastRouteName = session.castDevice?.friendlyName
            emitRoutes()
            emitSessionEvent(
                "routeDisconnected",
                CAST_ROUTE_ID,
                activeCastRouteName,
                positionMs = session.remoteMediaClient?.approximateStreamPosition,
            )
        },
        onSuspended = { session ->
            activeCastRouteName = session.castDevice?.friendlyName
            emitSessionEvent(
                "suspended",
                CAST_ROUTE_ID,
                activeCastRouteName,
                positionMs = session.remoteMediaClient?.approximateStreamPosition,
            )
        },
    )

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        applicationContext = binding.applicationContext
        relayServer = VesperRelayServer(applicationContext)
        sourcePreparer = VesperExternalPlaybackSourcePreparer(relayServer)
        castController = VesperCastController(applicationContext)
        methodChannel = MethodChannel(binding.binaryMessenger, METHOD_CHANNEL_NAME)
        routesEventChannel = EventChannel(binding.binaryMessenger, ROUTES_EVENT_CHANNEL_NAME)
        sessionEventChannel = EventChannel(binding.binaryMessenger, SESSION_EVENT_CHANNEL_NAME)
        methodChannel.setMethodCallHandler(this)
        routesEventChannel.setStreamHandler(
            object : EventChannel.StreamHandler {
                override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
                    routesSink = events
                    emitRoutes()
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
        binding.platformViewRegistry.registerViewFactory(ROUTE_BUTTON_VIEW_TYPE, this)
        runCatching {
            com.google.android.gms.cast.framework.CastContext
                .getSharedInstance(applicationContext)
                .sessionManager
                .addSessionManagerListener(castSessionListener, CastSession::class.java)
        }
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        runCatching {
            com.google.android.gms.cast.framework.CastContext
                .getSharedInstance(applicationContext)
                .sessionManager
                .removeSessionManagerListener(castSessionListener, CastSession::class.java)
        }
        dlnaDiscovery?.stop()
        dlnaDiscovery = null
        relayServer.stop()
        routesSink = null
        sessionSink = null
        routesEventChannel.setStreamHandler(null)
        sessionEventChannel.setStreamHandler(null)
        methodChannel.setMethodCallHandler(null)
    }

    @Suppress("DEPRECATION")
    override fun create(context: Context, viewId: Int, args: Any?): PlatformView {
        val routeTheme = routeTheme(context, args)
        val themedContext = ContextThemeWrapper(
            context,
            routeTheme.buttonTheme,
        )
        val button = MediaRouteButton(themedContext)
        runCatching {
            CastButtonFactory.setUpMediaRouteButton(themedContext, button)
        }
        button.dialogFactory = VesperRouteButtonDialogFactory(routeTheme.buttonTheme)
        button.setAlwaysVisible(true)
        return RouteButtonPlatformView(button)
    }

    private fun routeTheme(context: Context, args: Any?): RouteTheme {
        val brightness = (args as? Map<*, *>)?.get(ROUTE_BUTTON_BRIGHTNESS_KEY) as? String
        return when (brightness) {
            ROUTE_BUTTON_BRIGHTNESS_DARK -> RouteTheme.Dark
            ROUTE_BUTTON_BRIGHTNESS_LIGHT -> RouteTheme.Light
            else -> if (context.resources.configuration.isNightMode) {
                RouteTheme.Dark
            } else {
                RouteTheme.Light
            }
        }
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        runCatching {
            when (call.method) {
                "startDiscovery" -> {
                    startDiscovery()
                    result.success(null)
                }
                "stopDiscovery" -> {
                    stopDiscovery()
                    result.success(null)
                }
                "connect" -> result.success(connect(call.argumentMap()).toMap())
                "load" -> result.success(load(call.argumentMap()).toMap())
                "play" -> result.success(play().toMap())
                "pause" -> result.success(pause().toMap())
                "stop" -> result.success(stop().toMap())
                "seekTo" -> {
                    val positionMs = (call.argumentMap()["positionMs"] as? Number)?.toLong() ?: 0L
                    result.success(seekTo(positionMs).toMap())
                }
                "disconnect" -> result.success(disconnect().toMap())
                else -> result.notImplemented()
            }
        }.onFailure { error ->
            result.error(
                "vesper_external_playback_error",
                error.message ?: "External playback operation failed.",
                mapOf(
                    "message" to (error.message ?: "External playback operation failed."),
                    "category" to "platform",
                    "retriable" to false,
                ),
            )
        }
    }

    private fun startDiscovery() {
        if (dlnaDiscovery == null) {
            dlnaDiscovery = VesperDlnaDiscovery(
                applicationContext,
                object : VesperDlnaDiscovery.Listener {
                    override fun onRoutesChanged(routes: List<VesperDlnaDevice>) {
                        mainHandler.post {
                            dlnaDevices.clear()
                            routes.forEach { dlnaDevices[it.routeId] = it }
                            emitRoutes()
                        }
                    }

                    override fun onDiscoveryError(message: String) {
                        mainHandler.post {
                            emitSessionEvent("error", message = message)
                        }
                    }

                    override fun onDiscoveryDiagnostic(
                        diagnostic: VesperDlnaDiscoveryDiagnostic,
                    ) {
                        mainHandler.post {
                            emitSessionEvent(
                                "discoveryDiagnostic",
                                message = diagnostic.message,
                                code = diagnostic.code,
                                details = diagnostic.details + mapOf(
                                    "severity" to diagnostic.severity.name.lowercase(),
                                ),
                            )
                        }
                    }
                },
            )
        }
        dlnaDiscovery?.start()
        emitRoutes()
    }

    private fun stopDiscovery() {
        dlnaDiscovery?.stop()
        dlnaDiscovery = null
        dlnaDevices.clear()
        emitRoutes()
    }

    private fun connect(arguments: Map<String, Any?>): ExternalOperationResult {
        val routeId = arguments["routeId"] as? String
            ?: return ExternalOperationResult.Failed("Missing routeId.")
        if (routeId == CAST_ROUTE_ID) {
            return if (castController.isCastSessionAvailable()) {
                activeRouteId = CAST_ROUTE_ID
                emitRoutes()
                emitSessionEvent("routeConnected", CAST_ROUTE_ID, activeCastRouteName)
                ExternalOperationResult.Success(routeId = CAST_ROUTE_ID)
            } else {
                ExternalOperationResult.Unavailable("Select a Cast route with the system route button first.")
            }
        }
        val device = dlnaDevices[routeId]
            ?: return ExternalOperationResult.Unavailable("DLNA route is no longer available.")
        dlnaSession = VesperDlnaSession(device)
        activeRouteId = routeId
        emitRoutes()
        emitSessionEvent("routeConnected", routeId, device.friendlyName)
        return ExternalOperationResult.Success(routeId = routeId)
    }

    private fun load(arguments: Map<String, Any?>): ExternalOperationResult {
        val item = requireNestedMap(arguments, "item").toMediaItem()
        val startPositionMs = (arguments["startPositionMs"] as? Number)?.toLong() ?: 0L
        val autoplay = arguments["autoplay"] as? Boolean ?: true
        if (item.sources.isEmpty()) {
            return ExternalOperationResult.Unsupported("No media sources were provided.")
        }

        return when (activeRouteId) {
            CAST_ROUTE_ID -> loadCast(item, startPositionMs, autoplay)
            null -> ExternalOperationResult.Unavailable("No external playback route is connected.")
            else -> loadDlna(item, startPositionMs, autoplay)
        }
    }

    private fun loadCast(
        item: ExternalMediaItem,
        startPositionMs: Long,
        autoplay: Boolean,
    ): ExternalOperationResult {
        if (!castController.isCastSessionAvailable()) {
            return ExternalOperationResult.Unavailable("No active Cast session.")
        }
        val prepared = prepareSource(
            item = item,
            target = VesperExternalPlaybackTarget.Cast,
            capabilities = VesperExternalRouteCapabilities(
                supportsProgressive = true,
                supportsHls = true,
                supportsDash = true,
            ),
        ) ?: return lastPrepareFailure
        val castResult = castController.load(
            VesperCastLoadRequest(
                source = prepared.source,
                metadata = item.metadata,
                startPositionMs = startPositionMs,
                autoplay = autoplay,
            ),
        ).toExternalResult(CAST_ROUTE_ID, prepared.relayEnabled)
        if (castResult is ExternalOperationResult.Success) {
            prepared.relayToken?.let(activeRelayTokens::add)
            emitSessionEvent("loaded", CAST_ROUTE_ID, activeCastRouteName)
        }
        return castResult
    }

    private fun loadDlna(
        item: ExternalMediaItem,
        startPositionMs: Long,
        autoplay: Boolean,
    ): ExternalOperationResult {
        val session = dlnaSession
            ?: return ExternalOperationResult.Unavailable("No active DLNA session.")
        val protocolInfo = runCatching { session.protocolInfo() }.getOrDefault("")
        val prepared = prepareSource(
            item = item,
            target = VesperExternalPlaybackTarget.Dlna,
            capabilities = VesperExternalRouteCapabilities(
                supportsProgressive = true,
                supportsHls = VesperDlnaProtocolInfoParser.supportsHls(protocolInfo),
                supportsDash = false,
            ),
        ) ?: return lastPrepareFailure
        val dlnaResult = session.load(
            source = prepared.source,
            metadata = item.metadata,
            startPositionMs = startPositionMs,
            autoplay = autoplay,
        ).toExternalResult(session.device.routeId, prepared.relayEnabled)
        if (dlnaResult is ExternalOperationResult.Success) {
            prepared.relayToken?.let(activeRelayTokens::add)
            emitSessionEvent("loaded", session.device.routeId, session.device.friendlyName)
        }
        return dlnaResult
    }

    private var lastPrepareFailure: ExternalOperationResult =
        ExternalOperationResult.Unsupported("No playable external playback source is available.")

    private fun prepareSource(
        item: ExternalMediaItem,
        target: VesperExternalPlaybackTarget,
        capabilities: VesperExternalRouteCapabilities,
    ): VesperExternalSourcePreparationResult.Prepared? {
        return when (
            val prepared = sourcePreparer.prepare(
                VesperExternalSourcePreparationRequest(
                    target = target,
                    sources = item.sources,
                    proxyPolicy = item.proxyPolicy,
                    capabilities = capabilities,
                ),
            )
        ) {
            is VesperExternalSourcePreparationResult.Prepared -> prepared
            is VesperExternalSourcePreparationResult.Unsupported -> {
                lastPrepareFailure = ExternalOperationResult.Unsupported(prepared.message)
                null
            }
        }
    }

    private fun play(): ExternalOperationResult {
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.play().toExternalResult(CAST_ROUTE_ID)
            null -> ExternalOperationResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return ExternalOperationResult.Unavailable("No active DLNA session.")
                val result = session.play().toExternalResult(session.device.routeId)
                if (result is ExternalOperationResult.Success) {
                    emitSessionEvent("playing", session.device.routeId, session.device.friendlyName)
                }
                result
            }
        }
    }

    private fun pause(): ExternalOperationResult {
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.pause().toExternalResult(CAST_ROUTE_ID)
            null -> ExternalOperationResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return ExternalOperationResult.Unavailable("No active DLNA session.")
                val result = session.pause().toExternalResult(session.device.routeId)
                if (result is ExternalOperationResult.Success) {
                    emitSessionEvent("paused", session.device.routeId, session.device.friendlyName)
                }
                result
            }
        }
    }

    private fun stop(): ExternalOperationResult {
        val result = when (activeRouteId) {
            CAST_ROUTE_ID -> castController.stop().toExternalResult(CAST_ROUTE_ID)
            null -> ExternalOperationResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return ExternalOperationResult.Unavailable("No active DLNA session.")
                session.stop().toExternalResult(session.device.routeId)
            }
        }
        if (result is ExternalOperationResult.Success) {
            invalidateActiveRelay()
            emitSessionEvent("stopped", activeRouteId)
        }
        return result
    }

    private fun seekTo(positionMs: Long): ExternalOperationResult {
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.seekTo(positionMs).toExternalResult(CAST_ROUTE_ID)
            null -> ExternalOperationResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return ExternalOperationResult.Unavailable("No active DLNA session.")
                session.seekTo(positionMs).toExternalResult(session.device.routeId)
            }
        }
    }

    private fun disconnect(): ExternalOperationResult {
        val routeId = activeRouteId
        if (routeId != null) {
            runCatching {
                if (routeId == CAST_ROUTE_ID) {
                    castController.stop()
                } else {
                    dlnaSession?.stop()
                }
            }
        }
        invalidateActiveRelay()
        activeRouteId = null
        dlnaSession = null
        emitRoutes()
        emitSessionEvent("routeDisconnected", routeId)
        return ExternalOperationResult.Success(routeId = routeId)
    }

    private fun invalidateActiveRelay() {
        activeRelayTokens.forEach(relayServer::invalidate)
        activeRelayTokens.clear()
    }

    private fun emitRoutes() {
        val routes = mutableListOf<Map<String, Any?>>()
        if (castController.isCastSessionAvailable()) {
            routes += mapOf(
                "routeId" to CAST_ROUTE_ID,
                "name" to (activeCastRouteName ?: "Cast device"),
                "kind" to "cast",
                "active" to (activeRouteId == CAST_ROUTE_ID),
                "available" to true,
            )
        }
        routes += dlnaDevices.values.map { device ->
            mapOf(
                "routeId" to device.routeId,
                "name" to device.friendlyName,
                "kind" to "dlna",
                "manufacturer" to device.manufacturer,
                "modelName" to device.modelName,
                "active" to (activeRouteId == device.routeId),
                "available" to true,
            )
        }
        routesSink?.success(routes)
    }

    private fun emitSessionEvent(
        kind: String,
        routeId: String? = null,
        routeName: String? = null,
        message: String? = null,
        positionMs: Long? = null,
        code: String? = null,
        details: Map<String, String>? = null,
    ) {
        sessionSink?.success(
            mapOf(
                "kind" to kind,
                "routeId" to routeId,
                "routeName" to routeName,
                "message" to message,
                "positionMs" to positionMs,
                "code" to code,
                "details" to details,
            ),
        )
    }
}

private val Configuration.isNightMode: Boolean
    get() = uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES

private const val ROUTE_BUTTON_BRIGHTNESS_KEY = "brightness"
private const val ROUTE_BUTTON_BRIGHTNESS_DARK = "dark"
private const val ROUTE_BUTTON_BRIGHTNESS_LIGHT = "light"
private const val ROUTE_DIALOG_THEME_ARGUMENT = "routeDialogTheme"

private data class RouteTheme(
    val buttonTheme: Int,
) {
    companion object {
        val Light = RouteTheme(
            R.style.VesperPlayerExternalRouteButtonTheme_Light,
        )
        val Dark = RouteTheme(
            R.style.VesperPlayerExternalRouteButtonTheme_Dark,
        )
    }
}

private class VesperRouteButtonDialogFactory(
    private val routeDialogTheme: Int,
) : MediaRouteDialogFactory() {
    override fun onCreateChooserDialogFragment(): MediaRouteChooserDialogFragment =
        VesperRouteChooserDialogFragment.newInstance(routeDialogTheme)

    override fun onCreateControllerDialogFragment(): MediaRouteControllerDialogFragment =
        VesperRouteControllerDialogFragment.newInstance(routeDialogTheme)
}

class VesperRouteChooserDialogFragment : MediaRouteChooserDialogFragment() {
    override fun onCreateChooserDialog(
        context: Context,
        savedInstanceState: Bundle?,
    ): MediaRouteChooserDialog =
        MediaRouteChooserDialog(routeContext(context), routeDialogTheme())

    override fun onCreateDynamicChooserDialog(context: Context): MediaRouteDynamicChooserDialog =
        MediaRouteDynamicChooserDialog(routeContext(context), routeDialogTheme())

    private fun routeContext(context: Context): Context =
        ContextThemeWrapper(context, routeDialogTheme())

    private fun routeDialogTheme(): Int =
        arguments?.getInt(ROUTE_DIALOG_THEME_ARGUMENT, 0)
            ?.takeIf { it != 0 }
            ?: R.style.VesperPlayerExternalRouteButtonTheme_Light

    companion object {
        fun newInstance(routeDialogTheme: Int): VesperRouteChooserDialogFragment =
            VesperRouteChooserDialogFragment().apply {
                arguments = Bundle().apply {
                    putInt(ROUTE_DIALOG_THEME_ARGUMENT, routeDialogTheme)
                }
            }
    }
}

class VesperRouteControllerDialogFragment : MediaRouteControllerDialogFragment() {
    override fun onCreateControllerDialog(
        context: Context,
        savedInstanceState: Bundle?,
    ): MediaRouteControllerDialog =
        MediaRouteControllerDialog(routeContext(context), routeDialogTheme())

    override fun onCreateDynamicControllerDialog(context: Context): MediaRouteDynamicControllerDialog =
        MediaRouteDynamicControllerDialog(routeContext(context), routeDialogTheme())

    private fun routeContext(context: Context): Context =
        ContextThemeWrapper(context, routeDialogTheme())

    private fun routeDialogTheme(): Int =
        arguments?.getInt(ROUTE_DIALOG_THEME_ARGUMENT, 0)
            ?.takeIf { it != 0 }
            ?: R.style.VesperPlayerExternalRouteButtonTheme_Light

    companion object {
        fun newInstance(routeDialogTheme: Int): VesperRouteControllerDialogFragment =
            VesperRouteControllerDialogFragment().apply {
                arguments = Bundle().apply {
                    putInt(ROUTE_DIALOG_THEME_ARGUMENT, routeDialogTheme)
                }
            }
    }
}

private class RouteButtonPlatformView(private val button: MediaRouteButton) : PlatformView {
    override fun getView(): View = button

    override fun dispose() = Unit
}

private class VesperExternalCastSessionListener(
    private val onActive: (CastSession) -> Unit,
    private val onEnded: (CastSession) -> Unit,
    private val onSuspended: (CastSession) -> Unit,
) : SessionManagerListener<CastSession> {
    override fun onSessionStarted(session: CastSession, sessionId: String) = onActive(session)
    override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) = onActive(session)
    override fun onSessionEnded(session: CastSession, error: Int) = onEnded(session)
    override fun onSessionSuspended(session: CastSession, reason: Int) = onSuspended(session)
    override fun onSessionStarting(session: CastSession) = Unit
    override fun onSessionStartFailed(session: CastSession, error: Int) = Unit
    override fun onSessionEnding(session: CastSession) = Unit
    override fun onSessionResuming(session: CastSession, sessionId: String) = Unit
    override fun onSessionResumeFailed(session: CastSession, error: Int) = Unit
}

private data class ExternalMediaItem(
    val sources: List<VesperPlayerSource>,
    val metadata: VesperSystemPlaybackMetadata,
    val proxyPolicy: VesperExternalProxyPolicy,
)

private sealed class ExternalOperationResult {
    data class Success(
        val routeId: String? = null,
        val relayEnabled: Boolean = false,
    ) : ExternalOperationResult()

    data class Unavailable(val message: String) : ExternalOperationResult()
    data class Unsupported(val message: String) : ExternalOperationResult()
    data class Failed(val message: String) : ExternalOperationResult()
}

private fun ExternalOperationResult.toMap(): Map<String, Any?> =
    when (this) {
        is ExternalOperationResult.Success -> mapOf(
            "status" to "success",
            "routeId" to routeId,
            "relayEnabled" to relayEnabled,
        )
        is ExternalOperationResult.Unavailable ->
            mapOf("status" to "unavailable", "message" to message)
        is ExternalOperationResult.Unsupported ->
            mapOf("status" to "unsupported", "message" to message)
        is ExternalOperationResult.Failed ->
            mapOf("status" to "failed", "message" to message)
    }

private fun VesperCastOperationResult.toExternalResult(
    routeId: String? = null,
    relayEnabled: Boolean = false,
): ExternalOperationResult =
    when (this) {
        VesperCastOperationResult.Success -> ExternalOperationResult.Success(routeId, relayEnabled)
        is VesperCastOperationResult.Unavailable -> ExternalOperationResult.Unavailable(message)
        is VesperCastOperationResult.Unsupported -> ExternalOperationResult.Unsupported(message)
    }

private fun VesperDlnaOperationResult.toExternalResult(
    routeId: String? = null,
    relayEnabled: Boolean = false,
): ExternalOperationResult =
    when (this) {
        VesperDlnaOperationResult.Success -> ExternalOperationResult.Success(routeId, relayEnabled)
        is VesperDlnaOperationResult.Unavailable -> ExternalOperationResult.Unavailable(message)
        is VesperDlnaOperationResult.Unsupported -> ExternalOperationResult.Unsupported(message)
        is VesperDlnaOperationResult.Failed -> ExternalOperationResult.Failed(message)
    }

private fun Map<String, Any?>.toMediaItem(): ExternalMediaItem {
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
    return ExternalMediaItem(sources, metadata, proxyPolicy)
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

private const val CAST_ROUTE_ID = "cast:active"
private const val METHOD_CHANNEL_NAME = "io.github.ikaros.vesper_player_external_playback"
private const val ROUTES_EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player_external_playback/routes"
private const val SESSION_EVENT_CHANNEL_NAME = "io.github.ikaros.vesper_player_external_playback/events"
private const val ROUTE_BUTTON_VIEW_TYPE =
    "io.github.ikaros.vesper_player_external_playback/route_button"
