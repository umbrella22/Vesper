package io.github.ikaros.vesper.player.android

import android.content.Context
import android.view.ViewGroup
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

class VesperPlayerController internal constructor(
    private val bridge: PlayerBridge,
) {
    /**
     * Public backend family for diagnostics and federated wrapper snapshots.
     */
    val backendFamily: VesperPlayerBackendFamily
        get() = bridge.backend.toBackendFamily()

    val uiState: StateFlow<PlayerHostUiState>
        get() = bridge.uiState

    val trackCatalog: StateFlow<VesperTrackCatalog>
        get() = bridge.trackCatalog

    val trackSelection: StateFlow<VesperTrackSelectionSnapshot>
        get() = bridge.trackSelection

    /** Latest valid subtitle selection requested by the host. */
    val requestedSubtitleSelection: StateFlow<VesperTrackSelection>
        get() = bridge.requestedSubtitleSelection

    /** Latest subtitle selection confirmed by the native player. */
    val confirmedSubtitleSelection: StateFlow<VesperTrackSelection>
        get() = bridge.confirmedSubtitleSelection

    /** Native subtitle track id confirmed as effective, or `null` when disabled. */
    val effectiveSubtitleTrackId: StateFlow<String?>
        get() = bridge.effectiveSubtitleTrackId

    /**
     * First-class subtitle lifecycle state. Mirrors the iOS
     * `subtitleState` getter. Flutter observes this to render
     * loading / ready / failed states without coupling to the generic
     * `lastError` channel.
     */
    val subtitleState: StateFlow<VesperSubtitleState>
        get() = bridge.subtitleState

    val effectiveVideoTrackId: StateFlow<String?>
        get() = bridge.effectiveVideoTrackId

    val videoVariantObservation: StateFlow<VesperVideoVariantObservation?>
        get() = bridge.videoVariantObservation

    val resiliencePolicy: StateFlow<VesperPlaybackResiliencePolicy>
        get() = bridge.resiliencePolicy

    /**
     * Current subtitle styling (font scale, visibility). Hosts observe this to
     * drive a [androidx.media3.ui.SubtitleView] or equivalent overlay; it does
     * not flow through the player bridge because it only affects rendering.
     */
    private val _subtitleStyle = MutableStateFlow(VesperSubtitleStyle.Default)
    val subtitleStyle: StateFlow<VesperSubtitleStyle> = _subtitleStyle.asStateFlow()

    val pluginDiagnostics: List<Map<String, Any?>>
        get() = bridge.pluginDiagnostics

    /**
     * Enqueues player initialization and returns after the request is accepted.
     * Use [initializeAsync] when the caller needs to wait for source startup.
     */
    fun initialize() = bridge.initialize()

    /**
     * Initializes the current source and resumes only after startup work has completed.
     */
    suspend fun initializeAsync() = bridge.initializeAsync()

    fun dispose() = bridge.dispose()

    fun refresh() = bridge.refresh()

    fun sampleTimeline(): TimelineUiState? = bridge.sampleTimeline()

    /**
     * Enqueues source selection and returns after the request is accepted.
     * Use [selectSourceAsync] when the caller needs to wait for source startup.
     */
    fun selectSource(source: VesperPlayerSource) = bridge.selectSource(source)

    /**
     * Selects a source and resumes only after startup work has completed or failed.
     */
    suspend fun selectSourceAsync(source: VesperPlayerSource) = bridge.selectSourceAsync(source)

    fun attachSurfaceHost(host: ViewGroup) = bridge.attachSurfaceHost(host)

    fun detachSurfaceHost(host: ViewGroup? = null) = bridge.detachSurfaceHost(host)

    fun play() = bridge.play()

    fun pause() = bridge.pause()

    fun togglePause() = bridge.togglePause()

    fun stop() = bridge.stop()

    fun seekBy(deltaMs: Long) = bridge.seekBy(deltaMs)

    fun seekToRatio(ratio: Float) = bridge.seekToRatio(ratio)

    fun seekToLiveEdge() = bridge.seekToLiveEdge()

    fun setPlaybackRate(rate: Float) = bridge.setPlaybackRate(rate)

    fun setVideoTrackSelection(selection: VesperTrackSelection) =
        bridge.setVideoTrackSelection(selection)

    fun setAudioTrackSelection(selection: VesperTrackSelection) =
        bridge.setAudioTrackSelection(selection)

    /** Applies a subtitle selection and waits for native confirmation. */
    suspend fun setSubtitleTrackSelection(selection: VesperTrackSelection) =
        withContext(Dispatchers.Main.immediate) {
            bridge.setSubtitleTrackSelection(selection)
        }

    /**
     * Updates subtitle styling (font scale, visibility). Hosts observing
     * [subtitleStyle] should apply the new value to their subtitle view.
     */
    fun setSubtitleStyle(style: VesperSubtitleStyle) {
        bridge.setSubtitleStyle(style)
        _subtitleStyle.value = style
    }

    fun setAbrPolicy(
        policy: VesperAbrPolicy,
        expectedCatalogRevision: Long? = null,
    ) = bridge.setAbrPolicy(policy, expectedCatalogRevision)

    fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) =
        bridge.setResiliencePolicy(policy)

    fun setKeepScreenOnDuringPlayback(enabled: Boolean) =
        bridge.setKeepScreenOnDuringPlayback(enabled)

    fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) =
        bridge.configureSystemPlayback(configuration)

    fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) =
        bridge.updateSystemPlaybackMetadata(metadata)

    fun clearSystemPlayback() = bridge.clearSystemPlayback()

    fun pictureInPictureReadiness(): VesperPictureInPictureReadiness =
        bridge.pictureInPictureReadiness()

    fun drainRuntimeWarnings(): List<VesperRuntimeWarning> = bridge.drainRuntimeWarnings()

    fun drainBenchmarkEvents(): List<VesperBenchmarkEvent> = bridge.drainBenchmarkEvents()

    /** Returns and clears structured reports emitted by playback EventHooks. */
    fun drainPipelineEventHookReports(): VesperPipelineEventHookReportBatch =
        bridge.drainPipelineEventHookReports()

    fun benchmarkSummary(): VesperBenchmarkSummary = bridge.benchmarkSummary()

    /**
     * Waits for the benchmark sink's final flush after [dispose].
     *
     * The bounded wait runs off the caller thread so UI hosts can emit their
     * disposal state immediately and collect the final report afterward.
     */
    suspend fun awaitBenchmarkSinkShutdown(timeoutMs: Long): Boolean =
        withContext(Dispatchers.IO) {
            bridge.awaitBenchmarkSinkShutdown(timeoutMs)
        }

    companion object {
        val supportedPlaybackRates: List<Float> = listOf(0.5f, 1.0f, 1.5f, 2.0f, 3.0f)
    }
}

