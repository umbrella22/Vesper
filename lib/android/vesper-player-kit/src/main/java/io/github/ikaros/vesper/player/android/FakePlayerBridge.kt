package io.github.ikaros.vesper.player.android

import android.content.Context
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.core.view.isEmpty
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.atomic.AtomicBoolean

internal class FakePlayerBridge(
    initialSource: VesperPlayerSource? = null,
    resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
    preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    benchmarkConfiguration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
    private var keepScreenOnDuringPlayback: Boolean = true,
    appContext: Context? = null,
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource
    private var attachedHost: ViewGroup? = null
    private val i18n = VesperPlayerI18n.fromContext(appContext)
    private val benchmarkRecorder = VesperBenchmarkRecorder(benchmarkConfiguration)
    private val isDisposed = AtomicBoolean(false)

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
    override val pluginDiagnostics: List<Map<String, Any?>> = emptyList()

    override fun initialize() {
        if (isDisposed.get()) {
            return
        }
        recordBenchmark("initialize_start")
        if (currentSource == null) {
            recordBenchmark("initialize_without_source")
        } else {
            recordBenchmark("initialize_completed")
        }
    }

    override fun dispose() {
        if (!isDisposed.compareAndSet(false, true)) {
            return
        }
        recordBenchmark("dispose_command")
        attachedHost?.keepScreenOn = false
        attachedHost = null
        benchmarkRecorder.dispose()
    }

    override fun refresh() = Unit

    override fun selectSource(source: VesperPlayerSource) {
        if (isDisposed.get()) {
            return
        }
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
        if (isDisposed.get()) {
            return
        }
        attachedHost?.keepScreenOn = false
        attachedHost = host
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
        syncKeepScreenOn()
    }

    override fun detachSurfaceHost(host: ViewGroup?) {
        if (isDisposed.get()) {
            return
        }
        if (host != null && attachedHost !== host) {
            return
        }
        attachedHost?.keepScreenOn = false
        attachedHost = null
    }

    override fun play() {
        if (isDisposed.get()) {
            return
        }
        recordBenchmark("play_command")
        updateState {
            copy(playbackState = PlaybackStateUi.Playing, isBuffering = false)
        }
    }

    override fun pause() {
        if (isDisposed.get()) {
            return
        }
        recordBenchmark("pause_command")
        updateState { copy(playbackState = PlaybackStateUi.Paused, isBuffering = false) }
    }

    override fun togglePause() {
        if (isDisposed.get()) {
            return
        }
        when (_uiState.value.playbackState) {
            PlaybackStateUi.Playing -> pause()
            PlaybackStateUi.Ready,
            PlaybackStateUi.Paused,
            PlaybackStateUi.Finished,
            -> play()
        }
    }

    override fun stop() {
        if (isDisposed.get()) {
            return
        }
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
        if (isDisposed.get()) {
            return
        }
        updateState {
            val timeline = timeline
            val target = timeline.clampedPosition(timeline.positionMs + deltaMs)
            recordBenchmark("seek_start", mapOf("positionMs" to target.toString()))
            copy(timeline = timeline.copy(positionMs = target))
        }
    }

    override fun seekToRatio(ratio: Float) {
        if (isDisposed.get()) {
            return
        }
        updateState {
            val timeline = timeline
            val position = timeline.positionForRatio(ratio)
            recordBenchmark("seek_start", mapOf("positionMs" to position.toString()))
            copy(timeline = timeline.copy(positionMs = position))
        }
    }

    override fun seekToLiveEdge() {
        if (isDisposed.get()) {
            return
        }
        updateState {
            val liveEdge = timeline.goLivePositionMs ?: timeline.positionMs
            recordBenchmark("seek_start", mapOf("positionMs" to liveEdge.toString()))
            copy(timeline = timeline.copy(positionMs = liveEdge))
        }
    }

    override fun setPlaybackRate(rate: Float) {
        if (isDisposed.get()) {
            return
        }
        recordBenchmark("set_playback_rate_command", mapOf("rate" to rate.toString()))
        updateState { copy(playbackRate = rate) }
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAudioTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAbrPolicy(policy: VesperAbrPolicy) = Unit

    override fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) {
        if (isDisposed.get()) {
            return
        }
        _resiliencePolicy.value = policy
    }

    override fun setKeepScreenOnDuringPlayback(enabled: Boolean) {
        if (isDisposed.get()) {
            return
        }
        keepScreenOnDuringPlayback = enabled
        syncKeepScreenOn()
    }

    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) = Unit

    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) = Unit

    override fun clearSystemPlayback() = Unit

    override fun drainBenchmarkEvents(): List<VesperBenchmarkEvent> =
        benchmarkRecorder.drainEvents()

    override fun benchmarkSummary(): VesperBenchmarkSummary =
        benchmarkRecorder.summary()

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
        syncKeepScreenOn()
    }

    private fun syncKeepScreenOn() {
        attachedHost?.keepScreenOn =
            keepScreenOnDuringPlayback &&
            _uiState.value.playbackState == PlaybackStateUi.Playing
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
