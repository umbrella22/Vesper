package io.github.ikaros.vesper.player.android.external

import io.github.ikaros.vesper.player.android.external.internal.cast.VesperCastLoadRequest
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaProtocolInfoParser
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaSession
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalPlaybackTarget
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalRouteCapabilities
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationRequest
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationResult
import io.github.ikaros.vesper.player.android.external.internal.relay.releaseRelayIfLoadFailed
import java.net.InetAddress
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.withContext

internal fun VesperExternalPlaybackController.loadCast(
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
    val castResult =
        try {
            castController.load(
                VesperCastLoadRequest(
                    source = prepared.source,
                    metadata = item.metadata,
                    startPositionMs = startPositionMs,
                    autoplay = autoplay,
                ),
            ).toExternalResult(VesperExternalPlaybackController.CAST_ROUTE_ID, prepared.relayEnabled)
        } catch (error: Exception) {
            VesperExternalPlaybackResult.Failed(error.message ?: "Cast load failed.")
        }
    if (castResult is VesperExternalPlaybackResult.Success) {
        activateRelayForLoadedSource(prepared)
        emitEvent(VesperExternalPlaybackEventKind.Loaded, VesperExternalPlaybackController.CAST_ROUTE_ID, activeCastRouteName)
    } else {
        prepared.releaseRelayIfLoadFailed(relayServer, loadSucceeded = false)
    }
    return castResult
}

internal suspend fun VesperExternalPlaybackController.loadCastAsync(
    item: VesperExternalPlaybackMediaItem,
    startPositionMs: Long,
    autoplay: Boolean,
): VesperExternalPlaybackResult {
    if (!castController.isCastSessionAvailable()) {
        return VesperExternalPlaybackResult.Unavailable("No active Cast session.")
    }
    val prepared = prepareSourceAsync(
        item = item,
        target = VesperExternalPlaybackTarget.Cast,
        capabilities = VesperExternalRouteCapabilities(
            supportsProgressive = true,
            supportsHls = true,
            supportsDash = true,
            supportsMpegTs = true,
        ),
    ) ?: return lastPrepareFailure
    val castResult =
        try {
            castController.load(
                VesperCastLoadRequest(
                    source = prepared.source,
                    metadata = item.metadata,
                    startPositionMs = startPositionMs,
                    autoplay = autoplay,
                ),
            ).toExternalResult(VesperExternalPlaybackController.CAST_ROUTE_ID, prepared.relayEnabled)
        } catch (error: CancellationException) {
            prepared.releaseRelayIfLoadFailed(relayServer, loadSucceeded = false)
            throw error
        } catch (error: Exception) {
            VesperExternalPlaybackResult.Failed(error.message ?: "Cast load failed.")
        }
    if (castResult is VesperExternalPlaybackResult.Success) {
        activateRelayForLoadedSource(prepared)
        emitEvent(VesperExternalPlaybackEventKind.Loaded, VesperExternalPlaybackController.CAST_ROUTE_ID, activeCastRouteName)
    } else {
        prepared.releaseRelayIfLoadFailed(relayServer, loadSucceeded = false)
    }
    return castResult
}

internal fun VesperExternalPlaybackController.loadDlna(
    item: VesperExternalPlaybackMediaItem,
    startPositionMs: Long,
    autoplay: Boolean,
): VesperExternalPlaybackResult {
    val session = dlnaSession
        ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
    val protocolInfo = runCatching { session.protocolInfo() }.getOrDefault("")
    val prepared = prepareDlnaSource(item, session, protocolInfo) ?: return lastPrepareFailure
    val dlnaResult =
        try {
            session.load(
                source = prepared.source,
                metadata = item.metadata,
                startPositionMs = startPositionMs,
                autoplay = autoplay,
            ).toExternalResult(session.device.routeId, prepared.relayEnabled)
        } catch (error: Exception) {
            VesperExternalPlaybackResult.Failed(error.message ?: "DLNA load failed.")
        }
    handleDlnaLoadedResult(session, prepared, dlnaResult)
    return dlnaResult
}

