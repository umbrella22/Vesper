package io.github.ikaros.vesper.player.android

import android.view.ViewGroup
import kotlinx.coroutines.flow.StateFlow
import kotlin.math.absoluteValue

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

    val goLivePositionMs: Long?
        get() = when (kind) {
            TimelineKind.Vod -> null
            TimelineKind.Live -> liveEdgeMs
            TimelineKind.LiveDvr -> liveEdgeMs ?: seekableRange?.endMs
        }

    val liveOffsetMs: Long?
        get() = goLivePositionMs?.let { liveEdge ->
            (liveEdge - clampedPosition(positionMs)).coerceAtLeast(0L)
        }

    fun clampedPosition(positionMs: Long): Long {
        val range = seekableRange
        if (range != null && range.endMs >= range.startMs) {
            return positionMs.coerceIn(range.startMs, range.endMs)
        }

        val total = durationMs ?: return positionMs.coerceAtLeast(0L)
        return positionMs.coerceIn(0L, total.coerceAtLeast(0L))
    }

    fun positionForRatio(ratio: Float): Long {
        val normalized = ratio.coerceIn(0f, 1f)
        val range = seekableRange
        if (range != null && range.endMs >= range.startMs) {
            val width = (range.endMs - range.startMs).toFloat()
            return clampedPosition(range.startMs + (width * normalized).toLong())
        }

        return clampedPosition(((durationMs ?: 0L).toFloat() * normalized).toLong())
    }

    fun isAtLiveEdge(toleranceMs: Long = 1_500L): Boolean {
        val liveEdge = goLivePositionMs ?: return false
        return (liveEdge - clampedPosition(positionMs)).absoluteValue <= toleranceMs.coerceAtLeast(0L)
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

data class VesperVideoVariantObservation(
    val bitRate: Long? = null,
    val width: Int? = null,
    val height: Int? = null,
) {
    fun toMap(): Map<String, Any?> =
        mapOf(
            "bitRate" to bitRate,
            "width" to width,
            "height" to height,
        )
}

interface PlayerBridge {
    val backend: PlayerBridgeBackend
    val uiState: StateFlow<PlayerHostUiState>
    val trackCatalog: StateFlow<VesperTrackCatalog>
    val trackSelection: StateFlow<VesperTrackSelectionSnapshot>
    val effectiveVideoTrackId: StateFlow<String?>
    val videoVariantObservation: StateFlow<VesperVideoVariantObservation?>
    val resiliencePolicy: StateFlow<VesperPlaybackResiliencePolicy>

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
    fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy)
    fun drainBenchmarkEvents(): List<VesperBenchmarkEvent>
    fun benchmarkSummary(): VesperBenchmarkSummary
}
