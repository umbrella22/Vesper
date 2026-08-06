package io.github.ikaros.vesper.player.android

import android.view.ViewGroup
import kotlinx.coroutines.flow.StateFlow
import kotlin.math.absoluteValue

private const val DEFAULT_SYSTEM_PLAYBACK_SEEK_OFFSET_MS = 10_000L
private const val MIN_SYSTEM_PLAYBACK_SEEK_OFFSET_MS = 1_000L
private const val MAX_SYSTEM_PLAYBACK_SEEK_OFFSET_MS = 60_000L
private const val MAX_SYSTEM_PLAYBACK_COMPACT_BUTTONS = 3

internal enum class PlayerBridgeBackend {
    FakeDemo,
    VesperNativeStub,
}

internal fun PlayerBridgeBackend.toBackendFamily(): VesperPlayerBackendFamily =
    when (this) {
        PlayerBridgeBackend.FakeDemo -> VesperPlayerBackendFamily.FakeDemo
        PlayerBridgeBackend.VesperNativeStub -> VesperPlayerBackendFamily.AndroidHostKit
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

enum class VesperPictureInPictureErrorCode {
    PictureInPictureNotSupported,
    PictureInPictureDisabledByHost,
    PictureInPictureSystemPlayerUnavailable,
    PictureInPictureSourceUnsupportedBySystemPlayer,
    PictureInPictureNativeFrameRouteCannotHandOff,
    PictureInPictureSurfaceUnavailable,
    PictureInPicturePlatformRequestRejected,
    PictureInPictureUnavailableForCurrentRoute,
}

data class VesperPictureInPictureError(
    val code: VesperPictureInPictureErrorCode,
    val message: String = "Current playback cannot enter Picture in Picture.",
    val userMessage: String = "Current playback cannot enter Picture in Picture.",
    val diagnostics: Map<String, Any?> = emptyMap(),
)

data class VesperPictureInPictureReadiness(
    val isAvailable: Boolean,
    val isActive: Boolean = false,
    val canAutoEnter: Boolean = false,
    val source: String = "system",
    val error: VesperPictureInPictureError? = null,
    val diagnostics: Map<String, Any?> = emptyMap(),
)

data class PlayerHostUiState(
    val title: String,
    val subtitle: String,
    val sourceLabel: String,
    val playbackState: PlaybackStateUi,
    val playbackRate: Float,
    val isBuffering: Boolean,
    val isInterrupted: Boolean,
    val timeline: TimelineUiState,
    val lastError: VesperPlayerErrorState? = null,
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

data class VesperRuntimeWarning(
    val domain: String,
    val payload: Map<String, Any?>,
)

internal interface PlayerBridge {
    val backend: PlayerBridgeBackend
    val uiState: StateFlow<PlayerHostUiState>
    val trackCatalog: StateFlow<VesperTrackCatalog>
    val trackSelection: StateFlow<VesperTrackSelectionSnapshot>
    /**
     * The latest subtitle selection requested by the host. This is kept
     * separate from the compatibility [trackSelection] snapshot because a
     * native player may take time to converge on the request.
     */
    val requestedSubtitleSelection: StateFlow<VesperTrackSelection>
    /**
     * The subtitle selection most recently confirmed by the native player.
     */
    val confirmedSubtitleSelection: StateFlow<VesperTrackSelection>
    /**
     * The native subtitle track id currently confirmed as effective.
     */
    val effectiveSubtitleTrackId: StateFlow<String?>
    /**
     * First-class subtitle lifecycle state. Mirrors the iOS
     * `publishedSubtitleState`. Driven by catalog refresh, structured JNI
     * failures, and source-switch reset
     * This state is emitted directly rather than derived from warnings.
     */
    val subtitleState: StateFlow<VesperSubtitleState>
    val effectiveVideoTrackId: StateFlow<String?>
    val videoVariantObservation: StateFlow<VesperVideoVariantObservation?>
    val resiliencePolicy: StateFlow<VesperPlaybackResiliencePolicy>
    val pluginDiagnostics: List<Map<String, Any?>>

    fun initialize()
    suspend fun initializeAsync()
    fun dispose()
    fun refresh()
    fun sampleTimeline(): TimelineUiState?
    fun selectSource(source: VesperPlayerSource)
    suspend fun selectSourceAsync(source: VesperPlayerSource)

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
    suspend fun setSubtitleTrackSelection(selection: VesperTrackSelection)
    fun setSubtitleStyle(style: VesperSubtitleStyle)
    fun setAbrPolicy(policy: VesperAbrPolicy)
    fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy)
    fun setKeepScreenOnDuringPlayback(enabled: Boolean)
    fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration)
    fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata)
    fun clearSystemPlayback()
    fun pictureInPictureReadiness(): VesperPictureInPictureReadiness
    fun drainRuntimeWarnings(): List<VesperRuntimeWarning>
    fun drainBenchmarkEvents(): List<VesperBenchmarkEvent>
    fun drainPipelineEventHookReports(): VesperPipelineEventHookReportBatch
    fun benchmarkSummary(): VesperBenchmarkSummary
    fun awaitBenchmarkSinkShutdown(timeoutMs: Long): Boolean
}