internal suspend fun VesperExternalPlaybackController.loadDlnaAsync(
    item: VesperExternalPlaybackMediaItem,
    startPositionMs: Long,
    autoplay: Boolean,
): VesperExternalPlaybackResult {
    val session = dlnaSession
        ?: return VesperExternalPlaybackResult.Unavailable("No active DLNA session.")
    val protocolInfo = runCatching { session.protocolInfoAsync() }.getOrDefault("")
    val prepared = prepareDlnaSourceAsync(item, session, protocolInfo) ?: return lastPrepareFailure
    val dlnaResult =
        try {
            withPreparedRelayLoadCancellationCleanup(
                prepared = prepared,
                cleanup = { relayPrepared ->
                    relayPrepared.releaseRelayIfLoadFailed(relayServer, loadSucceeded = false)
                },
            ) {
                session.loadAsync(
                    source = prepared.source,
                    metadata = item.metadata,
                    startPositionMs = startPositionMs,
                    autoplay = autoplay,
                ).toExternalResult(session.device.routeId, prepared.relayEnabled)
            }
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            VesperExternalPlaybackResult.Failed(error.message ?: "DLNA load failed.")
        }
    handleDlnaLoadedResult(session, prepared, dlnaResult)
    return dlnaResult
}

internal fun VesperExternalPlaybackController.prepareDlnaSource(
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

internal suspend fun VesperExternalPlaybackController.prepareDlnaSourceAsync(
    item: VesperExternalPlaybackMediaItem,
    session: VesperDlnaSession,
    protocolInfo: String,
): VesperExternalSourcePreparationResult.Prepared? =
    prepareSourceAsync(
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

internal fun VesperExternalPlaybackController.handleDlnaLoadedResult(
    session: VesperDlnaSession,
    prepared: VesperExternalSourcePreparationResult.Prepared,
    dlnaResult: VesperExternalPlaybackResult,
) {
    if (dlnaResult is VesperExternalPlaybackResult.Success) {
        activateRelayForLoadedSource(prepared)
        emitEvent(VesperExternalPlaybackEventKind.Loaded, session.device.routeId, session.device.friendlyName)
    } else {
        prepared.releaseRelayIfLoadFailed(relayServer, loadSucceeded = false)
    }
}

internal var lastPrepareFailure: VesperExternalPlaybackResult =
    VesperExternalPlaybackResult.Unsupported("No playable external playback source is available.")

internal fun VesperExternalPlaybackController.prepareSource(
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

internal suspend fun VesperExternalPlaybackController.prepareSourceAsync(
    item: VesperExternalPlaybackMediaItem,
    target: VesperExternalPlaybackTarget,
    capabilities: VesperExternalRouteCapabilities,
    routeId: String? = null,
    routeName: String? = null,
    routeLocalAddress: InetAddress? = null,
): VesperExternalSourcePreparationResult.Prepared? =
    prepareExternalSourceOnIo(
        cleanup = { prepared ->
            prepared.releaseRelayIfLoadFailed(relayServer, loadSucceeded = false)
        },
    ) {
        prepareSource(
            item = item,
            target = target,
            capabilities = capabilities,
            routeId = routeId,
            routeName = routeName,
            routeLocalAddress = routeLocalAddress,
        )
    }

internal suspend fun prepareExternalSourceOnIo(
    cleanup: (VesperExternalSourcePreparationResult.Prepared) -> Unit,
    isCancelled: (() -> Boolean)? = null,
    prepare: () -> VesperExternalSourcePreparationResult.Prepared?,
): VesperExternalSourcePreparationResult.Prepared? {
    val callerJob = currentCoroutineContext()[Job]
    val prepared = withContext(Dispatchers.IO + NonCancellable) {
        prepare()
    }
    if (isCancelled?.invoke() == true || callerJob?.isCancelled == true) {
        withContext(Dispatchers.IO + NonCancellable) {
            prepared?.let(cleanup)
        }
        throw CancellationException("External source preparation was cancelled.")
    }
    return prepared
}

internal suspend fun <T> withPreparedRelayLoadCancellationCleanup(
    prepared: VesperExternalSourcePreparationResult.Prepared,
    cleanup: (VesperExternalSourcePreparationResult.Prepared) -> Unit,
    load: suspend () -> T,
): T {
    return try {
        load()
    } catch (error: CancellationException) {
        withContext(Dispatchers.IO + NonCancellable) {
            cleanup(prepared)
        }
        throw error
    }
}
