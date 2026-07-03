package io.github.ikaros.vesper.player.android.external

import android.content.Context
import android.os.Handler
import android.os.Looper
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastSession
import io.github.ikaros.vesper.player.android.drmUnsupportedRouteDetails
import io.github.ikaros.vesper.player.android.drmUnsupportedRouteMessage
import io.github.ikaros.vesper.player.android.external.internal.cast.VesperCastController
import io.github.ikaros.vesper.player.android.external.internal.cast.VesperCastLoadRequest
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDevice
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDiscovery
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDiscoveryDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaSession
import io.github.ikaros.vesper.player.android.external.internal.dlna.matchesRouteId
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalPlaybackSourcePreparer
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayServer
import io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg.VesperRelayFfmpegAdapter
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

class VesperExternalPlaybackController(context: Context) {
    internal val applicationContext = context.applicationContext
    internal val mainHandler = Handler(Looper.getMainLooper())
    internal val relayServer = VesperRelayServer(
        applicationContext,
        formatAdapter = VesperRelayFfmpegAdapter(applicationContext),
        diagnosticListener = { diagnostic -> emitRelayDiagnostic(diagnostic) },
    )
    internal val sourcePreparer = VesperExternalPlaybackSourcePreparer(relayServer)
    internal val castController = VesperCastController(applicationContext)
    internal val dlnaDevices = linkedMapOf<String, VesperDlnaDevice>()
    internal val recentlySeenDlnaDevices = linkedMapOf<String, RecentDlnaDevice>()
    internal val activeRelayTokens = mutableSetOf<String>()
    internal var discoveryGeneration = 0
    internal var activeRouteId: String? = null
    internal var activeCastRouteName: String? = null
    internal var dlnaDiscovery: VesperDlnaDiscovery? = null
    internal var dlnaSession: VesperDlnaSession? = null
    @Volatile
    internal var released = false
    internal val castContextExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "vesper-cast-context").apply { isDaemon = true }
    }

    internal val _routes = MutableStateFlow<List<VesperExternalPlaybackRoute>>(emptyList())
    val routes: StateFlow<List<VesperExternalPlaybackRoute>> = _routes.asStateFlow()

    internal val _events = MutableSharedFlow<VesperExternalPlaybackEvent>(extraBufferCapacity = 64)
    val events: SharedFlow<VesperExternalPlaybackEvent> = _events.asSharedFlow()

    private val castSessionListener = VesperExternalCastSessionListener(
        onActive = { session ->
            activeRouteId = CAST_ROUTE_ID
            activeCastRouteName = session.castDevice?.friendlyName
            emitRoutes()
            emitEvent(
                VesperExternalPlaybackEventKind.RouteConnected,
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
            emitEvent(
                VesperExternalPlaybackEventKind.RouteDisconnected,
                CAST_ROUTE_ID,
                activeCastRouteName,
                positionMs = session.remoteMediaClient?.approximateStreamPosition,
            )
        },
        onSuspended = { session ->
            activeCastRouteName = session.castDevice?.friendlyName
            emitEvent(
                VesperExternalPlaybackEventKind.Suspended,
                CAST_ROUTE_ID,
                activeCastRouteName,
                positionMs = session.remoteMediaClient?.approximateStreamPosition,
            )
        },
    )

    init {
        prepareCastContextAsync(
            onSuccess = {
                sessionManager.addSessionManagerListener(castSessionListener, CastSession::class.java)
            },
        )
        emitRoutes()
    }

    fun prepareCastAsync(onComplete: (Boolean, String?) -> Unit = { _, _ -> }) {
        checkNotReleased()
        prepareCastContextAsync(
            onSuccess = {
                emitRoutes()
                mainHandler.post { onComplete(true, null) }
            },
            onFailure = { error ->
                val message = error.message ?: "Cast route selection is not available."
                emitEvent(VesperExternalPlaybackEventKind.Error, message = message)
                mainHandler.post { onComplete(false, message) }
            },
        )
    }

    fun startDiscovery() {
        checkNotReleased()
        if (dlnaDiscovery == null) {
            val generation = ++discoveryGeneration
            dlnaDiscovery = VesperDlnaDiscovery(
                applicationContext,
                object : VesperDlnaDiscovery.Listener {
                    override fun onRoutesChanged(routes: List<VesperDlnaDevice>) {
                        mainHandler.post {
                            if (generation != discoveryGeneration || released) {
                                return@post
                            }
                            pruneRecentlySeenDlnaDevices()
                            dlnaDevices.clear()
                            routes.forEach { device ->
                                dlnaDevices[device.routeId] = device
                                recentlySeenDlnaDevices[device.routeId] = RecentDlnaDevice(
                                    device = device,
                                    expiresAtMillis = System.currentTimeMillis() + RECENT_DLNA_ROUTE_GRACE_MS,
                                )
                            }
                            emitRoutes()
                        }
                    }

                    override fun onDiscoveryError(message: String) {
                        mainHandler.post {
                            if (generation == discoveryGeneration && !released) {
                                emitEvent(VesperExternalPlaybackEventKind.Error, message = message)
                            }
                        }
                    }

                    override fun onDiscoveryDiagnostic(diagnostic: VesperDlnaDiscoveryDiagnostic) {
                        mainHandler.post {
                            if (generation != discoveryGeneration || released) {
                                return@post
                            }
                            emitEvent(
                                VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
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

    fun stopDiscovery() {
        discoveryGeneration += 1
        dlnaDiscovery?.stop()
        dlnaDiscovery = null
        dlnaDevices.clear()
        pruneRecentlySeenDlnaDevices()
        emitRoutes()
    }

    fun connect(routeId: String): VesperExternalPlaybackResult {
        checkNotReleased()
        if (routeId == CAST_ROUTE_ID) {
            return if (castController.isCastSessionAvailable()) {
                activeRouteId = CAST_ROUTE_ID
                emitRoutes()
                emitEvent(VesperExternalPlaybackEventKind.RouteConnected, CAST_ROUTE_ID, activeCastRouteName)
                VesperExternalPlaybackResult.Success(routeId = CAST_ROUTE_ID)
            } else {
                VesperExternalPlaybackResult.Unavailable("Select a Cast route with the system route button first.")
            }
        }

        val device = findDlnaDevice(routeId)
        if (device == null) {
            emitEvent(
                VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
                message = "DLNA route is no longer available.",
                code = "dlna_route_cache_miss",
                details = dlnaRouteCacheMissDetails(routeId),
            )
            return VesperExternalPlaybackResult.Unavailable("DLNA route is no longer available.")
        }
        dlnaSession = VesperDlnaSession(device)
        activeRouteId = device.routeId
        emitRoutes()
        emitEvent(VesperExternalPlaybackEventKind.RouteConnected, device.routeId, device.friendlyName)
        return VesperExternalPlaybackResult.Success(routeId = device.routeId)
    }

    fun load(
        item: VesperExternalPlaybackMediaItem,
        startPositionMs: Long = 0,
        autoplay: Boolean = true,
    ): VesperExternalPlaybackResult {
        checkNotReleased()
        if (item.sources.isEmpty()) {
            return VesperExternalPlaybackResult.Unsupported("No media sources were provided.")
        }
        item.sources.firstOrNull { it.drmConfiguration != null }?.let { source ->
            return VesperExternalPlaybackResult.Unsupported(
                drmUnsupportedRouteMessage("externalPlayback"),
                details = drmUnsupportedRouteDetails(source, route = "externalPlayback"),
            )
        }
        return when (activeRouteId) {
            CAST_ROUTE_ID -> loadCast(item, startPositionMs, autoplay)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> loadDlna(item, startPositionMs, autoplay)
        }
    }

    suspend fun loadAsync(
        item: VesperExternalPlaybackMediaItem,
        startPositionMs: Long = 0,
        autoplay: Boolean = true,
    ): VesperExternalPlaybackResult {
        checkNotReleased()
        if (item.sources.isEmpty()) {
            return VesperExternalPlaybackResult.Unsupported("No media sources were provided.")
        }
        item.sources.firstOrNull { it.drmConfiguration != null }?.let { source ->
            return VesperExternalPlaybackResult.Unsupported(
                drmUnsupportedRouteMessage("externalPlayback"),
                details = drmUnsupportedRouteDetails(source, route = "externalPlayback"),
            )
        }
        return when (activeRouteId) {
            CAST_ROUTE_ID -> loadCast(item, startPositionMs, autoplay)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> loadDlnaAsync(item, startPositionMs, autoplay)
        }
    }

    fun play(): VesperExternalPlaybackResult {
        checkNotReleased()
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.play().toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                val result = session.play().toExternalResult(session.device.routeId)
                if (result is VesperExternalPlaybackResult.Success) {
                    emitEvent(VesperExternalPlaybackEventKind.Playing, session.device.routeId, session.device.friendlyName)
                }
                result
            }
        }
    }

    suspend fun playAsync(): VesperExternalPlaybackResult {
        checkNotReleased()
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.play().toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                val result = session.playAsync().toExternalResult(session.device.routeId)
                if (result is VesperExternalPlaybackResult.Success) {
                    emitEvent(VesperExternalPlaybackEventKind.Playing, session.device.routeId, session.device.friendlyName)
                }
                result
            }
        }
    }

    fun pause(): VesperExternalPlaybackResult {
        checkNotReleased()
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.pause().toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                val result = session.pause().toExternalResult(session.device.routeId)
                if (result is VesperExternalPlaybackResult.Success) {
                    emitEvent(VesperExternalPlaybackEventKind.Paused, session.device.routeId, session.device.friendlyName)
                }
                result
            }
        }
    }

    suspend fun pauseAsync(): VesperExternalPlaybackResult {
        checkNotReleased()
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.pause().toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                val result = session.pauseAsync().toExternalResult(session.device.routeId)
                if (result is VesperExternalPlaybackResult.Success) {
                    emitEvent(VesperExternalPlaybackEventKind.Paused, session.device.routeId, session.device.friendlyName)
                }
                result
            }
        }
    }

    fun stop(): VesperExternalPlaybackResult {
        checkNotReleased()
        val result = when (activeRouteId) {
            CAST_ROUTE_ID -> castController.stop().toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                session.stop().toExternalResult(session.device.routeId)
            }
        }
        if (result is VesperExternalPlaybackResult.Success) {
            invalidateActiveRelay()
            emitEvent(VesperExternalPlaybackEventKind.Stopped, activeRouteId)
        }
        return result
    }

    suspend fun stopAsync(): VesperExternalPlaybackResult {
        checkNotReleased()
        val result = when (activeRouteId) {
            CAST_ROUTE_ID -> castController.stop().toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                session.stopAsync().toExternalResult(session.device.routeId)
            }
        }
        if (result is VesperExternalPlaybackResult.Success) {
            invalidateActiveRelay()
            emitEvent(VesperExternalPlaybackEventKind.Stopped, activeRouteId)
        }
        return result
    }

    fun seekTo(positionMs: Long): VesperExternalPlaybackResult {
        checkNotReleased()
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.seekTo(positionMs).toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                session.seekTo(positionMs).toExternalResult(session.device.routeId)
            }
        }
    }

    suspend fun seekToAsync(positionMs: Long): VesperExternalPlaybackResult {
        checkNotReleased()
        return when (activeRouteId) {
            CAST_ROUTE_ID -> castController.seekTo(positionMs).toExternalResult(CAST_ROUTE_ID)
            null -> VesperExternalPlaybackResult.Unavailable("No external playback route is connected.")
            else -> {
                val session = dlnaSession
                    ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
                session.seekToAsync(positionMs).toExternalResult(session.device.routeId)
            }
        }
    }

    fun disconnect(): VesperExternalPlaybackResult {
        checkNotReleased()
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
        emitEvent(VesperExternalPlaybackEventKind.RouteDisconnected, routeId)
        return VesperExternalPlaybackResult.Success(routeId = routeId)
    }

    suspend fun disconnectAsync(): VesperExternalPlaybackResult {
        checkNotReleased()
        val routeId = activeRouteId
        if (routeId != null) {
            runCatching {
                if (routeId == CAST_ROUTE_ID) {
                    castController.stop()
                } else {
                    dlnaSession?.stopAsync()
                }
            }
        }
        invalidateActiveRelay()
        activeRouteId = null
        dlnaSession = null
        emitRoutes()
        emitEvent(VesperExternalPlaybackEventKind.RouteDisconnected, routeId)
        return VesperExternalPlaybackResult.Success(routeId = routeId)
    }

    fun release() {
        if (released) {
            return
        }
        released = true
        runCatching {
            CastContext
                .getSharedInstance(applicationContext)
                .sessionManager
                .removeSessionManagerListener(castSessionListener, CastSession::class.java)
        }
        castContextExecutor.shutdownNow()
        stopDiscovery()
        invalidateActiveRelay()
        relayServer.stop()
    }

    companion object {
        const val CAST_ROUTE_ID: String = "cast:active"
    }
}
