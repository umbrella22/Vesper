package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import android.view.ViewGroup
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.absoluteValue

internal class VesperNativePlayerBridge(
    private val bindings: VesperNativeBindings = MissingVesperNativeBindings(),
    private val initialSource: VesperPlayerSource? = null,
    private var currentResiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    private var trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
    private val preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    private val decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
    private val benchmarkRecorder: VesperBenchmarkRecorder = VesperBenchmarkRecorder(),
    private var keepScreenOnDuringPlayback: Boolean = true,
    appContext: Context? = null,
    private val surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
    private val sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
        VesperSourceNormalizerConfiguration(),
    private val frameProcessorConfiguration: VesperFrameProcessorConfiguration =
        VesperFrameProcessorConfiguration(),
    private val nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(),
    private val nativeFramePipelinePumpScheduler: NativeFramePipelinePumpScheduler =
        HandlerNativeFramePipelinePumpScheduler(),
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource
    private var hasInitializedSource = false
    private val isDisposed = AtomicBoolean(false)
    private var nativeUpdateEpoch = 0L
    private var pendingAutoPlay = false
    private val i18n = VesperPlayerI18n.fromContext(appContext)
    private val mainHandler = Handler(Looper.getMainLooper())
    private val nativeFramePipelineRuntimeLock = Any()

    private val _uiState = MutableStateFlow(
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
    private val _trackCatalog = MutableStateFlow(VesperTrackCatalog.Empty)
    private val _trackSelection = MutableStateFlow(VesperTrackSelectionSnapshot())
    private val _effectiveVideoTrackId = MutableStateFlow<String?>(null)
    private val _videoVariantObservation = MutableStateFlow<VesperVideoVariantObservation?>(null)
    private val _resiliencePolicy = MutableStateFlow(currentResiliencePolicy)
    private val surfaceHost = VesperNativeSurfaceHost(bindings, surfaceKind)
    @Volatile
    private var nativeFramePipelineFallbackReason: String? = null
    private var nativeFramePipelineRequiredFailure = false
    @Volatile
    private var nativeFramePipelineOpenStatus: Map<String, Any?>? = null
    private var nativeFramePipelineLastStatus: Map<String, Any?>? = null
    @Volatile
    private var nativeFramePipelinePumpRunning = false
    @Volatile
    private var nativeFramePipelinePumpEpoch = 0L
    private var nativeFramePipelinePlaybackRequested = false
    private var pendingTimedNativeFrame: TimedNativeFrameRelease? = null
    private var nativeFramePipelineFirstFrameWatchdogStartedAtMs: Long? = null
    private var nativeFramePipelineLastLoggedPumpKey: String? = null
    private var nativeFramePipelineLastPublishedDiagnosticsKey: String? = null
    private var nativeFramePipelineDiagnosticsDirty = false
    private val runtimeWarnings = ArrayDeque<VesperRuntimeWarning>()
    private var currentPluginDiagnostics: List<Map<String, Any?>> =
        initialSource?.let(::probePluginsForSource)
            ?: nativeFramePipelineDiagnostics()

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

    override fun initialize() {
        if (isDisposed.get()) {
            return
        }
        recordBenchmark("initialize_start")
        val source = currentSource ?: run {
            recordBenchmark("initialize_without_source")
            clearTrackState()
            updateState {
                copy(
                    subtitle = i18n.selectSourcePrompt(),
                    sourceLabel = i18n.noSourceSelected(),
                    playbackState = PlaybackStateUi.Ready,
                    isBuffering = false,
                )
            }
            return
        }

        currentPluginDiagnostics = probePluginsForSource(source)
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        closeNativeFramePipelineOnRuntime()
        nativeFramePipelineOpenStatus = null
        nativeFramePipelineLastStatus = null
        clearPendingTimedNativeFrameFromRuntime()
        nativeFramePipelinePlaybackRequested = false
        resetNativeFramePipelineRuntimeMarkers()
        val nativeFrameDecision = evaluateNativeFramePipelineRoute()
        Log.i(
            TAG,
            "native-frame route decision=${nativeFrameRouteLogLabel(nativeFrameDecision)} " +
                "mode=${nativeFramePipelineConfiguration.mode} surface=$surfaceKind " +
                "sourceNormalizerPlugins=${sourceNormalizerConfiguration.pluginLibraryPaths.size} " +
                "decoderPlugins=${nativeFramePipelineConfiguration.decoderPluginLibraryPaths.size} " +
                "frameProcessors=${nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths.size}",
        )
        when (nativeFrameDecision) {
            NativeFramePipelineRoute.SystemPlayer -> Unit
            is NativeFramePipelineRoute.Fallback -> {
                Log.i(TAG, "native-frame pipeline fallback: ${nativeFrameDecision.reason}")
            }
            is NativeFramePipelineRoute.Fail -> {
                recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to nativeFrameDecision.reason))
                hasInitializedSource = false
                pendingAutoPlay = false
                clearTrackState()
                updateState {
                    copy(
                        subtitle = i18n.stubError(nativeFrameDecision.reason),
                        sourceLabel = source.label,
                    )
                }
                return
            }
            NativeFramePipelineRoute.NativeFrame -> {
                recordBenchmark("native_frame_pipeline_selected")
            }
        }
        advanceNativeUpdateEpoch()
        runCatching {
            bindings.initialize(
                source,
                currentResiliencePolicy,
                trackPreferencePolicy,
                systemPlaybackUsesSourceNormalizerResource =
                    nativeFrameDecision != NativeFramePipelineRoute.NativeFrame,
                systemPlaybackVideoEnabled =
                    nativeFrameDecision != NativeFramePipelineRoute.NativeFrame,
            )
        }
            .onSuccess {
                if (nativeFrameDecision == NativeFramePipelineRoute.NativeFrame &&
                    !openNativeFramePipelineAfterSystemStartup(source, it.pluginDiagnostics)
                ) {
                    return@onSuccess
                }
                if (it.pluginDiagnostics.isNotEmpty() || !nativeFramePipelineConfiguration.isDisabled) {
                    currentPluginDiagnostics =
                        pluginDiagnosticsWithNativeFramePipeline(it.pluginDiagnostics)
                }
                recordBenchmark("initialize_completed")
                hasInitializedSource = true
                Log.i(
                    TAG,
                    "initialized source=${source.uri} label=${source.label} kind=${source.kind} protocol=${source.protocol} decoderBackend=$decoderBackend",
                )
                surfaceHost.reattachIfAvailable()
                val shouldAutoPlay = pendingAutoPlay
                pendingAutoPlay = false
                if (shouldAutoPlay) {
                    Log.i(TAG, "auto-playing selected source=${source.uri}")
                    bindings.play()
                    nativeFramePipelinePlaybackRequested = true
                    updateState { copy(playbackState = PlaybackStateUi.Playing, isBuffering = false) }
                    startNativeFramePipelinePump("autoplay")
                }
                updateState {
                    copy(
                        subtitle = it.subtitle ?: sourceSubtitle(source),
                        sourceLabel = source.label,
                    )
                }
                refreshFromNative()
            }
            .onFailure {
                recordBenchmark(
                    "initialize_failed",
                    mapOf("error" to (it.message ?: it::class.java.simpleName)),
                )
                hasInitializedSource = false
                pendingAutoPlay = false
                clearTrackState()
                Log.e(TAG, "failed to initialize source=${source.uri}", it)
                val message = it.message?.takeUnless(String::isBlank) ?: i18n.nativeBindingsUnavailable()
                updateState {
                    copy(
                        subtitle = i18n.stubError(message),
                        sourceLabel = source.label,
                    )
                }
            }
    }

    override fun dispose() {
        if (!isDisposed.compareAndSet(false, true)) {
            return
        }
        advanceNativeUpdateEpoch(clearListener = true)
        hasInitializedSource = false
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        closeNativeFramePipelineOnRuntime()
        nativeFramePipelinePumpScheduler.close()
        clearTrackState()
        nativeFramePipelineOpenStatus = null
        nativeFramePipelineLastStatus = null
        clearPendingTimedNativeFrameFromRuntime()
        nativeFramePipelinePlaybackRequested = false
        resetNativeFramePipelineRuntimeMarkers()
        bindings.clearSystemPlayback()
        surfaceHost.setKeepScreenOn(false)
        surfaceHost.detach()
        bindings.dispose()
        recordBenchmark("dispose_command")
        benchmarkRecorder.dispose()
    }

    override fun refresh() {
        if (isDisposed.get()) {
            return
        }
        bindings.refreshSnapshot()
        refreshFromNative()
    }

    override fun selectSource(source: VesperPlayerSource) {
        if (isDisposed.get()) {
            return
        }
        recordBenchmark(
            "select_source_start",
            mapOf("targetProtocol" to source.protocol.name.lowercase()),
        )
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        closeNativeFramePipelineOnRuntime()
        nativeFramePipelineOpenStatus = null
        nativeFramePipelineLastStatus = null
        clearPendingTimedNativeFrameFromRuntime()
        resetNativeFramePipelineRuntimeMarkers()
        currentSource = source
        pendingAutoPlay = true
        clearTrackState()
        Log.i(
            TAG,
            "selecting source=${source.uri} label=${source.label} kind=${source.kind} protocol=${source.protocol}",
        )
        updateState {
            copy(
                subtitle = i18n.openingSource(source.label),
                sourceLabel = source.label,
                playbackState = PlaybackStateUi.Ready,
                isBuffering = true,
                timeline = timeline.copy(positionMs = 0L),
            )
        }
        initialize()
    }

    private fun probePluginsForSource(source: VesperPlayerSource): List<Map<String, Any?>> {
        if (
            sourceNormalizerConfiguration.isDisabled &&
                frameProcessorConfiguration.isDisabled &&
                nativeFramePipelineConfiguration.isDisabled
        ) {
            return emptyList()
        }
        val pluginDiagnostics = runCatching {
            bindings.probeMobilePlugins(
                source = source,
                sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                frameProcessorConfiguration = frameProcessorConfiguration,
            )
        }.onFailure { error ->
            Log.w(TAG, "mobile plugin diagnostics failed for source=${source.uri}", error)
        }.getOrDefault(emptyList())
        return pluginDiagnosticsWithNativeFramePipeline(pluginDiagnostics)
    }

    private fun pluginDiagnosticsWithNativeFramePipeline(
        pluginDiagnostics: List<Map<String, Any?>>,
    ): List<Map<String, Any?>> {
        val withoutNativeFrame =
            pluginDiagnostics.filter { diagnostic ->
                diagnostic["pluginKind"] != "native_frame_pipeline"
            }
        return withoutNativeFrame + nativeFramePipelineDiagnostics()
    }

    private fun nativeFramePipelineDiagnostics(): List<Map<String, Any?>> {
        if (nativeFramePipelineConfiguration.isDisabled) {
            return emptyList()
        }
        val participation =
            if (nativeFramePipelineFallbackReason != null) {
                if (nativeFramePipelineRequiredFailure) "selected" else "fallback"
            } else {
                when (nativeFramePipelineConfiguration.mode) {
                    VesperNativeFramePipelineMode.PreferNativeFrame,
                    VesperNativeFramePipelineMode.RequireNativeFrame -> "selected"
                    VesperNativeFramePipelineMode.Disabled,
                    VesperNativeFramePipelineMode.DiagnosticsOnly -> "available"
                }
            }
        val route =
            when (nativeFramePipelineConfiguration.mode) {
                VesperNativeFramePipelineMode.Disabled,
                VesperNativeFramePipelineMode.DiagnosticsOnly -> "systemPlayer"
                VesperNativeFramePipelineMode.PreferNativeFrame,
                VesperNativeFramePipelineMode.RequireNativeFrame ->
                    if (
                        nativeFramePipelineFallbackReason == null ||
                        nativeFramePipelineRequiredFailure
                    ) {
                        "sdkManagedNativeFrame"
                    } else {
                        "systemPlayer"
                    }
            }
        val status = if (nativeFramePipelineFallbackReason == null) "loaded" else "unsupported"
        val message =
            when (nativeFramePipelineConfiguration.mode) {
                VesperNativeFramePipelineMode.Disabled ->
                    "Mobile native-frame pipeline is disabled; system player remains selected."
                VesperNativeFramePipelineMode.DiagnosticsOnly ->
                    "Mobile native-frame pipeline diagnostics are enabled; playback still uses the system player."
                VesperNativeFramePipelineMode.PreferNativeFrame ->
                    "Mobile native-frame pipeline is explicitly preferred; Android MediaCodec release-to-surface lane is selected when available."
                VesperNativeFramePipelineMode.RequireNativeFrame ->
                    "Mobile native-frame pipeline is explicitly required; Android MediaCodec release-to-surface lane must be available."
            }
        val resolvedMessage =
            nativeFramePipelineFallbackReason?.let {
                val failureLabel =
                    if (nativeFramePipelineRequiredFailure) "Failure reason" else "Fallback reason"
                "$message $failureLabel: $it"
            } ?: nativeFramePipelineLastStatus?.get("message")?.toString()?.takeIf(String::isNotBlank)?.let {
                "$message Native-frame lifecycle is open; advance currently reports: $it."
            } ?: nativeFramePipelineOpenStatus?.let {
                "$message Native-frame lifecycle is open; packet decode and release-to-surface presentation are active while playback is running."
            } ?: message
        val counters = nativeFramePipelineCounters()
        return listOf(
            mutableMapOf<String, Any?>(
                "path" to
                    (
                        nativeFramePipelineConfiguration.decoderPluginLibraryPaths +
                            nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths
                    ).joinToString(separator = java.io.File.pathSeparator),
                "pluginName" to "vesper-android-native-frame-pipeline",
                "pluginKind" to "native_frame_pipeline",
                "status" to status,
                "message" to
                    "$resolvedMessage decoderPlugins=${nativeFramePipelineConfiguration.decoderPluginLibraryPaths.size}; " +
                        "frameProcessors=${nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths.size}; " +
                        "maxInFlightFrames=${nativeFramePipelineConfiguration.maxInFlightFrames ?: "default"}",
                "participation" to participation,
                "route" to route,
                "sourceInput" to "sourceNormalizerPacket",
                "decoderAdapter" to "MediaCodec",
                "presenterProfile" to
                    (
                        nativeFramePipelineOpenStatus?.get("presenterProfile")?.toString()
                            ?: nativeFramePresenterProfileName()
                    ),
                "presenterReady" to nativeFramePipelineBooleanValue("presenterReady"),
                "presenterConfigured" to nativeFramePipelineBooleanValue("presenterConfigured"),
                "presenterState" to nativeFramePipelineStringValue("presenterState"),
                "surfaceAttached" to nativeFramePipelineBooleanValue("surfaceAttached"),
                "surfaceProfile" to nativeFramePipelineStringValue("surfaceProfile"),
                "pipelineProfile" to
                    (
                        nativeFramePipelineStringValue("pipelineProfile")
                            ?: "media_codec_surface_texture"
                    ),
                "pumpRunning" to nativeFramePipelinePumpRunning,
                "decodedFrames" to counters.longValue("decodedFrames"),
                "processedFrames" to counters.longValue("processedFrames"),
                "presenterSubmitCount" to counters.longValue("presenterSubmitCount"),
                "presentedFrames" to counters.longValue("presentedFrames"),
                "deadlineMisses" to counters.longValue("deadlineMisses"),
                "backpressureCount" to counters.longValue("backpressureCount"),
                "lateDropped" to counters.longValue("lateDropped"),
                "lifecycle" to
                    when {
                        nativeFramePipelineRequiredFailure -> "failed"
                        nativeFramePipelineFallbackReason != null -> "fallback"
                        nativeFramePipelineOpenStatus != null -> "open"
                        else -> "notOpened"
                    },
                "lastAdvanceStatus" to nativeFramePipelineLastStatus?.get("status"),
                "fallbackTargetRoute" to
                    if (
                        nativeFramePipelineFallbackReason == null ||
                        nativeFramePipelineRequiredFailure
                    ) {
                        null
                    } else {
                        "systemPlayer"
                    },
                "fallbackReason" to nativeFramePipelineFallbackReason,
            )
        )
    }

    private fun evaluateNativeFramePipelineRoute(): NativeFramePipelineRoute {
        return when (nativeFramePipelineConfiguration.mode) {
            VesperNativeFramePipelineMode.Disabled,
            VesperNativeFramePipelineMode.DiagnosticsOnly -> {
                nativeFramePipelineFallbackReason = null
                nativeFramePipelineRequiredFailure = false
                NativeFramePipelineRoute.SystemPlayer
            }
            VesperNativeFramePipelineMode.PreferNativeFrame,
            VesperNativeFramePipelineMode.RequireNativeFrame -> {
                val reason = nativeFramePipelineUnavailableReason()
                if (reason == null) {
                    nativeFramePipelineFallbackReason = null
                    nativeFramePipelineRequiredFailure = false
                    currentPluginDiagnostics = probePluginsForSource(currentSource ?: return NativeFramePipelineRoute.SystemPlayer)
                    NativeFramePipelineRoute.NativeFrame
                } else {
                    nativeFramePipelineFallbackReason = reason
                    nativeFramePipelineRequiredFailure =
                        nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
                    currentSource?.let { currentPluginDiagnostics = probePluginsForSource(it) }
                    if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
                        NativeFramePipelineRoute.Fail(reason)
                    } else {
                        NativeFramePipelineRoute.Fallback(reason)
                    }
                }
            }
        }
    }

    private fun nativeFramePipelineUnavailableReason(): String? {
        if (nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty()) {
            return "Android native-frame pipeline requires a MediaCodec decoder plugin path."
        }
        if (sourceNormalizerConfiguration.pluginLibraryPaths.isEmpty()) {
            return "Android native-frame pipeline requires a SourceNormalizer packet-stream plugin path."
        }
        if (surfaceKind != NativeVideoSurfaceKind.SurfaceView) {
            return "Android native-frame pipeline currently supports SurfaceView only; TextureView falls back to system playback."
        }
        return null
    }

    private sealed interface NativeFramePipelineRoute {
        data object SystemPlayer : NativeFramePipelineRoute
        data class Fallback(val reason: String) : NativeFramePipelineRoute
        data class Fail(val reason: String) : NativeFramePipelineRoute
        data object NativeFrame : NativeFramePipelineRoute
    }

    private fun nativeFrameRouteLogLabel(route: NativeFramePipelineRoute): String =
        when (route) {
            NativeFramePipelineRoute.SystemPlayer -> "systemPlayer"
            is NativeFramePipelineRoute.Fallback -> "fallback:${route.reason}"
            is NativeFramePipelineRoute.Fail -> "fail:${route.reason}"
            NativeFramePipelineRoute.NativeFrame -> "sdkManagedNativeFrame"
        }

    override fun attachSurfaceHost(host: ViewGroup) {
        recordBenchmark("attach_surface_host")
        surfaceHost.updateVideoLayout(bindings.currentVideoLayoutInfo())
        surfaceHost.attach(host)
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        syncNativeFramePipelineSurfaceDiagnostics()
        resetNativeFramePipelineFirstFrameWatchdogIfDetached()
        syncNativeFramePipelinePumpWithPlaybackState()
        refreshFromNative()
    }

    override fun detachSurfaceHost(host: ViewGroup?) {
        recordBenchmark("detach_surface_host")
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        if (isRequiredNativeFramePipelineFailureActive()) {
            surfaceHost.detachWithoutNativeNotification(host)
            return
        }
        surfaceHost.detach(host)
        syncNativeFramePipelineSurfaceDiagnostics()
        resetNativeFramePipelineFirstFrameWatchdogIfDetached()
    }

    override fun play() {
        recordBenchmark("play_command")
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        if (_uiState.value.playbackState == PlaybackStateUi.Finished) {
            restartNativeFramePipelineFromBeginning()
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
        }
        bindings.play()
        nativeFramePipelinePlaybackRequested = true
        updateState { copy(playbackState = PlaybackStateUi.Playing, isBuffering = false) }
        startNativeFramePipelinePump("play")
        refreshFromNative()
    }

    override fun pause() {
        recordBenchmark("pause_command")
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.pause()
        nativeFramePipelinePlaybackRequested = false
        updateState { copy(playbackState = PlaybackStateUi.Paused, isBuffering = false) }
        refreshFromNative()
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
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        stopNativeFramePipelinePump()
        nativeFramePipelinePlaybackRequested = false
        bindings.stop()
        flushNativeFramePipeline()
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        updateState {
            copy(
                playbackState = PlaybackStateUi.Ready,
                timeline = timeline.copy(positionMs = 0L),
                isBuffering = false,
            )
        }
        refreshFromNative()
    }

    override fun seekBy(deltaMs: Long) {
        val current = _uiState.value.timeline
        val target = current.clampedPosition(current.positionMs + deltaMs)
        recordBenchmark("seek_start", mapOf("positionMs" to target.toString()))
        if (!seekBindingsTo(target)) {
            return
        }
        updateState { copy(timeline = timeline.copy(positionMs = target)) }
        refreshFromNative()
    }

    override fun seekToRatio(ratio: Float) {
        val timeline = _uiState.value.timeline
        val position = timeline.positionForRatio(ratio)
        recordBenchmark("seek_start", mapOf("positionMs" to position.toString()))
        if (!seekBindingsTo(position)) {
            return
        }
        updateState { copy(timeline = timeline.copy(positionMs = position)) }
        refreshFromNative()
    }

    override fun seekToLiveEdge() {
        val timeline = _uiState.value.timeline
        val liveEdge = timeline.goLivePositionMs ?: return
        recordBenchmark("seek_start", mapOf("positionMs" to liveEdge.toString()))
        if (!seekBindingsTo(liveEdge)) {
            return
        }
        updateState { copy(timeline = timeline.copy(positionMs = liveEdge)) }
        refreshFromNative()
    }

    override fun setPlaybackRate(rate: Float) {
        recordBenchmark("set_playback_rate_command", mapOf("rate" to rate.toString()))
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.setPlaybackRate(rate)
        updateState { copy(playbackRate = rate) }
        reschedulePendingTimedNativeFrameForCurrentRate()
        refreshFromNative()
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        recordBenchmark("set_video_track_selection_command", mapOf("mode" to selection.mode.name))
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.setVideoTrackSelection(selection)
        refreshFromNative()
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        recordBenchmark("set_audio_track_selection_command", mapOf("mode" to selection.mode.name))
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.setAudioTrackSelection(selection)
        refreshFromNative()
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        recordBenchmark("set_subtitle_track_selection_command", mapOf("mode" to selection.mode.name))
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.setSubtitleTrackSelection(selection)
        refreshFromNative()
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        recordBenchmark("set_abr_policy_command", mapOf("mode" to policy.mode.name))
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.setAbrPolicy(policy)
        refreshFromNative()
    }

    override fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) {
        if (currentResiliencePolicy == policy) {
            return
        }

        currentResiliencePolicy = policy
        _resiliencePolicy.value = policy
        recordBenchmark("set_resilience_policy_command")
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        val source = currentSource ?: return
        if (!hasInitializedSource) {
            return
        }

        val preservedState = PreservedPlaybackState.capture(
            uiState = _uiState.value,
            trackSelection = _trackSelection.value,
        )

        Log.i(
            TAG,
            "apply resilience policy buffering=${policy.buffering.preset} retry=${policy.retry.backoff} cache=${policy.cache.preset}",
        )
        updateState { copy(isBuffering = true) }
        initialize()
        restorePlaybackState(source, preservedState)
    }

    override fun setKeepScreenOnDuringPlayback(enabled: Boolean) {
        keepScreenOnDuringPlayback = enabled
        syncKeepScreenOn()
    }

    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) {
        if (isDisposed.get() || isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.configureSystemPlayback(configuration)
        refreshFromNative()
    }

    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) {
        if (isDisposed.get() || isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.updateSystemPlaybackMetadata(metadata)
        refreshFromNative()
    }

    override fun clearSystemPlayback() {
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.clearSystemPlayback()
    }

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

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
        syncKeepScreenOn()
    }

    private fun syncKeepScreenOn() {
        surfaceHost.setKeepScreenOn(
            !isDisposed.get() &&
                keepScreenOnDuringPlayback &&
                _uiState.value.playbackState == PlaybackStateUi.Playing,
        )
    }

    private fun recordBenchmark(
        eventName: String,
        attributes: Map<String, String> = emptyMap(),
    ) {
        benchmarkRecorder.record(eventName, currentSource?.protocol, attributes)
    }

    private fun restorePlaybackState(
        source: VesperPlayerSource,
        preservedState: PreservedPlaybackState,
    ) {
        if (!hasInitializedSource) {
            return
        }

        when {
            preservedState.seekToLiveEdge &&
                _uiState.value.timeline.kind == TimelineKind.LiveDvr -> {
                val liveEdge =
                    _uiState.value.timeline.goLivePositionMs ?: _uiState.value.timeline.positionMs
                if (!seekBindingsTo(liveEdge)) {
                    return
                }
            }
            preservedState.restorePosition &&
                (source.kind == VesperPlayerSourceKind.Local ||
                    source.kind == VesperPlayerSourceKind.Remote) -> {
                if (!seekBindingsTo(preservedState.positionMs.coerceAtLeast(0L))) {
                    return
                }
            }
        }

        if ((preservedState.playbackRate - 1.0f).absoluteValue > 0.001f) {
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
            bindings.setPlaybackRate(preservedState.playbackRate)
        }

        if (preservedState.videoSelection.mode != VesperTrackSelectionMode.Auto) {
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
            bindings.setVideoTrackSelection(preservedState.videoSelection)
        }
        if (preservedState.audioSelection.mode != VesperTrackSelectionMode.Auto) {
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
            bindings.setAudioTrackSelection(preservedState.audioSelection)
        }
        if (preservedState.subtitleSelection.mode != VesperTrackSelectionMode.Auto) {
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
            bindings.setSubtitleTrackSelection(preservedState.subtitleSelection)
        }
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        bindings.setAbrPolicy(preservedState.abrPolicy)

        if (preservedState.shouldResumePlayback) {
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
            bindings.play()
            nativeFramePipelinePlaybackRequested = true
            startNativeFramePipelinePump("restore-playback")
        } else if (preservedState.playbackState == PlaybackStateUi.Paused) {
            if (isRequiredNativeFramePipelineFailureActive()) {
                return
            }
            stopNativeFramePipelinePump()
            nativeFramePipelinePlaybackRequested = false
            bindings.pause()
        }

        refreshFromNative()
    }

    private fun refreshFromNative() {
        if (isDisposed.get() || isRequiredNativeFramePipelineFailureActive()) {
            return
        }
        surfaceHost.updateVideoLayout(bindings.currentVideoLayoutInfo())
        _trackCatalog.value = bindings.currentTrackCatalog()
        _trackSelection.value = bindings.currentTrackSelection()
        _effectiveVideoTrackId.value = bindings.currentEffectiveVideoTrackId()
        _videoVariantObservation.value = bindings.currentVideoVariantObservation()
        currentNativeFramePipelineStatusOnRuntime()

        bindings.pollSnapshot()?.let { snapshot ->
            updateState {
                copy(
                    playbackState = snapshot.playbackState,
                    playbackRate = snapshot.playbackRate,
                    isBuffering = snapshot.isBuffering,
                    isInterrupted = snapshot.isInterrupted,
                    timeline = snapshot.timeline,
                )
            }
        }

        bindings.drainEvents().forEach { event ->
            when (event) {
                is NativeBridgeEvent.PlaybackStateChanged -> {
                    recordBenchmark(
                        "playback_state_changed",
                        mapOf("state" to event.state.name),
                    )
                    nativeFramePipelinePlaybackRequested = event.state == PlaybackStateUi.Playing
                    updateState {
                        copy(playbackState = event.state)
                    }
                }
                is NativeBridgeEvent.PlaybackRateChanged -> {
                    recordBenchmark(
                        "playback_rate_changed",
                        mapOf("rate" to event.rate.toString()),
                    )
                    updateState {
                        copy(playbackRate = event.rate)
                    }
                }
                is NativeBridgeEvent.BufferingChanged -> {
                    recordBenchmark(
                        "buffering_changed",
                        mapOf("isBuffering" to event.isBuffering.toString()),
                    )
                    updateState {
                        copy(isBuffering = event.isBuffering)
                    }
                }
                is NativeBridgeEvent.InterruptionChanged -> {
                    recordBenchmark(
                        "interruption_changed",
                        mapOf("isInterrupted" to event.isInterrupted.toString()),
                    )
                    updateState {
                        copy(isInterrupted = event.isInterrupted)
                    }
                }
                is NativeBridgeEvent.VideoSurfaceChanged -> {
                    recordBenchmark(
                        "video_surface_changed",
                        mapOf("attached" to event.attached.toString()),
                    )
                    updateState {
                        copy(
                            subtitle = if (event.attached) {
                                i18n.surfaceAttached(currentSource?.let(::sourceSubtitle))
                            } else {
                                i18n.surfaceDetached(currentSource?.let(::sourceSubtitle))
                            }
                        )
                    }
                }
                is NativeBridgeEvent.SeekCompleted -> {
                    recordBenchmark(
                        "seek_completed",
                        mapOf("positionMs" to event.positionMs.toString()),
                    )
                    updateState {
                        copy(timeline = timeline.copy(positionMs = event.positionMs))
                    }
                }
                is NativeBridgeEvent.RetryScheduled -> {
                    recordBenchmark(
                        "retry_scheduled",
                        mapOf(
                            "attempt" to event.attempt.toString(),
                            "delayMs" to event.delayMs.toString(),
                        ),
                    )
                    updateState {
                        copy(
                            subtitle = i18n.retryScheduled(
                                i18n.retryDelay(event.delayMs),
                                event.attempt,
                            ),
                        )
                    }
                }
                is NativeBridgeEvent.Ended -> {
                    recordBenchmark("playback_ended")
                    updateState {
                        copy(playbackState = PlaybackStateUi.Finished, isBuffering = false)
                    }
                }
                is NativeBridgeEvent.Warning -> {
                    runtimeWarnings += event.warning
                }
                is NativeBridgeEvent.Error -> {
                    recordBenchmark(
                        "playback_error",
                        mapOf(
                            "categoryOrdinal" to event.categoryOrdinal.toString(),
                            "retriable" to event.retriable.toString(),
                        ),
                    )
                    updateState {
                        copy(subtitle = i18n.nativeError(event.message))
                    }
                }
            }
        }

        syncNativeFramePipelinePumpWithPlaybackState()
    }

    private fun installNativeUpdateListener() {
        val epoch = nativeUpdateEpoch
        bindings.setOnNativeUpdateListener {
            if (isDisposed.get() || epoch != nativeUpdateEpoch) {
                return@setOnNativeUpdateListener
            }
            refreshFromNative()
        }
    }

    private fun advanceNativeUpdateEpoch(clearListener: Boolean = false) {
        nativeUpdateEpoch += 1
        if (clearListener) {
            bindings.setOnNativeUpdateListener(null)
        } else {
            installNativeUpdateListener()
        }
    }

    private fun clearTrackState() {
        _trackCatalog.value = VesperTrackCatalog.Empty
        _trackSelection.value = VesperTrackSelectionSnapshot()
        _effectiveVideoTrackId.value = null
        _videoVariantObservation.value = null
    }

    private fun sourceSubtitle(source: VesperPlayerSource): String = i18n.sourceSubtitle(source)

    private fun openNativeFramePipelineAfterSystemStartup(
        source: VesperPlayerSource,
        startupDiagnostics: List<Map<String, Any?>>,
    ): Boolean {
        currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)
        nativeFramePipelinePumpScheduler.execute {
            val result =
                runCatching {
                    val openStatus =
                        bindings.openNativeFramePipeline(
                            source = source,
                            sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                            nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
                            surfaceKind = surfaceKind,
                        )
                    check(openStatus != null) {
                        "Android native-frame pipeline open returned no session."
                    }
                    openStatus to advanceNativeFramePipelineOnce()
                }
            runOnMainThread {
                applyNativeFramePipelineOpenResult(source, startupDiagnostics, result)
            }
        }
        return !isRequiredNativeFramePipelineFailureActive()
    }

    private fun applyNativeFramePipelineOpenResult(
        source: VesperPlayerSource,
        startupDiagnostics: List<Map<String, Any?>>,
        result: Result<Pair<Map<String, Any?>, Map<String, Any?>?>>,
    ) {
        if (isDisposed.get() || source != currentSource) {
            result.getOrNull()?.second?.nativeFramePipelineFrameHandle()?.let { handle ->
                postNativeFramePipelineRelease(handle, presented = false)
            }
            return
        }
        result
            .onSuccess { opened ->
                nativeFramePipelineOpenStatus = opened.first
                Log.i(
                    TAG,
                    "native-frame pipeline opened route=${nativeFramePipelineOpenStatus?.get("route")} " +
                        "presenter=${nativeFramePipelineOpenStatus?.get("presenterProfile")} " +
                        "surfaceAttached=${nativeFramePipelineOpenStatus?.get("surfaceAttached")}",
                )
                nativeFramePipelineLastStatus = opened.second
                Log.i(
                    TAG,
                    "native-frame pipeline first advance status=${nativeFramePipelineLastStatus?.get("status")} " +
                        "message=${nativeFramePipelineLastStatus?.get("message")}",
                )
                publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
                currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)
                syncNativeFramePipelinePumpWithPlaybackState()
            }
            .onFailure { error ->
                handleNativeFramePipelineOpenFailure(source, startupDiagnostics, error)
            }
    }

    private fun handleNativeFramePipelineOpenFailure(
        source: VesperPlayerSource,
        startupDiagnostics: List<Map<String, Any?>>,
        error: Throwable,
    ) {
        val reason =
            error.message
                ?.takeUnless(String::isBlank)
                ?: "Android native-frame pipeline open failed."
        stopNativeFramePipelinePump()
        nativeFramePipelineOpenStatus = null
        nativeFramePipelineLastStatus = null
        resetNativeFramePipelineRuntimeMarkers()
        nativeFramePipelineFallbackReason = reason
        nativeFramePipelineRequiredFailure =
            nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
        closeNativeFramePipelineOnRuntime()
        currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)

        if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
            recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to reason))
            failRequiredNativeFramePipeline(reason, source)
            return
        }

        recordBenchmark("native_frame_pipeline_fallback", mapOf("reason" to reason))
        Log.i(TAG, "native-frame pipeline open failed; continuing system playback: $reason")
    }

    private fun seekBindingsTo(positionMs: Long): Boolean {
        val shouldResumeNativeFramePump =
            nativeFramePipelinePumpRunning || _uiState.value.playbackState == PlaybackStateUi.Playing
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        if (isRequiredNativeFramePipelineFailureActive()) {
            return false
        }
        bindings.seekTo(positionMs)
        flushNativeFramePipeline()
        if (isRequiredNativeFramePipelineFailureActive()) {
            return false
        }
        if (nativeFramePipelineOpenStatus == null) {
            return true
        }
        seekNativeFramePipelineOnRuntime(positionMs)
        if (isRequiredNativeFramePipelineFailureActive()) {
            return false
        }
        if (shouldResumeNativeFramePump) {
            startNativeFramePipelinePump("seek")
        }
        return true
    }

    private fun flushNativeFramePipeline() {
        if (nativeFramePipelineOpenStatus == null) {
            return
        }
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        flushNativeFramePipelineOnRuntime()
    }

    private fun restartNativeFramePipelineFromBeginning() {
        if (nativeFramePipelineOpenStatus == null) {
            return
        }
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        seekNativeFramePipelineOnRuntime(0L)
    }

    private fun isRequiredNativeFramePipelineFailureActive(): Boolean =
        nativeFramePipelineRequiredFailure && nativeFramePipelineFallbackReason != null

    private fun syncNativeFramePipelineSurfaceDiagnostics() {
        if (nativeFramePipelineOpenStatus == null) {
            return
        }
        currentNativeFramePipelineStatusOnRuntime()
    }

    private fun advanceNativeFramePipelineOnce(): Map<String, Any?>? {
        val status = bindings.advanceNativeFramePipeline() ?: return null
        val frameHandle = status.nativeFramePipelineFrameHandle()
        if (status["status"] == "frame" && frameHandle != null) {
            if (status["requiresHostRelease"].toBooleanOrFalse()) {
                return status
            }
            // Presented release-to-surface frames are released by the Rust pipeline and report
            // `presented`. Any raw frame handle reaching Kotlin is not presented by this bridge.
            return bindings.releaseNativeFramePipelineFrame(frameHandle, presented = false)
                ?: status
        }
        return status
    }

    private fun flushNativeFramePipelineOnRuntime() {
        postNativeFramePipelineCommand("flush") {
            releasePendingTimedNativeFrameFromRuntime(presented = false)
            bindings.flushNativeFramePipeline()
        }
    }

    private fun postNativeFramePipelineCommand(
        operation: String,
        command: () -> Map<String, Any?>?,
    ) {
        nativeFramePipelinePumpScheduler.execute {
            val result = runCatching(command)
            runOnMainThread {
                applyNativeFramePipelineCommandResult(operation, result)
            }
        }
    }

    private fun seekNativeFramePipelineOnRuntime(positionMs: Long) {
        postNativeFramePipelineCommand("seek") {
            releasePendingTimedNativeFrameFromRuntime(presented = false)
            bindings.seekNativeFramePipeline(positionMs)
        }
    }

    private fun currentNativeFramePipelineStatusOnRuntime() {
        postNativeFramePipelineCommand("status") {
            bindings.currentNativeFramePipelineStatus()
        }
    }

    private fun closeNativeFramePipelineOnRuntime() {
        nativeFramePipelinePumpScheduler.execute {
            runCatching {
                releasePendingTimedNativeFrameFromRuntime(presented = false)
                bindings.closeNativeFramePipeline()
            }.onFailure { error ->
                runOnMainThread {
                    Log.w(TAG, "native-frame pipeline close failed", error)
                }
            }
        }
    }

    private fun applyNativeFramePipelineCommandResult(
        operation: String,
        result: Result<Map<String, Any?>?>,
    ) {
        if (isDisposed.get()) {
            return
        }
        result
            .onSuccess { status ->
                nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
                markNativeFramePipelineDiagnosticsDirty()
            }
            .onFailure { handleNativeFramePipelineRuntimeFailure(operation, it) }
    }

    private fun handleNativeFramePipelineRuntimeFailure(operation: String, error: Throwable) {
        val reason =
            error.message
                ?.takeUnless(String::isBlank)
                ?: "Android native-frame pipeline $operation failed."
        if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
            Log.w(TAG, "required native-frame pipeline $operation failed; stopping playback", error)
        } else {
            Log.w(TAG, "native-frame pipeline $operation failed; falling back to system playback", error)
        }
        stopNativeFramePipelinePump()
        releasePendingTimedNativeFrameOnRuntime(presented = false)
        nativeFramePipelineFallbackReason = reason
        nativeFramePipelineRequiredFailure =
            nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
        nativeFramePipelineOpenStatus = null
        nativeFramePipelineLastStatus = null
        resetNativeFramePipelineRuntimeMarkers()
        markNativeFramePipelineDiagnosticsDirty()
        val shouldFailRequiredNativeFrame =
            nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
        runOnMainThread {
            if (isDisposed.get()) {
                return@runOnMainThread
            }
            closeNativeFramePipelineOnRuntime()
            if (shouldFailRequiredNativeFrame) {
                recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to reason))
                failRequiredNativeFramePipeline(reason, currentSource)
            }
        }
    }

    private fun runOnMainThread(action: () -> Unit) {
        if (
            nativeFramePipelinePumpScheduler.inlineCallbacksForTests ||
                Looper.myLooper() == Looper.getMainLooper()
        ) {
            action()
        } else {
            mainHandler.post(action)
        }
    }

    private fun failRequiredNativeFramePipeline(reason: String, source: VesperPlayerSource?) {
        hasInitializedSource = false
        pendingAutoPlay = false
        nativeFramePipelinePlaybackRequested = false
        runCatching { bindings.clearSystemPlayback() }
        clearTrackState()
        surfaceHost.reattachIfAvailable()
        updateState {
            copy(
                subtitle = i18n.stubError(reason),
                sourceLabel = source?.label ?: sourceLabel,
                playbackState = PlaybackStateUi.Ready,
                isBuffering = false,
            )
        }
    }

    private fun startNativeFramePipelinePump(reason: String) {
        if (
            isDisposed.get() ||
                nativeFramePipelineOpenStatus == null ||
                nativeFramePipelineFallbackReason != null
        ) {
            return
        }
        if (nativeFramePipelinePumpRunning) {
            markNativeFramePipelineDiagnosticsDirty()
            return
        }
        Log.d(TAG, "starting native-frame pipeline pump reason=$reason")
        nativeFramePipelinePumpRunning = true
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        scheduleNativeFramePipelinePump(delayMs = 0L)
        markNativeFramePipelineDiagnosticsDirty()
    }

    private fun stopNativeFramePipelinePump() {
        if (!nativeFramePipelinePumpRunning) {
            return
        }
        Log.d(TAG, "stopping native-frame pipeline pump")
        nativeFramePipelinePumpRunning = false
        nativeFramePipelinePumpEpoch += 1
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        nativeFramePipelinePumpScheduler.cancel()
        markNativeFramePipelineDiagnosticsDirty()
    }

    private fun scheduleNativeFramePipelinePump(delayMs: Long) {
        val epoch = nativeFramePipelinePumpEpoch
        nativeFramePipelinePumpScheduler.schedule(delayMs) {
            runNativeFramePipelinePumpTickWorker(epoch)
        }
    }

    private fun runNativeFramePipelinePumpTickWorker(epoch: Long) {
        runCatching {
            runNativeFramePipelinePumpTickWorkerUnchecked(epoch)
        }.onFailure { error ->
            runOnMainThread {
                if (canApplyNativeFramePumpResult(epoch)) {
                    handleNativeFramePipelineRuntimeFailure("pump", error)
                }
            }
        }
    }

    private fun runNativeFramePipelinePumpTickWorkerUnchecked(epoch: Long) {
        if (!canContinueNativeFramePump(epoch)) {
            releasePendingTimedNativeFrameFromRuntime(presented = false)
            return
        }
        val pendingRelease = takePendingTimedNativeFrameForRuntime()
        if (pendingRelease != null) {
            if (!canContinueNativeFramePump(epoch)) {
                releaseStaleNativeFramePipelineFrame(pendingRelease.handle)
                return
            }
            val releaseResult =
                runCatching {
                    bindings.releaseNativeFramePipelineFrame(pendingRelease.handle, presented = true)
                }
            runOnMainThread {
                applyNativeFramePipelineReleaseResult(epoch, releaseResult)
            }
            if (!canContinueNativeFramePump(epoch)) {
                return
            }
        } else if (!canContinueNativeFramePump(epoch)) {
            return
        }

        val advanceResult = runCatching { advanceNativeFramePipelineOnce() }
        runOnMainThread {
            applyNativeFramePipelineAdvanceResult(epoch, advanceResult)
        }
    }

    private fun applyNativeFramePipelineReleaseResult(
        epoch: Long,
        result: Result<Map<String, Any?>?>,
    ) {
        if (!canApplyNativeFramePumpResult(epoch)) {
            return
        }
        result
            .onSuccess { status ->
                nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
                publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
            }
            .onFailure { handleNativeFramePipelineRuntimeFailure("release", it) }
    }

    private fun applyNativeFramePipelineAdvanceResult(
        epoch: Long,
        result: Result<Map<String, Any?>?>,
    ) {
        if (!canApplyNativeFramePumpResult(epoch)) {
            return
        }
        val status =
            result
                .onFailure { handleNativeFramePipelineRuntimeFailure("advance", it) }
                .getOrElse { return }
        runNativeFramePipelinePumpTick(epoch, status)
    }

    private fun runNativeFramePipelinePumpTick(
        epoch: Long,
        status: Map<String, Any?>?,
    ) {
        if (
            epoch != nativeFramePipelinePumpEpoch ||
                !nativeFramePipelinePumpRunning ||
                isDisposed.get()
        ) {
            return
        }
        if (nativeFramePipelineOpenStatus == null || nativeFramePipelineFallbackReason != null) {
            stopNativeFramePipelinePump()
            return
        }

        nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
        publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
        val timedFrame = status?.nativeFramePipelineTimedFrame()
        if (timedFrame != null) {
            val delayMs = nativeFramePipelineDelayUntilPresentation(timedFrame.presentationTimeUs)
            if (delayMs > 0L) {
                storePendingTimedNativeFrameFromRuntime(timedFrame)
                scheduleNativeFramePipelinePump(delayMs)
                return
            }
            val releaseResult =
                runCatching {
                    bindings.releaseNativeFramePipelineFrame(timedFrame.handle, presented = true)
                }
            applyNativeFramePipelineReleaseResult(epoch, releaseResult)
            if (nativeFramePipelineOpenStatus == null || nativeFramePipelineFallbackReason != null) {
                return
            }
            publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
        }
        if (enforceNativeFramePipelineFirstFrameWatchdog()) {
            return
        }
        val nextDelayMs = nativeFramePipelinePumpDelayMs(status)
        if (nextDelayMs == null) {
            stopNativeFramePipelinePump()
            return
        }
        scheduleNativeFramePipelinePump(nextDelayMs)
    }

    private fun canContinueNativeFramePump(epoch: Long): Boolean =
        canApplyNativeFramePumpResult(epoch) &&
            nativeFramePipelineOpenStatus != null &&
            nativeFramePipelineFallbackReason == null

    private fun canApplyNativeFramePumpResult(epoch: Long): Boolean =
        epoch == nativeFramePipelinePumpEpoch &&
            nativeFramePipelinePumpRunning &&
            !isDisposed.get()

    private fun publishNativeFramePipelinePumpStatus(status: Map<String, Any?>?) {
        if (status == null) {
            return
        }
        val key = nativeFramePipelinePumpSummaryKey(status)
        if (key != nativeFramePipelineLastLoggedPumpKey) {
            nativeFramePipelineLastLoggedPumpKey = key
            Log.d(TAG, "native-frame pump ${nativeFramePipelinePumpSummary(status)}")
        }
        if (key != nativeFramePipelineLastPublishedDiagnosticsKey) {
            nativeFramePipelineLastPublishedDiagnosticsKey = key
            markNativeFramePipelineDiagnosticsDirty()
        }
    }

    private fun markNativeFramePipelineDiagnosticsDirty() {
        nativeFramePipelineDiagnosticsDirty = true
    }

    private fun refreshNativeFramePipelineDiagnosticsIfDirty() {
        if (!nativeFramePipelineDiagnosticsDirty) {
            return
        }
        nativeFramePipelineDiagnosticsDirty = false
        currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
    }

    private fun nativeFramePipelinePumpSummary(status: Map<String, Any?>): String {
        val counters = nativeFramePipelineCounters()
        val message =
            status["message"]
                ?.toString()
                ?.replace('\n', ' ')
                ?.take(120)
                .orEmpty()
        return "status=${status["status"]} " +
            "surfaceAttached=${nativeFramePipelineBooleanValue("surfaceAttached")} " +
            "presenterReady=${nativeFramePipelineBooleanValue("presenterReady")} " +
            "presenterState=${nativeFramePipelineStringValue("presenterState")} " +
            "decodedFrames=${counters.longValue("decodedFrames")} " +
            "processedFrames=${counters.longValue("processedFrames")} " +
            "presenterSubmits=${counters.longValue("presenterSubmitCount")} " +
            "presentedFrames=${counters.longValue("presentedFrames")} " +
            "sourcePackets=${counters.longValue("sourcePacketsRead")} " +
            "decoderPackets=${counters.longValue("decoderPacketsSent")} " +
            "decoderBackpressure=${counters.longValue("decoderBackpressureCount")} " +
            "deadlineMisses=${counters.longValue("deadlineMisses")} " +
            "backpressure=${counters.longValue("backpressureCount")} " +
            "message=$message"
    }

    private fun nativeFramePipelinePumpSummaryKey(status: Map<String, Any?>): String {
        val counters = nativeFramePipelineCounters()
        val message =
            status["message"]
                ?.toString()
                ?.replace('\n', ' ')
                ?.take(80)
                .orEmpty()
        return "status=${status["status"]};" +
            "surface=${nativeFramePipelineBooleanValue("surfaceAttached")};" +
            "ready=${nativeFramePipelineBooleanValue("presenterReady")};" +
            "state=${nativeFramePipelineStringValue("presenterState")};" +
            "decoded=${nativeFramePipelineCounterLogBucket(counters.longValue("decodedFrames"))};" +
            "processed=${nativeFramePipelineCounterLogBucket(counters.longValue("processedFrames"))};" +
            "submits=${nativeFramePipelineCounterLogBucket(counters.longValue("presenterSubmitCount"))};" +
            "presented=${nativeFramePipelineCounterLogBucket(counters.longValue("presentedFrames"))};" +
            "sourcePackets=${nativeFramePipelineCounterLogBucket(counters.longValue("sourcePacketsRead"))};" +
            "decoderPackets=${nativeFramePipelineCounterLogBucket(counters.longValue("decoderPacketsSent"))};" +
            "decoderBackpressure=${nativeFramePipelineCounterLogBucket(counters.longValue("decoderBackpressureCount"))};" +
            "deadline=${nativeFramePipelineCounterLogBucket(counters.longValue("deadlineMisses"))};" +
            "backpressure=${nativeFramePipelineCounterLogBucket(counters.longValue("backpressureCount"))};" +
            "message=$message"
    }

    private fun nativeFramePipelineCounterLogBucket(value: Long): Long =
        when {
            value <= 0L -> 0L
            value < NATIVE_FRAME_PIPELINE_LOG_COUNTER_BUCKET_SIZE -> 1L
            else -> value / NATIVE_FRAME_PIPELINE_LOG_COUNTER_BUCKET_SIZE
        }

    private fun enforceNativeFramePipelineFirstFrameWatchdog(): Boolean {
        if (
            nativeFramePipelineOpenStatus == null ||
                nativeFramePipelineFallbackReason != null ||
                !nativeFramePipelinePumpRunning
        ) {
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
            return false
        }
        val counters = nativeFramePipelineCounters()
        if (counters.longValue("presentedFrames") > 0L) {
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
            return false
        }
        val surfaceAttached = nativeFramePipelineBooleanValue("surfaceAttached")
        val presenterConfigured = nativeFramePipelineBooleanValue("presenterConfigured")
        if (!surfaceAttached || !presenterConfigured) {
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
            return false
        }
        val now = SystemClock.elapsedRealtime()
        val startedAt =
            nativeFramePipelineFirstFrameWatchdogStartedAtMs ?: now.also {
                nativeFramePipelineFirstFrameWatchdogStartedAtMs = it
            }
        if (now - startedAt < NATIVE_FRAME_PIPELINE_FIRST_FRAME_TIMEOUT_MS) {
            return false
        }

        val reason =
            "Android native-frame pipeline did not present a frame within " +
                "${NATIVE_FRAME_PIPELINE_FIRST_FRAME_TIMEOUT_MS}ms after surface attachment " +
                "(presenterState=${nativeFramePipelineStringValue("presenterState")}, " +
                "decodedFrames=${counters.longValue("decodedFrames")}, " +
                "presenterSubmits=${counters.longValue("presenterSubmitCount")}, " +
                "presentedFrames=${counters.longValue("presentedFrames")})."
        handleNativeFramePipelineRuntimeFailure(
            "first-frame-timeout",
            IllegalStateException(reason),
        )
        return true
    }

    private fun resetNativeFramePipelineFirstFrameWatchdogIfDetached() {
        if (
            !nativeFramePipelineBooleanValue("surfaceAttached") ||
                nativeFramePipelineCounters().longValue("presentedFrames") > 0L
        ) {
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        }
    }

    private fun resetNativeFramePipelineRuntimeMarkers() {
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        nativeFramePipelineLastLoggedPumpKey = null
        nativeFramePipelineLastPublishedDiagnosticsKey = null
    }

    private fun nativeFramePipelinePumpDelayMs(status: Map<String, Any?>?): Long? =
        when (status?.get("status")?.toString()) {
            "endOfStream" -> null
            "presented", "released", "frame" -> NATIVE_FRAME_PIPELINE_ACTIVE_PUMP_DELAY_MS
            "pending" -> {
                val message = status["message"]?.toString().orEmpty()
                if (message.contains("backpressure", ignoreCase = true)) {
                    NATIVE_FRAME_PIPELINE_BACKPRESSURE_PUMP_DELAY_MS
                } else {
                    NATIVE_FRAME_PIPELINE_IDLE_PUMP_DELAY_MS
                }
            }
            else -> NATIVE_FRAME_PIPELINE_IDLE_PUMP_DELAY_MS
        }

    private fun syncNativeFramePipelinePumpWithPlaybackState() {
        if (nativeFramePipelineOpenStatus != null && nativeFramePipelineFallbackReason == null) {
            if (nativeFramePipelinePlaybackRequested) {
                startNativeFramePipelinePump("playback-request")
            } else {
                stopNativeFramePipelinePump()
            }
            return
        }
        when (_uiState.value.playbackState) {
            PlaybackStateUi.Playing -> startNativeFramePipelinePump("playback-state")
            PlaybackStateUi.Ready,
            PlaybackStateUi.Paused,
            PlaybackStateUi.Finished,
            -> stopNativeFramePipelinePump()
        }
    }

    private fun nativeFramePipelineDelayUntilPresentation(presentationTimeUs: Long): Long {
        val timeline = _uiState.value.timeline
        val framePositionMs = (presentationTimeUs / 1_000L).coerceAtLeast(0L)
        val deltaMs = framePositionMs - timeline.positionMs
        if (deltaMs <= 0L) {
            return 0L
        }
        val playbackRate = _uiState.value.playbackRate.takeIf { it.isFinite() && it > 0f } ?: 1f
        return (deltaMs / playbackRate).toLong().coerceIn(1L, NATIVE_FRAME_PIPELINE_MAX_FRAME_DELAY_MS)
    }

    private fun reschedulePendingTimedNativeFrameForCurrentRate() {
        val pending = synchronized(nativeFramePipelineRuntimeLock) {
            pendingTimedNativeFrame
        } ?: return
        if (
            !nativeFramePipelinePumpRunning ||
                nativeFramePipelineOpenStatus == null ||
                nativeFramePipelineFallbackReason != null
        ) {
            return
        }
        scheduleNativeFramePipelinePump(
            nativeFramePipelineDelayUntilPresentation(pending.presentationTimeUs)
        )
    }

    private fun releasePendingTimedNativeFrame(presented: Boolean) {
        releasePendingTimedNativeFrameOnRuntime(presented)
    }

    private fun releasePendingTimedNativeFrameOnRuntime(presented: Boolean) {
        nativeFramePipelinePumpScheduler.execute {
            val result = runCatching {
                releasePendingTimedNativeFrameFromRuntime(presented)
            }
            runOnMainThread {
                result.onFailure { handleNativeFramePipelineRuntimeFailure("release", it) }
            }
        }
    }

    private fun takePendingTimedNativeFrameForRuntime(): TimedNativeFrameRelease? =
        synchronized(nativeFramePipelineRuntimeLock) {
            pendingTimedNativeFrame?.also {
                pendingTimedNativeFrame = null
            }
        }

    private fun storePendingTimedNativeFrameFromRuntime(timedFrame: TimedNativeFrameRelease) {
        synchronized(nativeFramePipelineRuntimeLock) {
            pendingTimedNativeFrame = timedFrame
        }
    }

    private fun clearPendingTimedNativeFrameFromRuntime(): TimedNativeFrameRelease? =
        synchronized(nativeFramePipelineRuntimeLock) {
            pendingTimedNativeFrame?.also {
                pendingTimedNativeFrame = null
            }
        }

    private fun releasePendingTimedNativeFrameFromRuntime(presented: Boolean) {
        val pending = clearPendingTimedNativeFrameFromRuntime() ?: return
        if (presented) {
            bindings.releaseNativeFramePipelineFrame(pending.handle, presented = true)
        } else {
            releaseStaleNativeFramePipelineFrame(pending.handle)
        }
    }

    private fun releaseNativeFramePipelineFrame(frameHandle: Long, presented: Boolean) {
        postNativeFramePipelineRelease(frameHandle, presented)
    }

    private fun postNativeFramePipelineRelease(frameHandle: Long, presented: Boolean) {
        nativeFramePipelinePumpScheduler.execute {
            val result =
                runCatching {
                    bindings.releaseNativeFramePipelineFrame(frameHandle, presented = presented)
                }
            runOnMainThread {
                result
                    .onSuccess { status ->
                        nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
                        publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
                    }
                    .onFailure { handleNativeFramePipelineRuntimeFailure("release", it) }
            }
        }
    }

    private fun releaseStaleNativeFramePipelineFrame(frameHandle: Long) {
        bindings.releaseNativeFramePipelineFrame(frameHandle, presented = false)
    }

    private fun nativeFramePipelineCounters(): Map<String, Any?> {
        val counters =
            nativeFramePipelineLastStatus?.get("counters")
                ?: nativeFramePipelineOpenStatus?.get("counters")
                ?: return emptyMap()
        return (counters as? Map<*, *>)
            ?.mapNotNull { (key, value) ->
                key?.toString()?.let { it to value }
            }
            ?.toMap()
            ?: emptyMap()
    }

    private fun nativeFramePipelineStringValue(key: String): String? =
        if (nativeFramePipelineLastStatus != null) {
            nativeFramePipelineLastStatus?.get(key)?.toString()
        } else {
            nativeFramePipelineOpenStatus?.get(key)?.toString()
        }

    private fun nativeFramePipelineBooleanValue(key: String): Boolean =
        nativeFramePipelineLastStatus?.get(key)?.toBooleanOrFalse()
            ?: nativeFramePipelineOpenStatus?.get(key).toBooleanOrFalse()

    private fun Any?.toBooleanOrFalse(): Boolean =
        when (this) {
            is Boolean -> this
            is String -> this.equals("true", ignoreCase = true)
            is Number -> this.toInt() != 0
            else -> false
        }

    private fun Map<String, Any?>.longValue(key: String): Long =
        when (val value = this[key]) {
            is Number -> value.toLong()
            is String -> value.toLongOrNull() ?: 0L
            else -> 0L
        }

    private fun Map<String, Any?>.nativeFramePipelineFrameHandle(): Long? {
        val value = this["handle"] ?: return null
        return when (value) {
            is Number -> value.toLong()
            is String -> value.toLongOrNull()
            else -> null
        }?.takeIf { it > 0L }
    }

    private fun Map<String, Any?>.nativeFramePipelineTimedFrame(): TimedNativeFrameRelease? {
        if (this["status"] != "frame") {
            return null
        }
        val handle = nativeFramePipelineFrameHandle() ?: return null
        val presentationTimeUs =
            when (val value = this["presentationTimeUs"]) {
                is Number -> value.toLong()
                is String -> value.toLongOrNull()
                else -> null
            } ?: return null
        return TimedNativeFrameRelease(handle, presentationTimeUs)
    }

    private fun nativeFramePresenterProfileName(): String =
        when (surfaceKind) {
            NativeVideoSurfaceKind.SurfaceView -> "SurfaceView"
            NativeVideoSurfaceKind.TextureView -> "SurfaceTexture"
        }
}

