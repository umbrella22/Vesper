package io.github.ikaros.vesper.player.android

import android.view.ViewGroup
import android.widget.FrameLayout
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class FakePlayerBridge(
    initialSource: VesperPlayerSource? = null,
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource

    private val _uiState = MutableStateFlow(
        PlayerHostUiState(
            title = "Vesper",
            subtitle = initialSource?.let(::previewSourceSubtitle) ?: "Android host preview bridge",
            sourceLabel = initialSource?.label ?: "No source selected",
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

    override val backend: PlayerBridgeBackend = PlayerBridgeBackend.FakeDemo
    override val uiState: StateFlow<PlayerHostUiState> = _uiState.asStateFlow()

    override fun initialize() = Unit

    override fun dispose() = Unit

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
        if (host.childCount == 0) {
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

    override fun detachSurfaceHost() = Unit

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
            val target = (timeline.positionMs + deltaMs).coerceIn(
                timeline.seekableRange?.startMs ?: 0L,
                timeline.seekableRange?.endMs ?: (timeline.durationMs ?: 0L),
            )
            copy(timeline = timeline.copy(positionMs = target))
        }
    }

    override fun seekToRatio(ratio: Float) {
        updateState {
            val timeline = timeline
            val range = timeline.seekableRange
            val position = if (range != null && range.endMs >= range.startMs) {
                val width = (range.endMs - range.startMs).toFloat()
                range.startMs + (width * ratio.coerceIn(0f, 1f)).toLong()
            } else {
                ((timeline.durationMs ?: 0L).toFloat() * ratio.coerceIn(0f, 1f)).toLong()
            }
            copy(timeline = timeline.copy(positionMs = position))
        }
    }

    override fun seekToLiveEdge() {
        updateState {
            val liveEdge = timeline.liveEdgeMs ?: timeline.seekableRange?.endMs ?: timeline.positionMs
            copy(timeline = timeline.copy(positionMs = liveEdge))
        }
    }

    override fun setPlaybackRate(rate: Float) {
        updateState { copy(playbackRate = rate) }
    }

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
    }
}

private fun previewSourceSubtitle(source: VesperPlayerSource): String =
    when (source.kind) {
        VesperPlayerSourceKind.Local -> "Android host preview bridge (local source)"
        VesperPlayerSourceKind.Remote ->
            "Android host preview bridge (${source.protocol.name.lowercase()} remote source)"
    }
