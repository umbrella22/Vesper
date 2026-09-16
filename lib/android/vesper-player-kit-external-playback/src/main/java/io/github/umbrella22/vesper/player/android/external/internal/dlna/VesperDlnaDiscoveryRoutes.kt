package io.github.umbrella22.vesper.player.android.external.internal.dlna

import java.util.Locale

internal fun VesperDlnaDiscovery.pruneExpired(generation: Long) {
    val now = System.currentTimeMillis()
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) return
        if (!devices.entries.removeIf { it.value.expiresAtMillis <= now }) return
        queueRouteSnapshotLocked()
    }
    emitPendingRoutes()
}

internal fun VesperDlnaDiscovery.upsertDevice(device: VesperDlnaDevice, generation: Long) {
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) return
        devices[device.routeId] = device
        queueRouteSnapshotLocked()
    }
    emitPendingRoutes()
}

internal fun VesperDlnaDiscovery.refreshKnownDevice(
    request: VesperDlnaDescriptionRequest,
    binding: DlnaNetworkBinding,
    generation: Long,
): Boolean {
    val refreshed = synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) return false
        val entry = devices.entries.firstOrNull { (_, device) ->
            device.matchesDescriptionRequest(request)
        } ?: return false
        if (!entry.value.canReuseDescriptionFor(request, binding)) return false
        val updated = entry.value.copy(
            usn = request.usn,
            expiresAtMillis = maxOf(entry.value.expiresAtMillis, request.expiresAtMillis),
        )
        devices[entry.key] = updated
        queueRouteSnapshotLocked()
        updated
    }
    emitPendingRoutes()
    emitDiagnostic(
        code = "description_fetch_skipped_known_route",
        severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
        message = "Known DLNA route was refreshed from SSDP without refetching its description.",
        details = request.details("routeId" to refreshed.routeId),
    )
    return true
}

internal fun VesperDlnaDiscovery.removeDevice(routeId: String, generation: Long): Boolean {
    synchronized(routeLock) {
        if (!isDiscoveryActive(generation)) return false
        val directRemoved = devices.remove(routeId) != null
        val aliasKey = if (directRemoved) {
            null
        } else {
            devices.entries.firstOrNull { (_, device) -> device.matchesRouteId(routeId) }?.key
        }
        val aliasRemoved = aliasKey?.let { devices.remove(it) != null } == true
        if (!directRemoved && !aliasRemoved) return false
        queueRouteSnapshotLocked()
    }
    emitPendingRoutes()
    return true
}

// Call under routeLock. Coalesce concurrent updates into one pending snapshot;
// the active delivery always finishes before the latest snapshot is emitted.
internal fun VesperDlnaDiscovery.queueRouteSnapshotLocked() {
    pendingRouteSnapshot = devices.values
        .filter { it.supportsPlayback }
        .sortedBy { it.friendlyName.lowercase(Locale.US) }
}

internal fun VesperDlnaDiscovery.emitPendingRoutes() {
    synchronized(routeLock) {
        if (routeDeliveryInProgress) return
        routeDeliveryInProgress = true
    }
    var firstFailure: Throwable? = null
    while (true) {
        val snapshot = synchronized(routeLock) {
            val pending = pendingRouteSnapshot
            if (pending == null) {
                routeDeliveryInProgress = false
            }
            pendingRouteSnapshot = null
            pending
        } ?: break
        // No monitor is held during host callbacks, including reentrant stop().
        try {
            listener.onRoutesChanged(snapshot)
        } catch (error: Throwable) {
            // A concurrent stop may already have queued its final empty snapshot.
            // Deliver pending state before propagating the first callback failure.
            if (firstFailure == null) firstFailure = error
        }
    }
    firstFailure?.let { throw it }
}
