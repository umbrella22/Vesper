package io.github.umbrella22.vesper.player.android.external.internal.dlna

import java.util.Locale
internal fun VesperDlnaDiscovery.pruneExpired(generation: Long) {
    val now = System.currentTimeMillis()
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) {
            return
        }
        val removed = devices.entries.removeIf { it.value.expiresAtMillis <= now }
        if (removed) {
            emitRoutesLocked()
        }
    }
}

internal fun VesperDlnaDiscovery.upsertDevice(device: VesperDlnaDevice, generation: Long) {
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) {
            return
        }
        devices[device.routeId] = device
        emitRoutesLocked()
    }
}

internal fun VesperDlnaDiscovery.refreshKnownDevice(
    request: VesperDlnaDescriptionRequest,
    binding: DlnaNetworkBinding,
    generation: Long,
): Boolean =
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) {
            return@synchronized false
        }
        val entry = devices.entries.firstOrNull { (_, device) ->
            device.matchesDescriptionRequest(request)
        } ?: return@synchronized false
        if (!entry.value.canReuseDescriptionFor(request, binding)) {
            return@synchronized false
        }
        val refreshed = entry.value.copy(
            usn = request.usn,
            expiresAtMillis = maxOf(entry.value.expiresAtMillis, request.expiresAtMillis),
        )
        devices[entry.key] = refreshed
        emitRoutesLocked()
        emitDiagnostic(
            code = "description_fetch_skipped_known_route",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
            message = "Known DLNA route was refreshed from SSDP without refetching its description.",
            details = request.details("routeId" to refreshed.routeId),
        )
        true
    }

internal fun VesperDlnaDiscovery.removeDevice(routeId: String, generation: Long): Boolean =
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) {
            return@synchronized false
        }
        val directRemoved = devices.remove(routeId) != null
        val aliasKey = if (directRemoved) {
            null
        } else {
            devices.entries
                .firstOrNull { (_, device) -> device.matchesRouteId(routeId) }
                ?.key
        }
        val aliasRemoved = aliasKey?.let { devices.remove(it) != null } == true
        val removed = directRemoved || aliasRemoved
        if (removed) {
            emitRoutesLocked()
        }
        removed
    }

internal fun VesperDlnaDiscovery.emitRoutesLocked() {
    listener.onRoutesChanged(
        devices.values
            .filter { it.supportsPlayback }
            .sortedBy { it.friendlyName.lowercase(Locale.US) },
    )
}
