package io.github.ikaros.vesper.player.android

import android.content.Context
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.core.view.isEmpty
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class FakePlayerBridge(
    initialSource: VesperPlayerSource? = null,
    resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
    preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    appContext: Context? = null,
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource
    private val i18n = VesperPlayerI18n.fromContext(appContext)

    private val _uiState = MutableStateFlow(
        PlayerHostUiState(
            title = i18n.playerTitle(),
            subtitle = initialSource?.let(::previewSourceSubtitle) ?: i18n.previewBridgeReady(),
            sourceLabel = initialSource?.label ?: i18n.noSourceSelected(),
            playbackState = PlaybackStateUi.Ready,
            playbackRate = 1.0f,
            isBuffering = false,
            isInterrupted = false,
            timeline = TimelineUiState(
                kind = TimelineKind.Vod,
                isSeekable = true,
                seekableRange = SeekableRangeUi(0L, 134_100L),
                liveEdgeMs = null,
                positionMs = 0L,
                durationMs = 134_100L,
            ),
        )
    )
    private val _trackCatalog = MutableStateFlow(VesperTrackCatalog.Empty)
    private val _trackSelection = MutableStateFlow(VesperTrackSelectionSnapshot())

    override val backend: PlayerBridgeBackend = PlayerBridgeBackend.FakeDemo
    override val uiState: StateFlow<PlayerHostUiState> = _uiState.asStateFlow()
    override val trackCatalog: StateFlow<VesperTrackCatalog> = _trackCatalog.asStateFlow()
    override val trackSelection: StateFlow<VesperTrackSelectionSnapshot> =
        _trackSelection.asStateFlow()

    override fun initialize() = Unit

    override fun dispose() = Unit

    override fun refresh() = Unit

    override fun selectSource(source: VesperPlayerSource) {
        currentSource = source
        updateState {
            copy(
                subtitle = previewSourceSubtitle(source),
                sourceLabel = source.label,
                playbackState = PlaybackStateUi.Ready,
                timeline = timeline.copy(positionMs = 0L),
                isBuffering = false,
            )
        }
    }

    override fun attachSurfaceHost(host: ViewGroup) {
        if (host.isEmpty()) {
            host.addView(
                FrameLayout(host.context).apply {
                    setBackgroundColor(0xFF000000.toInt())
                },
                ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                ),
            )
        }
    }

    override fun detachSurfaceHost(host: ViewGroup?) = Unit

    override fun play() {
        updateState {
            copy(playbackState = PlaybackStateUi.Playing, isBuffering = false)
        }
    }

    override fun pause() {
        updateState { copy(playbackState = PlaybackStateUi.Paused, isBuffering = false) }
    }

    override fun togglePause() {
        when (_uiState.value.playbackState) {
            PlaybackStateUi.Playing -> pause()
            PlaybackStateUi.Ready,
            PlaybackStateUi.Paused,
            PlaybackStateUi.Finished,
            -> play()
        }
    }

    override fun stop() {
        updateState {
            copy(
                playbackState = PlaybackStateUi.Ready,
                timeline = timeline.copy(positionMs = 0L),
                isBuffering = false,
            )
        }
    }

    override fun seekBy(deltaMs: Long) {
        updateState {
            val timeline = timeline
            val target = timeline.clampedPosition(timeline.positionMs + deltaMs)
            copy(timeline = timeline.copy(positionMs = target))
        }
    }

    override fun seekToRatio(ratio: Float) {
        updateState {
            val timeline = timeline
            val position = timeline.positionForRatio(ratio)
            copy(timeline = timeline.copy(positionMs = position))
        }
    }

    override fun seekToLiveEdge() {
        updateState {
            val liveEdge = timeline.goLivePositionMs ?: timeline.positionMs
            copy(timeline = timeline.copy(positionMs = liveEdge))
        }
    }

    override fun setPlaybackRate(rate: Float) {
        updateState { copy(playbackRate = rate) }
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAudioTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAbrPolicy(policy: VesperAbrPolicy) = Unit

    override fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) = Unit

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
    }

    private fun previewSourceSubtitle(source: VesperPlayerSource): String =
        i18n.previewSourceSubtitle(source)
}
