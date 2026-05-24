package io.github.ikaros.vesper.player.android.external

import android.content.Context
import android.os.Handler
import android.os.Looper
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastSession
import io.github.ikaros.vesper.player.android.external.internal.cast.VesperCastController
import io.github.ikaros.vesper.player.android.external.internal.cast.VesperCastLoadRequest
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDevice
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDiscovery
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDiscoveryDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaProtocolInfoParser
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaSession
import io.github.ikaros.vesper.player.android.external.internal.dlna.dlnaRouteIdentityKey
import io.github.ikaros.vesper.player.android.external.internal.dlna.matchesRouteId
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalPlaybackSourcePreparer
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalPlaybackTarget
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalRouteCapabilities
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationRequest
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationResult
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayServer
import io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg.VesperRelayFfmpegAdapter
import java.net.InetAddress
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

class VesperExternalPlaybackController(context: Context) {
    private val applicationContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())
    private val relayServer = VesperRelayServer(
        applicationContext,
        formatAdapter = VesperRelayFfmpegAdapter(applicationContext),
        diagnosticListener = { diagnostic -> emitRelayDiagnostic(diagnostic) },
    )
    private val sourcePreparer = VesperExternalPlaybackSourcePreparer(relayServer)
    private val castController = VesperCastController(applicationContext)
    private val dlnaDevices = linkedMapOf<String, VesperDlnaDevice>()
    private val recentlySeenDlnaDevices = linkedMapOf<String, RecentDlnaDevice>()
    private val activeRelayTokens = mutableSetOf<String>()
    private var discoveryGeneration = 0
    private var activeRouteId: String? = null
    private var activeCastRouteName: String? = null
    private var dlnaDiscovery: VesperDlnaDiscovery? = null
    private var dlnaSession: VesperDlnaSession? = null
    @Volatile
    private var released = false
    private val castContextExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "vesper-cast-context").apply { isDaemon = true }
    }

    private val _routes = MutableStateFlow<List<VesperExternalPlaybackRoute>>(emptyList())
    val routes: StateFlow<List<VesperExternalPlaybackRoute>> = _routes.asStateFlow()

    private val _events = MutableSharedFlow<VesperExternalPlaybackEvent>(extraBufferCapacity = 64)
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

    private fun loadCast(
        item: VesperExternalPlaybackMediaItem,
        startPositionMs: Long,
        autoplay: Boolean,
    ): VesperExternalPlaybackResult {
        if (!castController.isCastSessionAvailable()) {
            return VesperExternalPlaybackResult.Unavailable("No active Cast session.")
        }
        val prepared = prepareSource(
            item = item,
            target = VesperExternalPlaybackTarget.Cast,
            capabilities = VesperExternalRouteCapabilities(
                supportsProgressive = true,
                supportsHls = true,
                supportsDash = true,
                supportsMpegTs = true,
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
        if (castResult is VesperExternalPlaybackResult.Success) {
            prepared.relayToken?.let(activeRelayTokens::add)
            emitEvent(VesperExternalPlaybackEventKind.Loaded, CAST_ROUTE_ID, activeCastRouteName)
        }
        return castResult
    }

    private fun loadDlna(
        item: VesperExternalPlaybackMediaItem,
        startPositionMs: Long,
        autoplay: Boolean,
    ): VesperExternalPlaybackResult {
        val session = dlnaSession
            ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
        val protocolInfo = runCatching { session.protocolInfo() }.getOrDefault("")
        val prepared = prepareDlnaSource(item, session, protocolInfo) ?: return lastPrepareFailure
        val dlnaResult = session.load(
            source = prepared.source,
            metadata = item.metadata,
            startPositionMs = startPositionMs,
            autoplay = autoplay,
        ).toExternalResult(session.device.routeId, prepared.relayEnabled)
        handleDlnaLoadedResult(session, prepared, dlnaResult)
        return dlnaResult
    }

    private suspend fun loadDlnaAsync(
        item: VesperExternalPlaybackMediaItem,
        startPositionMs: Long,
        autoplay: Boolean,
    ): VesperExternalPlaybackResult {
        val session = dlnaSession
            ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
        val protocolInfo = runCatching { session.protocolInfoAsync() }.getOrDefault("")
        val prepared = prepareDlnaSource(item, session, protocolInfo) ?: return lastPrepareFailure
        val dlnaResult = session.loadAsync(
            source = prepared.source,
            metadata = item.metadata,
            startPositionMs = startPositionMs,
            autoplay = autoplay,
        ).toExternalResult(session.device.routeId, prepared.relayEnabled)
        handleDlnaLoadedResult(session, prepared, dlnaResult)
        return dlnaResult
    }

    private fun prepareDlnaSource(
        item: VesperExternalPlaybackMediaItem,
        session: VesperDlnaSession,
        protocolInfo: String,
    ): VesperExternalSourcePreparationResult.Prepared? =
        prepareSource(
            item = item,
            target = VesperExternalPlaybackTarget.Dlna,
            capabilities = VesperExternalRouteCapabilities(
                supportsProgressive = true,
                supportsHls = VesperDlnaProtocolInfoParser.supportsHls(protocolInfo),
                supportsDash = VesperDlnaProtocolInfoParser.supportsDash(protocolInfo),
                supportsMpegTs = protocolInfo.isBlank() ||
                    VesperDlnaProtocolInfoParser.supportsMpegTs(protocolInfo),
            ),
            routeId = session.device.routeId,
            routeName = session.device.friendlyName,
            routeLocalAddress = session.device.localAddress,
        )

    private fun handleDlnaLoadedResult(
        session: VesperDlnaSession,
        prepared: VesperExternalSourcePreparationResult.Prepared,
        dlnaResult: VesperExternalPlaybackResult,
    ) {
        if (dlnaResult is VesperExternalPlaybackResult.Success) {
            prepared.relayToken?.let(activeRelayTokens::add)
            emitEvent(VesperExternalPlaybackEventKind.Loaded, session.device.routeId, session.device.friendlyName)
        }
    }

    private var lastPrepareFailure: VesperExternalPlaybackResult =
        VesperExternalPlaybackResult.Unsupported("No playable external playback source is available.")

    private fun prepareSource(
        item: VesperExternalPlaybackMediaItem,
        target: VesperExternalPlaybackTarget,
        capabilities: VesperExternalRouteCapabilities,
        routeId: String? = null,
        routeName: String? = null,
        routeLocalAddress: InetAddress? = null,
    ): VesperExternalSourcePreparationResult.Prepared? {
        return when (
            val prepared = sourcePreparer.prepare(
                VesperExternalSourcePreparationRequest(
                    target = target,
                    sources = item.sources,
                    proxyPolicy = item.proxyPolicy.toInternal(),
                    capabilities = capabilities,
                    formatAdaptation = item.formatAdaptation.toInternal(),
                    routeId = routeId,
                    routeName = routeName,
                    routeLocalAddress = routeLocalAddress,
                ),
            )
        ) {
            is VesperExternalSourcePreparationResult.Prepared -> prepared
            is VesperExternalSourcePreparationResult.Unsupported -> {
                prepared.code?.let { code ->
                    emitEvent(
                        VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
                        routeId = routeId,
                        routeName = routeName,
                        message = prepared.message,
                        code = code,
                        details = prepared.details + mapOf("severity" to "warning"),
                    )
                }
                lastPrepareFailure = VesperExternalPlaybackResult.Unsupported(prepared.message)
                null
            }
        }
    }

    private fun findDlnaDevice(routeId: String): VesperDlnaDevice? {
        dlnaDevices[routeId]?.let { return it }
        dlnaDevices.values
            .firstOrNull { device -> device.matchesRouteId(routeId) }
            ?.let { return it }
        return recentlySeenDlnaDevice(routeId)
    }

    private fun recentlySeenDlnaDevice(routeId: String): VesperDlnaDevice? {
        pruneRecentlySeenDlnaDevices()
        val recent = recentlySeenDlnaDevices[routeId]
            ?: recentlySeenDlnaDevices.values
                .firstOrNull { recent -> recent.device.matchesRouteId(routeId) }
            ?: return null
        emitEvent(
            VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
            message = "Using a recently discovered DLNA route during discovery refresh.",
            code = "dlna_route_recent_cache_used",
            details = mapOf(
                "severity" to "info",
                "requestedRouteId" to routeId,
                "routeId" to recent.device.routeId,
                "routeName" to recent.device.friendlyName,
            ),
        )
        return recent.device
    }

    private fun dlnaRouteCacheMissDetails(routeId: String): Map<String, String> =
        buildMap {
            put("severity", "warning")
            put("routeId", routeId)
            put("routeIdentity", dlnaRouteIdentityKey(routeId))
            put("availableRouteIds", dlnaDevices.keys.joinToString(","))
            put("recentRouteIds", recentlySeenDlnaDevices.keys.joinToString(","))
        }

    private fun pruneRecentlySeenDlnaDevices() {
        val now = System.currentTimeMillis()
        recentlySeenDlnaDevices.entries.removeIf { it.value.expiresAtMillis <= now }
    }

    private fun invalidateActiveRelay() {
        activeRelayTokens.forEach(relayServer::invalidate)
        activeRelayTokens.clear()
    }

    private fun emitRoutes() {
        val next = mutableListOf<VesperExternalPlaybackRoute>()
        if (castController.isCastSessionAvailable()) {
            next += VesperExternalPlaybackRoute(
                routeId = CAST_ROUTE_ID,
                name = activeCastRouteName ?: "Cast device",
                kind = VesperExternalPlaybackRouteKind.Cast,
                active = activeRouteId == CAST_ROUTE_ID,
                available = true,
            )
        }
        next += dlnaDevices.values.map { device ->
            VesperExternalPlaybackRoute(
                routeId = device.routeId,
                name = device.friendlyName,
                kind = VesperExternalPlaybackRouteKind.Dlna,
                manufacturer = device.manufacturer,
                modelName = device.modelName,
                active = activeRouteId?.let(device::matchesRouteId) == true,
                available = true,
            )
        }
        _routes.value = next
    }

    private fun prepareCastContextAsync(
        onSuccess: CastContext.() -> Unit,
        onFailure: (Throwable) -> Unit = {},
    ) {
        runCatching {
            CastContext
                .getSharedInstance(applicationContext, castContextExecutor.asExecutor())
                .addOnSuccessListener { castContext ->
                    if (released) {
                        return@addOnSuccessListener
                    }
                    mainHandler.post {
                        if (!released) {
                            runCatching { castContext.onSuccess() }
                                .onFailure(onFailure)
                        }
                    }
                }
                .addOnFailureListener { error ->
                    if (!released) {
                        mainHandler.post {
                            if (!released) {
                                onFailure(error)
                            }
                        }
                    }
                }
        }.onFailure { error ->
            if (!released) {
                mainHandler.post {
                    if (!released) {
                        onFailure(error)
                    }
                }
            }
        }
    }

    private fun emitRelayDiagnostic(diagnostic: VesperRelayDiagnostic) {
        mainHandler.post {
            emitEvent(
                VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
                message = diagnostic.message,
                code = diagnostic.code,
                details = diagnostic.details + mapOf("severity" to diagnostic.severity),
            )
        }
    }

    private fun emitEvent(
        kind: VesperExternalPlaybackEventKind,
        routeId: String? = null,
        routeName: String? = null,
        message: String? = null,
        positionMs: Long? = null,
        code: String? = null,
        details: Map<String, String> = emptyMap(),
    ) {
        _events.tryEmit(
            VesperExternalPlaybackEvent(
                kind = kind,
                routeId = routeId,
                routeName = routeName,
                message = message,
                positionMs = positionMs,
                code = code,
                details = details,
            ),
        )
    }

    private fun checkNotReleased() {
        check(!released) { "VesperExternalPlaybackController has been released." }
    }

    companion object {
        const val CAST_ROUTE_ID: String = "cast:active"
    }
}
