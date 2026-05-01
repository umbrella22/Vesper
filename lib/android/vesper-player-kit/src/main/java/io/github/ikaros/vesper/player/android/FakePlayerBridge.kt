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
    benchmarkConfiguration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
    appContext: Context? = null,
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource
    private val i18n = VesperPlayerI18n.fromContext(appContext)
    private val benchmarkRecorder = VesperBenchmarkRecorder(benchmarkConfiguration)

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
    private val _effectiveVideoTrackId = MutableStateFlow<String?>(null)
    private val _videoVariantObservation = MutableStateFlow<VesperVideoVariantObservation?>(null)
    private val _resiliencePolicy = MutableStateFlow(resiliencePolicy)

    override val backend: PlayerBridgeBackend = PlayerBridgeBackend.FakeDemo
    override val uiState: StateFlow<PlayerHostUiState> = _uiState.asStateFlow()
    override val trackCatalog: StateFlow<VesperTrackCatalog> = _trackCatalog.asStateFlow()
    override val trackSelection: StateFlow<VesperTrackSelectionSnapshot> =
        _trackSelection.asStateFlow()
    override val effectiveVideoTrackId: StateFlow<String?> =
        _effectiveVideoTrackId.asStateFlow()
    override val videoVariantObservation: StateFlow<VesperVideoVariantObservation?> =
        _videoVariantObservation.asStateFlow()
    override val resiliencePolicy: StateFlow<VesperPlaybackResiliencePolicy> =
        _resiliencePolicy.asStateFlow()

    override fun initialize() {
        recordBenchmark("initialize_start")
        if (currentSource == null) {
            recordBenchmark("initialize_without_source")
        } else {
            recordBenchmark("initialize_completed")
        }
    }

    override fun dispose() {
        recordBenchmark("dispose_command")
        benchmarkRecorder.dispose()
    }

    override fun refresh() = Unit

    override fun selectSource(source: VesperPlayerSource) {
        recordBenchmark(
            "select_source_start",
            mapOf("targetProtocol" to source.protocol.name.lowercase()),
        )
        currentSource = source
        _effectiveVideoTrackId.value = null
        _videoVariantObservation.value = null
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
        recordBenchmark("play_command")
        updateState {
            copy(playbackState = PlaybackStateUi.Playing, isBuffering = false)
        }
    }

    override fun pause() {
        recordBenchmark("pause_command")
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
        recordBenchmark("stop_command")
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
            recordBenchmark("seek_start", mapOf("positionMs" to target.toString()))
            copy(timeline = timeline.copy(positionMs = target))
        }
    }

    override fun seekToRatio(ratio: Float) {
        updateState {
            val timeline = timeline
            val position = timeline.positionForRatio(ratio)
            recordBenchmark("seek_start", mapOf("positionMs" to position.toString()))
            copy(timeline = timeline.copy(positionMs = position))
        }
    }

    override fun seekToLiveEdge() {
        updateState {
            val liveEdge = timeline.goLivePositionMs ?: timeline.positionMs
            recordBenchmark("seek_start", mapOf("positionMs" to liveEdge.toString()))
            copy(timeline = timeline.copy(positionMs = liveEdge))
        }
    }

    override fun setPlaybackRate(rate: Float) {
        recordBenchmark("set_playback_rate_command", mapOf("rate" to rate.toString()))
        updateState { copy(playbackRate = rate) }
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAudioTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAbrPolicy(policy: VesperAbrPolicy) = Unit

    override fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) {
        _resiliencePolicy.value = policy
    }

    override fun drainBenchmarkEvents(): List<VesperBenchmarkEvent> =
        benchmarkRecorder.drainEvents()

    override fun benchmarkSummary(): VesperBenchmarkSummary =
        benchmarkRecorder.summary()

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
    }

    private fun recordBenchmark(
        eventName: String,
        attributes: Map<String, String> = emptyMap(),
    ) {
        benchmarkRecorder.record(eventName, currentSource?.protocol, attributes)
    }

    private fun previewSourceSubtitle(source: VesperPlayerSource): String =
        i18n.previewSourceSubtitle(source)
}
