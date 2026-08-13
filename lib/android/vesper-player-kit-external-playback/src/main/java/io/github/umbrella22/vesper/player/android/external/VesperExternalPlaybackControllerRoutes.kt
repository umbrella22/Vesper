package io.github.umbrella22.vesper.player.android.external

import io.github.umbrella22.vesper.player.android.external.internal.dlna.VesperDlnaDevice
import io.github.umbrella22.vesper.player.android.external.internal.dlna.dlnaRouteIdentityKey
import io.github.umbrella22.vesper.player.android.external.internal.dlna.matchesRouteId

internal fun VesperExternalPlaybackController.findDlnaDevice(routeId: String): VesperDlnaDevice? {
    dlnaDevices[routeId]?.let { return it }
    dlnaDevices.values
        .firstOrNull { device -> device.matchesRouteId(routeId) }
        ?.let { return it }
    return recentlySeenDlnaDevice(routeId)
}

internal fun VesperExternalPlaybackController.recentlySeenDlnaDevice(routeId: String): VesperDlnaDevice? {
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

internal fun VesperExternalPlaybackController.dlnaRouteCacheMissDetails(routeId: String): Map<String, String> =
    buildMap {
        put("severity", "warning")
        put("routeId", routeId)
        put("routeIdentity", dlnaRouteIdentityKey(routeId))
        put("availableRouteIds", dlnaDevices.keys.joinToString(","))
        put("recentRouteIds", recentlySeenDlnaDevices.keys.joinToString(","))
    }

internal fun VesperExternalPlaybackController.pruneRecentlySeenDlnaDevices() {
    val now = System.currentTimeMillis()
    val iterator = recentlySeenDlnaDevices.iterator()
    while (iterator.hasNext()) {
        if (iterator.next().value.expiresAtMillis <= now) {
            iterator.remove()
        }
    }
}