object VesperPlayerControllerFactory {
    fun probePlaybackCapability(
        context: Context,
        request: VesperPlaybackCapabilityProbeRequest,
    ): VesperPlaybackCapabilityProbeResult {
        VesperBundledPluginResolver.resolve(
            context = context.applicationContext,
            sourceNormalizerConfiguration = request.sourceNormalizerConfiguration,
            frameProcessorConfiguration = request.frameProcessorConfiguration,
            nativeFramePipelineConfiguration = request.nativeFramePipelineConfiguration,
        )
        return VesperPlaybackCapabilityProbe.probe(
            request,
            sessionProbeProvider =
                VesperAndroidDisplaySessionProbeProvider.fromContext(context.applicationContext),
        )
    }

    fun createDefault(
        context: Context,
        initialSource: VesperPlayerSource? = null,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
        surfaceKind: VesperVideoSurfaceKind = VesperVideoSurfaceKind.SurfaceView,
        keepScreenOnDuringPlayback: Boolean = true,
        benchmarkConfiguration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(),
        pipelineEventHookConfiguration: VesperPipelineEventHookConfiguration =
            VesperPipelineEventHookConfiguration(),
    ): VesperPlayerController =
        VesperPlayerController(
            PlayerBridgeFactory.createDefault(
                context = context,
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                trackPreferencePolicy = trackPreferencePolicy,
                preloadBudgetPolicy = preloadBudgetPolicy,
                decoderBackend = decoderBackend,
                surfaceKind = surfaceKind.toNativeSurfaceKind(),
                keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
                benchmarkConfiguration = benchmarkConfiguration,
                sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                frameProcessorConfiguration = frameProcessorConfiguration,
                nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
                pipelineEventHookConfiguration = pipelineEventHookConfiguration,
            )
        )

    fun createPreview(
        initialSource: VesperPlayerSource? = null,
        keepScreenOnDuringPlayback: Boolean = true,
        benchmarkConfiguration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
    ): VesperPlayerController {
        require(benchmarkConfiguration.pluginReferences.isEmpty()) {
            "Preview players do not have an Android Context for benchmark plugin references"
        }
        return VesperPlayerController(
            FakePlayerBridge(
                initialSource = initialSource,
                keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
                benchmarkConfiguration = benchmarkConfiguration,
            )
        )
    }
}