private const val TAG = "VesperPlayerAndroidHost"
private const val NATIVE_FRAME_PIPELINE_ACTIVE_PUMP_DELAY_MS = 8L
private const val NATIVE_FRAME_PIPELINE_IDLE_PUMP_DELAY_MS = 16L
private const val NATIVE_FRAME_PIPELINE_BACKPRESSURE_PUMP_DELAY_MS = 32L
private const val NATIVE_FRAME_PIPELINE_MAX_FRAME_DELAY_MS = 100L
private const val NATIVE_FRAME_PIPELINE_FIRST_FRAME_TIMEOUT_MS = 2_500L
private const val NATIVE_FRAME_PIPELINE_LOG_COUNTER_BUCKET_SIZE = 30L

private data class TimedNativeFrameRelease(
    val handle: Long,
    val presentationTimeUs: Long,
)

internal interface NativeFramePipelinePumpScheduler {
    val inlineCallbacksForTests: Boolean
        get() = false
    fun schedule(delayMs: Long, action: () -> Unit)
    fun execute(action: () -> Unit) = schedule(delayMs = 0L, action)
    fun cancel()
    fun close() = cancel()
}

private class HandlerNativeFramePipelinePumpScheduler(
    private val inlineRuntimeCommandsForLocalTests: Boolean = isLocalUnitTestRuntime(),
) : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = inlineRuntimeCommandsForLocalTests
    private val thread: HandlerThread by lazy {
        HandlerThread("VesperNativeFramePump").also { it.start() }
    }
    private val handler: Handler by lazy { Handler(thread.looper) }
    private var scheduled: Runnable? = null
    private var started = false
    private var closed = false

    @Synchronized
    override fun schedule(delayMs: Long, action: () -> Unit) {
        if (closed) {
            return
        }
        cancel()
        lateinit var runnable: Runnable
        runnable =
            Runnable {
                synchronized(this) {
                    if (closed || scheduled !== runnable) {
                        return@Runnable
                    }
                    scheduled = null
                }
                action()
            }
        scheduled = runnable
        started = true
        handler.postDelayed(runnable, delayMs.coerceAtLeast(0L))
    }

    override fun execute(action: () -> Unit) {
        if (inlineRuntimeCommandsForLocalTests) {
            action()
            return
        }
        val shouldPost =
            synchronized(this) {
                if (closed) {
                    false
                } else {
                    started = true
                    true
                }
            }
        if (!shouldPost) {
            return
        }
        handler.post(action)
    }

    @Synchronized
    override fun cancel() {
        scheduled?.let(handler::removeCallbacks)
        scheduled = null
    }

    @Synchronized
    override fun close() {
        closed = true
        cancel()
        if (started) {
            handler.removeCallbacksAndMessages(null)
        }
        if (started && thread.isAlive) {
            thread.quitSafely()
        }
    }
}

