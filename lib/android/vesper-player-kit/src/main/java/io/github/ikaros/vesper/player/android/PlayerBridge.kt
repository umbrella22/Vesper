package io.github.ikaros.vesper.player.android

import android.view.ViewGroup
import kotlinx.coroutines.flow.StateFlow
import kotlin.math.absoluteValue

private const val DEFAULT_SYSTEM_PLAYBACK_SEEK_OFFSET_MS = 10_000L
private const val MIN_SYSTEM_PLAYBACK_SEEK_OFFSET_MS = 1_000L
private const val MAX_SYSTEM_PLAYBACK_SEEK_OFFSET_MS = 60_000L
private const val MAX_SYSTEM_PLAYBACK_COMPACT_BUTTONS = 3

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

enum class VesperBackgroundPlaybackMode {
    Disabled,
    ContinueAudio,
}

enum class VesperSystemPlaybackPermissionStatus {
    NotRequired,
    Granted,
    Denied,
}

enum class VesperSystemPlaybackControlKind {
    PlayPause,
    SeekBack,
    SeekForward,
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

data class VesperSystemPlaybackMetadata(
    val title: String,
    val artist: String? = null,
    val albumTitle: String? = null,
    val artworkUri: String? = null,
    val contentUri: String? = null,
    val durationMs: Long? = null,
    val isLive: Boolean = false,
)

data class VesperSystemPlaybackControlButton(
    val kind: VesperSystemPlaybackControlKind,
    val seekOffsetMs: Long? = null,
) {
    fun normalized(): VesperSystemPlaybackControlButton =
        when (kind) {
            VesperSystemPlaybackControlKind.PlayPause ->
                copy(seekOffsetMs = null)
            VesperSystemPlaybackControlKind.SeekBack,
            VesperSystemPlaybackControlKind.SeekForward,
            -> copy(seekOffsetMs = normalizedSeekOffsetMs)
        }

    val normalizedSeekOffsetMs: Long
        get() = (seekOffsetMs ?: DEFAULT_SYSTEM_PLAYBACK_SEEK_OFFSET_MS)
            .coerceIn(
                MIN_SYSTEM_PLAYBACK_SEEK_OFFSET_MS,
                MAX_SYSTEM_PLAYBACK_SEEK_OFFSET_MS,
            )

    companion object {
        fun playPause(): VesperSystemPlaybackControlButton =
            VesperSystemPlaybackControlButton(VesperSystemPlaybackControlKind.PlayPause)

        fun seekBack(offsetMs: Long = DEFAULT_SYSTEM_PLAYBACK_SEEK_OFFSET_MS): VesperSystemPlaybackControlButton =
            VesperSystemPlaybackControlButton(VesperSystemPlaybackControlKind.SeekBack, offsetMs)

        fun seekForward(offsetMs: Long = DEFAULT_SYSTEM_PLAYBACK_SEEK_OFFSET_MS): VesperSystemPlaybackControlButton =
            VesperSystemPlaybackControlButton(VesperSystemPlaybackControlKind.SeekForward, offsetMs)
    }
}

data class VesperSystemPlaybackControls(
    val compactButtons: List<VesperSystemPlaybackControlButton> = videoDefaultButtons(),
) {
    fun normalized(showSeekActions: Boolean = true): VesperSystemPlaybackControls {
        var buttons =
            compactButtons
                .take(MAX_SYSTEM_PLAYBACK_COMPACT_BUTTONS)
                .map { it.normalized() }
                .toMutableList()

        if (buttons.isEmpty()) {
            buttons = videoDefaultButtons().map { it.normalized() }.toMutableList()
        }
        if (buttons.size == MAX_SYSTEM_PLAYBACK_COMPACT_BUTTONS &&
            buttons[1].kind != VesperSystemPlaybackControlKind.PlayPause
        ) {
            buttons[1] = VesperSystemPlaybackControlButton.playPause()
        }
        if (buttons.none { it.kind == VesperSystemPlaybackControlKind.PlayPause }) {
            buttons = videoDefaultButtons().map { it.normalized() }.toMutableList()
        }
        if (!showSeekActions) {
            buttons.removeAll {
                it.kind == VesperSystemPlaybackControlKind.SeekBack ||
                    it.kind == VesperSystemPlaybackControlKind.SeekForward
            }
            if (buttons.isEmpty()) {
                buttons.add(VesperSystemPlaybackControlButton.playPause())
            }
        }

        return copy(compactButtons = buttons)
    }

    fun seekOffsetMs(kind: VesperSystemPlaybackControlKind): Long? =
        compactButtons
            .firstOrNull { it.kind == kind }
            ?.normalizedSeekOffsetMs

    companion object {
        fun videoDefault(): VesperSystemPlaybackControls =
            VesperSystemPlaybackControls(videoDefaultButtons())

        private fun videoDefaultButtons(): List<VesperSystemPlaybackControlButton> =
            listOf(
                VesperSystemPlaybackControlButton.seekBack(),
                VesperSystemPlaybackControlButton.playPause(),
                VesperSystemPlaybackControlButton.seekForward(),
            )
    }
}

data class VesperSystemPlaybackConfiguration(
    val enabled: Boolean = true,
    val backgroundMode: VesperBackgroundPlaybackMode = VesperBackgroundPlaybackMode.ContinueAudio,
    val showSystemControls: Boolean = true,
    val showSeekActions: Boolean = true,
    val metadata: VesperSystemPlaybackMetadata? = null,
    val controls: VesperSystemPlaybackControls = VesperSystemPlaybackControls.videoDefault(),
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
    fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration)
    fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata)
    fun clearSystemPlayback()
    fun drainBenchmarkEvents(): List<VesperBenchmarkEvent>
    fun benchmarkSummary(): VesperBenchmarkSummary
}
