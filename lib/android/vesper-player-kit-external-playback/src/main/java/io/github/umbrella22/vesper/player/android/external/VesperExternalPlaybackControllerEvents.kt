package io.github.umbrella22.vesper.player.android.external

import com.google.android.gms.cast.framework.CastContext
import io.github.umbrella22.vesper.player.android.external.internal.dlna.matchesRouteId
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationResult
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayDiagnostic

internal fun VesperExternalPlaybackController.invalidateActiveRelay() {
    activeRelayTokens.forEach(relayServer::invalidate)
    activeRelayTokens.clear()
}

internal fun replaceActiveRelayTokens(
    activeRelayTokens: MutableSet<String>,
    relayToken: String?,
    invalidate: (String) -> Unit,
) {
    val previousTokens = activeRelayTokens.toList()
    activeRelayTokens.clear()
    relayToken?.let(activeRelayTokens::add)
    previousTokens
        .asSequence()
        .filter { token -> token != relayToken }
        .forEach(invalidate)
}

internal fun VesperExternalPlaybackController.activateRelayForLoadedSource(
    prepared: VesperExternalSourcePreparationResult.Prepared,
) {
    replaceActiveRelayTokens(activeRelayTokens, prepared.relayToken, relayServer::invalidate)
}

internal fun VesperExternalPlaybackController.emitRoutes() {
    val next = mutableListOf<VesperExternalPlaybackRoute>()
    if (castController.isCastSessionAvailable()) {
        next += VesperExternalPlaybackRoute(
            routeId = VesperExternalPlaybackController.CAST_ROUTE_ID,
            name = activeCastRouteName ?: "Cast device",
            kind = VesperExternalPlaybackRouteKind.Cast,
            active = activeRouteId == VesperExternalPlaybackController.CAST_ROUTE_ID,
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

internal fun VesperExternalPlaybackController.prepareCastContextAsync(
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

internal fun VesperExternalPlaybackController.emitRelayDiagnostic(diagnostic: VesperRelayDiagnostic) {
    mainHandler.post {
        emitEvent(
            VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
            message = diagnostic.message,
            code = diagnostic.code,
            details = diagnostic.details + mapOf("severity" to diagnostic.severity),
        )
    }
}

internal fun VesperExternalPlaybackController.emitEvent(
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

internal fun VesperExternalPlaybackController.checkNotReleased() {
    check(!released) { "VesperExternalPlaybackController has been released." }
}