private fun isLocalUnitTestRuntime(): Boolean =
    System.getProperty("java.vm.name")
        ?.contains("Dalvik", ignoreCase = true) != true

private data class PreservedPlaybackState(
    val positionMs: Long,
    val restorePosition: Boolean,
    val seekToLiveEdge: Boolean,
    val playbackRate: Float,
    val playbackState: PlaybackStateUi,
    val shouldResumePlayback: Boolean,
    val videoSelection: VesperTrackSelection,
    val audioSelection: VesperTrackSelection,
    val subtitleSelection: VesperTrackSelection,
    val abrPolicy: VesperAbrPolicy,
) {
    companion object {
        fun capture(
            uiState: PlayerHostUiState,
            trackSelection: VesperTrackSelectionSnapshot,
        ): PreservedPlaybackState {
            val seekToLiveEdge =
                uiState.timeline.kind == TimelineKind.LiveDvr &&
                    uiState.timeline.isAtLiveEdge()
            return PreservedPlaybackState(
                positionMs = uiState.timeline.positionMs,
                restorePosition = uiState.timeline.isSeekable || uiState.timeline.durationMs != null,
                seekToLiveEdge = seekToLiveEdge,
                playbackRate = uiState.playbackRate,
                playbackState = uiState.playbackState,
                shouldResumePlayback = uiState.playbackState == PlaybackStateUi.Playing,
                videoSelection = trackSelection.video,
                audioSelection = trackSelection.audio,
                subtitleSelection = trackSelection.subtitle,
                abrPolicy = trackSelection.abrPolicy,
            )
        }
    }
}

