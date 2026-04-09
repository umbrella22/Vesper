package io.github.ikaros.vesper.player.android

import android.content.Context
import android.view.ViewGroup
import kotlinx.coroutines.flow.StateFlow

class VesperPlayerController internal constructor(
    private val bridge: PlayerBridge,
) {
    val backend: PlayerBridgeBackend
        get() = bridge.backend

    val uiState: StateFlow<PlayerHostUiState>
        get() = bridge.uiState

    val trackCatalog: StateFlow<VesperTrackCatalog>
        get() = bridge.trackCatalog

    val trackSelection: StateFlow<VesperTrackSelectionSnapshot>
        get() = bridge.trackSelection

    fun initialize() = bridge.initialize()

    fun dispose() = bridge.dispose()

    fun refresh() = bridge.refresh()

    fun selectSource(source: VesperPlayerSource) = bridge.selectSource(source)

    fun attachSurfaceHost(host: ViewGroup) = bridge.attachSurfaceHost(host)

    fun detachSurfaceHost(host: ViewGroup? = null) = bridge.detachSurfaceHost(host)

    fun play() = bridge.play()

    fun pause() = bridge.pause()

    fun togglePause() = bridge.togglePause()

    fun stop() = bridge.stop()

    fun seekBy(deltaMs: Long) = bridge.seekBy(deltaMs)

    fun seekToRatio(ratio: Float) = bridge.seekToRatio(ratio)

    fun seekToLiveEdge() = bridge.seekToLiveEdge()

    fun setPlaybackRate(rate: Float) = bridge.setPlaybackRate(rate)

    fun setVideoTrackSelection(selection: VesperTrackSelection) =
        bridge.setVideoTrackSelection(selection)

    fun setAudioTrackSelection(selection: VesperTrackSelection) =
        bridge.setAudioTrackSelection(selection)

    fun setSubtitleTrackSelection(selection: VesperTrackSelection) =
        bridge.setSubtitleTrackSelection(selection)

    fun setAbrPolicy(policy: VesperAbrPolicy) = bridge.setAbrPolicy(policy)

    companion object {
        val supportedPlaybackRates: List<Float> = listOf(0.5f, 1.0f, 1.5f, 2.0f, 3.0f)
    }
}

object VesperPlayerControllerFactory {
    fun createDefault(
        context: Context,
        initialSource: VesperPlayerSource? = null,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
    ): VesperPlayerController =
        VesperPlayerController(
            PlayerBridgeFactory.createDefault(
                context = context,
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                surfaceKind = surfaceKind,
            )
        )

    fun createPreview(
        initialSource: VesperPlayerSource? = null,
    ): VesperPlayerController =
        VesperPlayerController(FakePlayerBridge(initialSource))
}
