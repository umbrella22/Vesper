package io.github.ikaros.vesper.player.android

import android.content.Context
import android.view.ViewGroup
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import kotlinx.coroutines.flow.StateFlow

class VesperPlayerController internal constructor(
    private val bridge: PlayerBridge,
) {
    val backend: PlayerBridgeBackend
        get() = bridge.backend

    val uiState: StateFlow<PlayerHostUiState>
        get() = bridge.uiState

    fun initialize() = bridge.initialize()

    fun dispose() = bridge.dispose()

    fun selectSource(source: VesperPlayerSource) = bridge.selectSource(source)

    fun attachSurfaceHost(host: ViewGroup) = bridge.attachSurfaceHost(host)

    fun detachSurfaceHost() = bridge.detachSurfaceHost()

    fun play() = bridge.play()

    fun pause() = bridge.pause()

    fun togglePause() = bridge.togglePause()

    fun stop() = bridge.stop()

    fun seekBy(deltaMs: Long) = bridge.seekBy(deltaMs)

    fun seekToRatio(ratio: Float) = bridge.seekToRatio(ratio)

    fun seekToLiveEdge() = bridge.seekToLiveEdge()

    fun setPlaybackRate(rate: Float) = bridge.setPlaybackRate(rate)

    companion object {
        val supportedPlaybackRates: List<Float> = listOf(0.5f, 1.0f, 1.5f, 2.0f, 3.0f)
    }
}

object VesperPlayerControllerFactory {
    fun createDefault(
        context: Context,
        initialSource: VesperPlayerSource? = null,
    ): VesperPlayerController =
        VesperPlayerController(PlayerBridgeFactory.createDefault(context, initialSource))
}

@Composable
fun rememberVesperPlayerController(
    initialSource: VesperPlayerSource? = null,
): VesperPlayerController {
    val isPreview = LocalInspectionMode.current
    val context = LocalContext.current.applicationContext
    return remember(isPreview, context, initialSource) {
        if (isPreview) {
            VesperPlayerController(FakePlayerBridge(initialSource))
        } else {
            VesperPlayerControllerFactory.createDefault(context, initialSource)
        }
    }
}
