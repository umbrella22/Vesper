package io.github.ikaros.vesper.player.android

import android.view.ViewGroup
import kotlinx.coroutines.flow.StateFlow

enum class PlayerBridgeBackend {
    FakeDemo,
    VesperNativeStub,
}

enum class TimelineKind {
    Vod,
    Live,
    LiveDvr,
}

data class SeekableRangeUi(
    val startMs: Long,
    val endMs: Long,
)

data class TimelineUiState(
    val kind: TimelineKind,
    val isSeekable: Boolean,
    val seekableRange: SeekableRangeUi?,
    val liveEdgeMs: Long?,
    val positionMs: Long,
    val durationMs: Long?,
) {
    val displayedRatio: Float?
        get() {
            val range = seekableRange
            if (range != null && range.endMs > range.startMs) {
                val clamped = positionMs.coerceIn(range.startMs, range.endMs)
                return ((clamped - range.startMs).toFloat() / (range.endMs - range.startMs).toFloat())
                    .coerceIn(0f, 1f)
            }

            val total = durationMs ?: return null
            if (total <= 0L) return null
            return (positionMs.toFloat() / total.toFloat()).coerceIn(0f, 1f)
        }
}

enum class PlaybackStateUi {
    Ready,
    Playing,
    Paused,
    Finished,
}

data class PlayerHostUiState(
    val title: String,
    val subtitle: String,
    val sourceLabel: String,
    val playbackState: PlaybackStateUi,
    val playbackRate: Float,
    val isBuffering: Boolean,
    val isInterrupted: Boolean,
    val timeline: TimelineUiState,
)

interface PlayerBridge {
    val backend: PlayerBridgeBackend
    val uiState: StateFlow<PlayerHostUiState>
    val trackCatalog: StateFlow<VesperTrackCatalog>
    val trackSelection: StateFlow<VesperTrackSelectionSnapshot>

    fun initialize()
    fun dispose()
    fun refresh()
    fun selectSource(source: VesperPlayerSource)

    fun attachSurfaceHost(host: ViewGroup)
    fun detachSurfaceHost(host: ViewGroup? = null)

    fun play()
    fun pause()
    fun togglePause()
    fun stop()
    fun seekBy(deltaMs: Long)
    fun seekToRatio(ratio: Float)
    fun seekToLiveEdge()
    fun setPlaybackRate(rate: Float)
    fun setVideoTrackSelection(selection: VesperTrackSelection)
    fun setAudioTrackSelection(selection: VesperTrackSelection)
    fun setSubtitleTrackSelection(selection: VesperTrackSelection)
    fun setAbrPolicy(policy: VesperAbrPolicy)
}
