package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.view.Window
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExecutorCoroutineDispatcher
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

private const val SOURCE_LOAD_QUEUE_CAPACITY = 8
private const val DEFAULT_SOURCE_COMMAND_TIMEOUT_MS = 30_000L
private const val DEFAULT_SEEK_COMMAND_TIMEOUT_MS = 15_000L

internal class VesperNativePlayerBridge(
    internal val bindings: VesperNativeBindings = MissingVesperNativeBindings(),
    internal val initialSource: VesperPlayerSource? = null,
    internal var currentResiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    internal var trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
    internal val preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    internal val decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
    internal val benchmarkRecorder: VesperBenchmarkRecording = VesperBenchmarkCoordinator(),
    internal var keepScreenOnDuringPlayback: Boolean = true,
    appContext: Context? = null,
    internal val surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
    internal val sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
        VesperSourceNormalizerConfiguration(),
    internal val frameProcessorConfiguration: VesperFrameProcessorConfiguration =
        VesperFrameProcessorConfiguration(),
    internal val nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(),
    internal val pipelineEventHookRegistryOwner: VesperPluginRegistryHandleOwner? = null,
    internal val nativeFramePipelinePumpScheduler: NativeFramePipelinePumpScheduler =
        HandlerNativeFramePipelinePumpScheduler(),
    internal val sourceCommandTimeoutMs: Long = DEFAULT_SOURCE_COMMAND_TIMEOUT_MS,
    internal val seekCommandTimeoutMs: Long = DEFAULT_SEEK_COMMAND_TIMEOUT_MS,
) : PlayerBridge {
    override val appContext: Context? = appContext
    internal var currentSource: VesperPlayerSource? = initialSource
    internal var hasInitializedSource = false
    internal val isDisposed = AtomicBoolean(false)
    internal val disposeCleanupStarted = AtomicBoolean(false)
    internal var nativeUpdateEpoch = 0L
    internal var activeNativeItemEpoch: Long? = null
    internal var pendingAutoPlay = false
    internal val i18n = VesperPlayerI18n.fromContext(appContext)
    internal val mainHandler = Handler(Looper.getMainLooper())
    internal val nativeFramePipelineRuntimeLock = Any()
    internal val sourceLoadEpoch = AtomicLong(0L)
    internal val sourceCommandGeneration = AtomicLong(0L)
    internal val seekCommandGeneration = AtomicLong(0L)
    internal val pendingSourceCommand = AtomicReference<PendingSourceCommand?>(null)
    internal val sourceLoadDispatcher: ExecutorCoroutineDispatcher =
        ThreadPoolExecutor(
            2,
            2,
            0L,
            TimeUnit.MILLISECONDS,
            ArrayBlockingQueue(SOURCE_LOAD_QUEUE_CAPACITY),
            { runnable ->
                Thread(runnable, "vesper-source-load").apply {
                    isDaemon = true
                }
            },
            ThreadPoolExecutor.AbortPolicy(),
        ).asCoroutineDispatcher()
    internal val sourceLoadScope = CoroutineScope(SupervisorJob() + sourceLoadDispatcher)
    internal var sourceLoadJob: Job? = null
    /** Latest-wins token for queued source-load submissions. */
    @Volatile
    internal var sourceLoadRequestGeneration = 0L
    /** Monotonic source/item epoch used to invalidate stale subtitle callbacks. */
    internal var subtitleSourceEpoch = 0L
    internal var nextSubtitleCommandId = 0L
    internal var pendingSubtitleSelection: PendingSubtitleSelection? = null
    /** Selection mode last owned by the coordinator. Manual/disabled modes
     * freeze effective state against late native callbacks; auto mode remains
     * observable because the platform may legitimately choose another
     * effective track after confirmation. */
    internal var subtitleSelectionCoordinatorMode: VesperTrackSelectionMode? = null

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
    internal val _requestedSubtitleSelection =
        MutableStateFlow(VesperTrackSelection.disabled())
    internal val _confirmedSubtitleSelection =
        MutableStateFlow(VesperTrackSelection.disabled())
    internal val _effectiveSubtitleTrackId = MutableStateFlow<String?>(null)
    /**
     * First-class subtitle lifecycle state. Driven by catalog refresh
     * (ready/unavailable counts), structured JNI failures (failed), and
     * source-switch reset (empty). Exposed to the Flutter plugin so it
     * does not have to derive the state from catalog + drained warnings.
     */
    internal val _subtitleState = MutableStateFlow(VesperSubtitleState.EMPTY)
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
    internal var nativeFramePipelineParticipated = false
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
    internal var activeRetryStatusMessage: String? = null
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
    override val requestedSubtitleSelection: StateFlow<VesperTrackSelection> =
        _requestedSubtitleSelection.asStateFlow()
    override val confirmedSubtitleSelection: StateFlow<VesperTrackSelection> =
        _confirmedSubtitleSelection.asStateFlow()
    override val effectiveSubtitleTrackId: StateFlow<String?> =
        _effectiveSubtitleTrackId.asStateFlow()
    override val subtitleState: StateFlow<VesperSubtitleState> = _subtitleState.asStateFlow()
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
        if (bindings.isSystemPlaybackActive) {
            hasInitializedSource = true
            activeNativeItemEpoch = nativeUpdateEpoch
        }
        installNativeUpdateListener()
        bindings.setOnVideoLayoutInfoListener(surfaceHost::updateVideoLayout)
        bindings.setOnSubtitleCuesListener(surfaceHost::updateSubtitleCues)
        // Structured JNI track-selection failures (e.g. a stale subtitle id
        // arriving after a source refresh) surface as runtime warnings so
        // Flutter observes a `subtitle_track_not_found` warning alongside
        // the next snapshot.
        bindings.setOnTrackSelectionFailureListener { failure ->
            if (!hasInitializedSource || activeNativeItemEpoch != nativeUpdateEpoch) {
                return@setOnTrackSelectionFailureListener
            }
            val pendingSubtitle =
                if (failure.kind == NativeTrackKind.Subtitle) pendingSubtitleSelection else null
            val pendingAccepted =
                if (failure.kind == NativeTrackKind.Subtitle && pendingSubtitle != null) {
                    failPendingSubtitleSelection(failure)
                } else {
                    false
                }
            // Subtitle failures are command-scoped. A late or unassociated
            // callback must not affect the active transaction or warning/error
            // streams; the bounded transaction will time out if confirmation
            // never arrives.
            if (failure.kind == NativeTrackKind.Subtitle && !pendingAccepted) {
                return@setOnTrackSelectionFailureListener
            }
            synchronized(runtimeWarnings) {
                if (runtimeWarnings.size >= MAX_RUNTIME_WARNINGS) {
                    runtimeWarnings.removeFirst()
                }
                runtimeWarnings.add(
                    VesperRuntimeWarning(
                        domain = "io.github.umbrella22.vesper.player.trackSelection",
                        payload = mapOf(
                            "domain" to if (failure.kind == NativeTrackKind.Subtitle) "subtitle" else "track",
                            "kind" to failure.kind.name,
                            "trackId" to failure.trackId,
                            "code" to failure.code,
                            "phase" to failure.phase,
                            "retriable" to failure.retriable,
                            "message" to failure.message,
                        ),
                    ),
                )
            }
            // Mirror subtitle-domain failures into the first-class
            // subtitleState so the Flutter plugin can read it directly
            // instead of deriving from drained warnings.
            if (failure.kind == NativeTrackKind.Subtitle) {
                val current = _subtitleState.value
                val phase = VesperSubtitleErrorPhase.fromWire(failure.phase)
                _subtitleState.value = current.copy(
                    selectionState = VesperSubtitleSelectionState.Failed,
                    selectionError = VesperSubtitleError(
                        code = failure.code,
                        phase = phase,
                        phaseRawValue = failure.phase.takeIf {
                            phase == VesperSubtitleErrorPhase.Unknown
                        },
                        trackId = failure.trackId,
                        retriable = failure.retriable,
                        message = failure.message,
                        commandId = pendingSubtitle?.takeIf { pendingAccepted }?.commandId,
                        sourceEpoch = pendingSubtitle?.takeIf { pendingAccepted }?.sourceEpoch,
                    ),
                )
            }
        }
    }

    override fun initialize() = initializeNativeBridge()

    override suspend fun initializeAsync() = initializeNativeBridgeAsync()

    override fun dispose() = disposeNativeBridge()

    override fun refresh() = refreshNativeBridge()

    override fun sampleTimeline() = sampleTimelineNativeBridge()

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

    override suspend fun seekByAsync(deltaMs: Long) = seekNativeBridgeByAsync(deltaMs)

    override fun seekToRatio(ratio: Float) = seekNativeBridgeToRatio(ratio)

    override suspend fun seekToRatioAsync(ratio: Float) = seekNativeBridgeToRatioAsync(ratio)

    override fun seekToLiveEdge() = seekNativeBridgeToLiveEdge()

    override suspend fun seekToLiveEdgeAsync() = seekNativeBridgeToLiveEdgeAsync()

    override fun setPlaybackRate(rate: Float) = setNativePlaybackRate(rate)

    override fun setVideoTrackSelection(selection: VesperTrackSelection) =
        setNativeVideoTrackSelection(selection)

    override fun setAudioTrackSelection(selection: VesperTrackSelection) =
        setNativeAudioTrackSelection(selection)

    override suspend fun setSubtitleTrackSelection(selection: VesperTrackSelection) =
        setNativeSubtitleTrackSelection(selection)


    override fun setSubtitleStyle(style: VesperSubtitleStyle) {
        surfaceHost.updateSubtitleStyle(style)
    }

    override fun setAbrPolicy(
        policy: VesperAbrPolicy,
        expectedCatalogRevision: Long?,
    ) = setNativeAbrPolicy(policy, expectedCatalogRevision)

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

    override fun drainPipelineEventHookReports(): VesperPipelineEventHookReportBatch =
        bindings.drainPipelineEventHookReports()

    override fun drainRuntimeWarnings(): List<VesperRuntimeWarning> {
        synchronized(runtimeWarnings) {
            if (runtimeWarnings.isEmpty()) {
                return emptyList()
            }
            val warnings = runtimeWarnings.toList()
            runtimeWarnings.clear()
            return warnings
        }
    }

    override fun benchmarkSummary(): VesperBenchmarkSummary =
        benchmarkRecorder.summary()

    override fun awaitBenchmarkSinkShutdown(timeoutMs: Long): Boolean =
        benchmarkRecorder.awaitSinkShutdown(timeoutMs)

    override fun startPerformanceDiagnostics(
        configuration: VesperPerformanceDiagnosticsConfiguration,
        probe: VesperPerformanceProbe,
        window: Window?,
    ): String = performanceCoordinator().startPerformance(
        configuration,
        probe,
        window,
        initialPlaybackActive = _uiState.value.playbackState == PlaybackStateUi.Playing &&
            !_uiState.value.isBuffering,
    )

    override fun updatePerformanceOverlayState(
        runId: String,
        state: VesperPerformanceOverlayState,
    ) = performanceCoordinator().updateOverlayState(runId, state)

    override fun recordPerformanceMarker(
        runId: String,
        name: String,
        value: Double?,
        sequenceIndex: Int?,
        expectedOverlayActive: Boolean?,
    ) = performanceCoordinator().recordMarker(
        runId,
        name,
        value,
        sequenceIndex,
        expectedOverlayActive,
    )

    override fun submitPerformanceFrameSamples(
        runId: String,
        samples: List<VesperPerformanceFrameSample>,
    ) = performanceCoordinator().recordPerformanceFrames(runId, samples)

    override fun performanceDiagnosticsSnapshot(runId: String): VesperPerformanceDiagnosticsReport =
        performanceCoordinator().snapshot(runId)

    override fun stopPerformanceDiagnostics(runId: String): VesperPerformanceDiagnosticsReport =
        performanceCoordinator().stop(runId)

    private fun performanceCoordinator(): VesperBenchmarkCoordinator =
        benchmarkRecorder as? VesperBenchmarkCoordinator
            ?: throw VesperPerformanceDiagnosticsException(
                VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable,
                "The injected benchmark recorder does not support performance diagnostics.",
            )

}

internal data class PendingSourceCommand(
    val commandId: Long,
    val source: VesperPlayerSource,
    val startedAtNs: Long = System.nanoTime(),
    val completion: CompletableDeferred<Unit> = CompletableDeferred(),
    val job: AtomicReference<Job?> = AtomicReference(null),
)