internal interface VesperNativeBindings {
    fun probeMobilePlugins(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
    ): List<Map<String, Any?>>

    fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
    ): NativeBridgeStartup
    fun openNativeFramePipeline(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?>?
    fun advanceNativeFramePipeline(): Map<String, Any?>?
    fun releaseNativeFramePipelineFrame(frameHandle: Long, presented: Boolean): Map<String, Any?>?
    fun attachNativeFramePipelineSurface(
        surface: Surface,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?>?
    fun detachNativeFramePipelineSurface(): Map<String, Any?>?
    fun flushNativeFramePipeline(): Map<String, Any?>?
    fun seekNativeFramePipeline(positionMs: Long): Map<String, Any?>?
    fun currentNativeFramePipelineStatus(): Map<String, Any?>?
    fun closeNativeFramePipeline()
    fun dispose()
    fun refreshSnapshot()
    fun currentTrackCatalog(): VesperTrackCatalog
    fun currentTrackSelection(): VesperTrackSelectionSnapshot
    fun currentEffectiveVideoTrackId(): String?
    fun currentVideoVariantObservation(): VesperVideoVariantObservation?
    fun currentVideoLayoutInfo(): NativeVideoLayoutInfo?
    fun setOnNativeUpdateListener(listener: (() -> Unit)?)
    fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind)
    fun detachSurface()
    fun pollSnapshot(): NativeBridgeSnapshot?
    fun drainEvents(): List<NativeBridgeEvent>
    fun play()
    fun pause()
    fun stop()
    fun seekTo(positionMs: Long)
    fun setPlaybackRate(rate: Float)
    fun setVideoTrackSelection(selection: VesperTrackSelection)
    fun setAudioTrackSelection(selection: VesperTrackSelection)
    fun setSubtitleTrackSelection(selection: VesperTrackSelection)
    fun setAbrPolicy(policy: VesperAbrPolicy)
    fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration)
    fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata)
    fun clearSystemPlayback()
}

