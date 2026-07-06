package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExecutorCoroutineDispatcher
import kotlinx.coroutines.SupervisorJob
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.Executors

internal class VesperNativePlayerBridge(
    internal val bindings: VesperNativeBindings = MissingVesperNativeBindings(),
    internal val initialSource: VesperPlayerSource? = null,
    internal var currentResiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    internal var trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
    internal val preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    internal val decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
    internal val benchmarkRecorder: VesperBenchmarkRecorder = VesperBenchmarkRecorder(),
    internal var keepScreenOnDuringPlayback: Boolean = true,
    appContext: Context? = null,
    internal val surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
    internal val sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
        VesperSourceNormalizerConfiguration(),
    internal val frameProcessorConfiguration: VesperFrameProcessorConfiguration =
        VesperFrameProcessorConfiguration(),
    internal val nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(),
    internal val nativeFramePipelinePumpScheduler: NativeFramePipelinePumpScheduler =
        HandlerNativeFramePipelinePumpScheduler(),
) : PlayerBridge {
    internal var currentSource: VesperPlayerSource? = initialSource
    internal var hasInitializedSource = false
    internal val isDisposed = AtomicBoolean(false)
    internal var nativeUpdateEpoch = 0L
    internal var pendingAutoPlay = false
    internal val i18n = VesperPlayerI18n.fromContext(appContext)
    internal val mainHandler = Handler(Looper.getMainLooper())
    internal val nativeFramePipelineRuntimeLock = Any()
    internal val sourceLoadEpoch = AtomicLong(0L)
    internal val sourceLoadDispatcher: ExecutorCoroutineDispatcher =
        Executors.newFixedThreadPool(2) { runnable ->
            Thread(runnable, "vesper-source-load").apply {
                isDaemon = true
            }
        }.asCoroutineDispatcher()
    internal val sourceLoadScope = CoroutineScope(SupervisorJob() + sourceLoadDispatcher)

    internal val _uiState = MutableStateFlow(
        PlayerHostUiState(
            title = i18n.playerTitle(),
            subtitle = i18n.nativeBridgeReady(),
            sourceLabel = currentSource?.label ?: i18n.noSourceSelected(),
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
    internal val _trackCatalog = MutableStateFlow(VesperTrackCatalog.Empty)
    internal val _trackSelection = MutableStateFlow(VesperTrackSelectionSnapshot())
    internal val _effectiveVideoTrackId = MutableStateFlow<String?>(null)
    internal val _videoVariantObservation = MutableStateFlow<VesperVideoVariantObservation?>(null)
    internal val _resiliencePolicy = MutableStateFlow(currentResiliencePolicy)
    internal val surfaceHost = VesperNativeSurfaceHost(bindings, surfaceKind)
    @Volatile
    internal var nativeFramePipelineFallbackReason: String? = null
    internal var nativeFramePipelineRequiredFailure = false
    @Volatile
    internal var nativeFramePipelineOpenStatus: Map<String, Any?>? = null
    internal var nativeFramePipelineLastStatus: Map<String, Any?>? = null
    @Volatile
    internal var nativeFramePipelinePumpRunning = false
    @Volatile
    internal var nativeFramePipelinePumpEpoch = 0L
    internal var nativeFramePipelinePlaybackRequested = false
    internal var pendingTimedNativeFrame: TimedNativeFrameRelease? = null
    internal var nativeFramePipelineFirstFrameWatchdogStartedAtMs: Long? = null
    internal var nativeFramePipelineLastLoggedPumpKey: String? = null
    internal var nativeFramePipelineLastPublishedDiagnosticsKey: String? = null
    internal var nativeFramePipelineDiagnosticsDirty = false
    internal val runtimeWarnings = ArrayDeque<VesperRuntimeWarning>()

    internal companion object {
        const val MAX_RUNTIME_WARNINGS = 128
    }
    internal var currentPluginDiagnostics: List<Map<String, Any?>> =
        nativeFramePipelineDiagnostics()

    override val backend: PlayerBridgeBackend = PlayerBridgeBackend.VesperNativeStub
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
    override val pluginDiagnostics: List<Map<String, Any?>>
        get() {
            refreshNativeFramePipelineDiagnosticsIfDirty()
            return currentPluginDiagnostics
        }

    init {
        installNativeUpdateListener()
    }

    override fun initialize() = initializeNativeBridge()

    override suspend fun initializeAsync() = initializeNativeBridgeAsync()

    override fun dispose() = disposeNativeBridge()

    override fun refresh() = refreshNativeBridge()

    override fun selectSource(source: VesperPlayerSource) = selectNativeSource(source)

    override suspend fun selectSourceAsync(source: VesperPlayerSource) =
        selectNativeSourceAsync(source)

    override fun attachSurfaceHost(host: ViewGroup) = attachNativeSurfaceHost(host)

    override fun detachSurfaceHost(host: ViewGroup?) = detachNativeSurfaceHost(host)

    override fun play() = playNativeBridge()

    override fun pause() = pauseNativeBridge()

    override fun togglePause() = toggleNativePause()

    override fun stop() = stopNativeBridge()

    override fun seekBy(deltaMs: Long) = seekNativeBridgeBy(deltaMs)

    override fun seekToRatio(ratio: Float) = seekNativeBridgeToRatio(ratio)

    override fun seekToLiveEdge() = seekNativeBridgeToLiveEdge()

    override fun setPlaybackRate(rate: Float) = setNativePlaybackRate(rate)

    override fun setVideoTrackSelection(selection: VesperTrackSelection) =
        setNativeVideoTrackSelection(selection)

    override fun setAudioTrackSelection(selection: VesperTrackSelection) =
        setNativeAudioTrackSelection(selection)

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) =
        setNativeSubtitleTrackSelection(selection)

    override fun setAbrPolicy(policy: VesperAbrPolicy) = setNativeAbrPolicy(policy)

    override fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) =
        setNativeResiliencePolicy(policy)

    override fun setKeepScreenOnDuringPlayback(enabled: Boolean) =
        setNativeKeepScreenOnDuringPlayback(enabled)

    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) =
        configureNativeSystemPlayback(configuration)

    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) =
        updateNativeSystemPlaybackMetadata(metadata)

    override fun clearSystemPlayback() = clearNativeSystemPlayback()

    override fun pictureInPictureReadiness(): VesperPictureInPictureReadiness =
        pictureInPictureReadinessForNativeBridge()

    override fun drainBenchmarkEvents(): List<VesperBenchmarkEvent> =
        benchmarkRecorder.drainEvents()

    override fun drainRuntimeWarnings(): List<VesperRuntimeWarning> {
        if (runtimeWarnings.isEmpty()) {
            return emptyList()
        }
        val warnings = runtimeWarnings.toList()
        runtimeWarnings.clear()
        return warnings
    }

    override fun benchmarkSummary(): VesperBenchmarkSummary =
        benchmarkRecorder.summary()

}
