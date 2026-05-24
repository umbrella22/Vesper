package io.github.ikaros.vesper.player.flutter.android

import android.view.View
import android.widget.FrameLayout
import io.flutter.plugin.platform.PlatformView
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperPlayerController
import kotlinx.coroutines.Job

internal data class PlayerSession(
    val id: String,
    val controller: VesperPlayerController,
    val benchmarkConsoleLogging: Boolean = false,
    var hostView: FrameLayout? = null,
    var pendingHostDetachJob: Job? = null,
    var hostDetachGeneration: Long = 0L,
    var observerJob: Job? = null,
    var lastError: Map<String, Any?>? = null,
    var viewport: FlutterViewport? = null,
    var viewportHint: FlutterViewportHint = FlutterViewportHint.hidden(),
) {
    fun hasAttachedHost(): Boolean = hostView != null

    fun cancelPendingHostDetach() {
        pendingHostDetachJob?.cancel()
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