private class MissingVesperNativeBindings : VesperNativeBindings {
    override fun probeMobilePlugins(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
    ): List<Map<String, Any?>> = emptyList()

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
    ): NativeBridgeStartup {
        throw UnsupportedOperationException(VesperNativeLibrary.failureMessage())
    }

    override fun openNativeFramePipeline(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?>? {
        throw UnsupportedOperationException(VesperNativeLibrary.failureMessage())
    }

    override fun advanceNativeFramePipeline(): Map<String, Any?>? = null

    override fun releaseNativeFramePipelineFrame(
        frameHandle: Long,
        presented: Boolean,
    ): Map<String, Any?>? = null

    override fun attachNativeFramePipelineSurface(
        surface: Surface,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?>? = null

    override fun detachNativeFramePipelineSurface(): Map<String, Any?>? = null

    override fun flushNativeFramePipeline(): Map<String, Any?>? = null

    override fun seekNativeFramePipeline(positionMs: Long): Map<String, Any?>? = null

    override fun currentNativeFramePipelineStatus(): Map<String, Any?>? = null

    override fun closeNativeFramePipeline() = Unit

    override fun dispose() = Unit
    override fun refreshSnapshot() = Unit
    override fun currentTrackCatalog(): VesperTrackCatalog = VesperTrackCatalog.Empty
    override fun currentTrackSelection(): VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
    override fun currentEffectiveVideoTrackId(): String? = null
    override fun currentVideoVariantObservation(): VesperVideoVariantObservation? = null
    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = null
    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) = Unit
    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) = Unit
    override fun detachSurface() = Unit
    override fun pollSnapshot(): NativeBridgeSnapshot? = null
    override fun drainEvents(): List<NativeBridgeEvent> = emptyList()
    override fun play() = Unit
    override fun pause() = Unit
    override fun stop() = Unit
    override fun seekTo(positionMs: Long) = Unit
    override fun setPlaybackRate(rate: Float) = Unit
    override fun setVideoTrackSelection(selection: VesperTrackSelection) = Unit
    override fun setAudioTrackSelection(selection: VesperTrackSelection) = Unit
    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) = Unit
    override fun setAbrPolicy(policy: VesperAbrPolicy) = Unit
    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) = Unit
    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) = Unit
    override fun clearSystemPlayback() = Unit
}
