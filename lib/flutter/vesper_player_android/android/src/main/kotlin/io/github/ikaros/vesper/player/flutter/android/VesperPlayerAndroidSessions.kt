package io.github.ikaros.vesper.player.flutter.android

import android.view.View
import android.widget.FrameLayout
import io.flutter.plugin.platform.PlatformView
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeRequest
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeResult
import io.github.ikaros.vesper.player.android.VesperPlayerController
import kotlinx.coroutines.Job

internal data class SourceBoundCapabilityProbe(
    val sourceUri: String?,
    val sourceProtocol: String?,
    val result: VesperPlaybackCapabilityProbeResult,
) {
    fun sourceMatches(request: VesperPlaybackCapabilityProbeRequest?): Boolean {
        val requestSource = request?.source ?: return sourceUri == null
        return sourceUri == requestSource.uri &&
            sourceProtocol == requestSource.protocol.toWireName()
    }
}

internal data class PlayerSession(
    val id: String,
    val controller: VesperPlayerController,
    val benchmarkConsoleLogging: Boolean = false,
    var hostView: FrameLayout? = null,
    var pendingHostDetachJob: Job? = null,
    var hostDetachGeneration: Long = 0L,
    var observerJob: Job? = null,
    var lastError: Map<String, Any?>? = null,
    var lastEmittedTerminalError: Map<String, Any?>? = null,
    var lastEmittedSnapshot: Map<String, Any?>? = null,
    var viewport: FlutterViewport? = null,
    var viewportHint: FlutterViewportHint = FlutterViewportHint.hidden(),
    var recentCapabilityProbe: SourceBoundCapabilityProbe? = null,
    var pictureInPictureConfiguration: FlutterPictureInPictureConfiguration =
        FlutterPictureInPictureConfiguration(),
    var pictureInPictureState: String = "inactive",
    var pictureInPictureActive: Boolean = false,
) {
    fun hasAttachedHost(): Boolean = hostView != null

    fun cancelPendingHostDetach() {
        pendingHostDetachJob?.cancel()
        pendingHostDetachJob = null
    }

    fun clearPendingHostDetach() {
        pendingHostDetachJob = null
    }

    fun advanceHostDetachGeneration(): Long {
        hostDetachGeneration += 1L
        return hostDetachGeneration
    }
}

internal data class DownloadSession(
    val id: String,
    val manager: VesperDownloadManager,
    var observerJob: Job? = null,
    var lastError: Map<String, Any?>? = null,
)

internal data class FlutterViewport(
    val left: Double,
    val top: Double,
    val width: Double,
    val height: Double,
) {
    fun toMap(): Map<String, Any> =
        mapOf(
            "left" to left,
            "top" to top,
            "width" to width,
            "height" to height,
        )
}

internal data class FlutterViewportHint(
    val kind: String,
    val visibleFraction: Double,
) {
    fun toMap(): Map<String, Any> =
        mapOf(
            "kind" to kind,
            "visibleFraction" to visibleFraction,
        )

    companion object {
        fun hidden(): FlutterViewportHint = FlutterViewportHint("hidden", 0.0)
    }
}

internal class VesperPlayerPlatformView(
    private val hostView: FrameLayout,
    private val onDispose: () -> Unit,
) : PlatformView {
    override fun getView(): View = hostView

    override fun dispose() {
        onDispose()
    }
}

internal class SurfaceHostLifecycleCoordinator<Session : Any, Host : Any>(
    private val findSession: (String) -> Session?,
    private val getHost: (Session) -> Host?,
    private val setHost: (Session, Host?) -> Unit,
    private val cancelPendingDetach: (Session) -> Unit,
    private val clearPendingDetach: (Session) -> Unit,
    private val advanceDetachGeneration: (Session) -> Long,
    private val currentDetachGeneration: (Session) -> Long,
    private val schedulePendingDetach: (
        session: Session,
        generation: Long,
        action: () -> Unit,
    ) -> Unit,
    private val attachHost: (Session, Host) -> Unit,
    private val detachHost: (Session, Host) -> Unit,
    private val clearHostView: (Host) -> Unit,
    private val emitSnapshot: (Session) -> Unit,
) {
    fun bind(playerId: String, host: Host) {
        val session = findSession(playerId) ?: return
        cancelPendingDetach(session)
        advanceDetachGeneration(session)
        if (getHost(session) === host) {
            attachHost(session, host)
            emitSnapshot(session)
            return
        }

        val previousHost = getHost(session)
        setHost(session, host)
        attachHost(session, host)
        previousHost?.let(clearHostView)
        emitSnapshot(session)
    }

    fun unbind(playerId: String, host: Host) {
        val session = findSession(playerId) ?: return
        if (getHost(session) !== host) {
            return
        }
        cancelPendingDetach(session)
        val generation = advanceDetachGeneration(session)
        schedulePendingDetach(session, generation) {
            val currentSession = findSession(playerId) ?: return@schedulePendingDetach
            if (currentSession !== session || getHost(currentSession) !== host) {
                return@schedulePendingDetach
            }
            if (currentDetachGeneration(currentSession) != generation) {
                return@schedulePendingDetach
            }
            detachHost(currentSession, host)
            setHost(currentSession, null)
            clearPendingDetach(currentSession)
            emitSnapshot(currentSession)
        }
    }

    fun detachSession(session: Session) {
        cancelPendingDetach(session)
        advanceDetachGeneration(session)
        getHost(session)?.let { host ->
            detachHost(session, host)
            setHost(session, null)
        }
    }
}
