package io.github.ikaros.vesper.player.android.external

import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.SessionManagerListener
import io.github.ikaros.vesper.player.android.external.internal.cast.VesperCastOperationResult
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaDevice
import io.github.ikaros.vesper.player.android.external.internal.dlna.VesperDlnaOperationResult
import java.util.concurrent.Executor
import java.util.concurrent.ExecutorService

internal fun ExecutorService.asExecutor(): Executor = Executor { command ->
    execute(command)
}

internal fun VesperCastOperationResult.toExternalResult(
    routeId: String? = null,
    relayEnabled: Boolean = false,
): VesperExternalPlaybackResult =
    when (this) {
        VesperCastOperationResult.Success -> VesperExternalPlaybackResult.Success(routeId, relayEnabled)
        is VesperCastOperationResult.Unavailable -> VesperExternalPlaybackResult.Unavailable(message)
        is VesperCastOperationResult.Unsupported -> VesperExternalPlaybackResult.Unsupported(message)
    }

internal fun VesperDlnaOperationResult.toExternalResult(
    routeId: String? = null,
    relayEnabled: Boolean = false,
): VesperExternalPlaybackResult =
    when (this) {
        VesperDlnaOperationResult.Success -> VesperExternalPlaybackResult.Success(routeId, relayEnabled)
        is VesperDlnaOperationResult.Unavailable -> VesperExternalPlaybackResult.Unavailable(message)
        is VesperDlnaOperationResult.Unsupported -> VesperExternalPlaybackResult.Unsupported(message)
        is VesperDlnaOperationResult.Failed -> VesperExternalPlaybackResult.Failed(message)
    }

internal class VesperExternalCastSessionListener(
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

internal data class RecentDlnaDevice(
    val device: VesperDlnaDevice,
    val expiresAtMillis: Long,
)

internal const val RECENT_DLNA_ROUTE_GRACE_MS: Long = 120_000L
