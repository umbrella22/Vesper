package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.Trace
import android.util.Log
import android.view.Surface
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.AudioAttributes
import androidx.media3.common.ColorInfo
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.Timeline
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.text.Cue
import androidx.media3.common.util.UnstableApi
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.ExoPlaybackException
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.Renderer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistTracker
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.MergingMediaSource
import androidx.media3.exoplayer.text.TextOutput
import androidx.media3.exoplayer.text.TextRenderer
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo
import java.io.File
import java.net.URI
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.pow
import kotlin.math.roundToLong
import org.json.JSONArray
import org.json.JSONObject

internal class VesperNativeJniBindings(
    context: Context,
    preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    internal val decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
    internal val benchmarkRecorder: VesperBenchmarkRecorder = VesperBenchmarkRecorder(),
    internal val sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
        VesperSourceNormalizerConfiguration(),
) : VesperNativeBindings {
    internal val appContext = context.applicationContext
    internal val i18n = VesperPlayerI18n.fromContext(appContext)
    internal val mainHandler = Handler(Looper.getMainLooper())

    @Volatile
    internal var sessionHandle: Long? = null
    @Volatile
    internal var nativeFramePipelineHandle: Long? = null
    @Volatile
    internal var nativeFramePipelineStatus: Map<String, Any?>? = null
    internal val isDisposed = AtomicBoolean(false)
    internal val systemPlaybackCallbackGeneration = AtomicLong(0L)
    internal val subtitleSelectionCommandGenerationState = AtomicLong(0L)
    internal var player: ExoPlayer? = null
    internal var playerListener: Player.Listener? = null

    override val isSystemPlaybackActive: Boolean
        get() = player != null && !isDisposed.get()
    internal var analyticsListener: AnalyticsListener? = null
    @Volatile
    internal var attachedSurface: Surface? = null
    internal var nativeFramePipelineOwnsSurface = false
    internal var updateListener: (() -> Unit)? = null
    internal var subtitleCuesListener: ((List<Cue>) -> Unit)? = null
    /**
     * Optional callback invoked when a JNI track-selection command cannot
     * resolve the requested track id against the current Media3 [Tracks]
     * state. Wired by [VesperNativePlayerBridge] to push a structured
     * runtime warning so Flutter observes `subtitle_track_not_found`
     * instead of a silent `Log.w`.
     */
    internal var trackSelectionFailureListener:
        ((NativeTrackSelectionFailure) -> Unit)? = null
    internal var currentTrackCatalogState: VesperTrackCatalog = VesperTrackCatalog.Empty
    internal var currentTrackSelectionState: VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
    internal var currentAppliedSubtitleSelectionState: VesperTrackSelection =
        VesperTrackSelection.disabled()
    internal var currentAdvertisedSubtitleTrackCountState = 0
    @Volatile
    internal var trackSelectionChangeGenerationState = 0L
    @Volatile
    internal var hasObservedTrackCatalog = false
    internal var currentSubtitleCatalogFailure: NativeTrackSelectionFailure? = null
    internal var currentEffectiveVideoTrackIdState: String? = null
    internal var currentVideoVariantObservationState: VesperVideoVariantObservation? = null
    internal var currentVideoLayoutState: NativeVideoLayoutInfo? = null
    internal var currentVideoDecoderName: String? = null
    internal var currentRuntimeHdrEvidence: AndroidRuntimeHdrEvidence? = null
    internal var currentRuntimeSessionProbe: AndroidRuntimeSessionProbeSnapshot? = null
    internal var currentDrmDiagnosticsSource: VesperPlayerSource? = null
    internal var currentRetryMaxAttempts: Int? = null
    internal var currentDrmRuntimeErrorCount = 0
    internal var terminalErrorReportedForCurrentSource = false
    internal var firstFrameWatchdogSource: VesperPlayerSource? = null
    internal var firstFrameWatchdogRunnable: Runnable? = null
    internal var firstFrameRenderedForCurrentSource = false
    @Volatile
    internal var lastSnapshotLogElapsedMs = 0L
    internal val localBridgeEvents = ArrayDeque<NativeBridgeEvent>()

    internal fun addLocalBridgeEvent(event: NativeBridgeEvent) {
        if (localBridgeEvents.size >= MAX_LOCAL_BRIDGE_EVENTS) {
            localBridgeEvents.removeFirst()
        }
        localBridgeEvents += event
    }

    companion object {
        private const val MAX_LOCAL_BRIDGE_EVENTS = 256
        internal const val EXO_SNAPSHOT_LOG_INTERVAL_MS = 2_000L
    }
    internal val preloadCoordinator =
        VesperNativePreloadCoordinator(
            bindings = VesperNativePreloadCoordinator.NativeJniPreloadBindings,
            preloadBudgetPolicy = preloadBudgetPolicy,
        )
    internal val systemPlaybackCoordinator = VesperAndroidSystemPlaybackCoordinator(appContext)
    internal val sourceNormalizerLoopbackServer = VesperSourceNormalizerLoopbackServer()
    internal var currentBenchmarkSourceProtocol: VesperPlayerSourceProtocol? = null
    /**
     * Protocol of the source currently loaded by the player. Used by
     * `collectTrackCatalog` to gate subtitle stable-id generation on DASH
     * sources only — non-DASH subtitle tracks keep the legacy positional
     * id so HLS/MP4 embedded captions are not mislabeled as
     * `subtitle:dash:*`.
     */
    internal var currentSourceProtocol: VesperPlayerSourceProtocol? = null
    internal var currentExternalSubtitleIds: List<String> = emptyList()
    /** All source-declared external ids, including resources that failed preparation. */
    internal var currentDeclaredExternalSubtitleIds: List<String> = emptyList()
    internal var currentExternalSubtitleSources: List<VesperExternalSubtitleSource> = emptyList()
    internal val failedExternalSubtitleIds = linkedSetOf<String>()
    internal var currentSubtitleResourceFailure: NativeTrackSelectionFailure? = null
    internal var currentDeclaredExternalSubtitleCount = 0
    internal var currentDeclaredExternalSubtitleDefaultCount = 0
    internal var currentSubtitleSelectionModeOrdinal = NativeTrackSelectionMode.Disabled.ordinal
    internal var currentSourceNormalizerResource: NativeSourceNormalizerResource? = null
    internal var currentNativeFramePacketSource: NativeFramePacketSource? = null
    internal val firstFrameGate = VesperPlaybackEpochFirstFrameGate()

    override fun probeMobilePlugins(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
    ): List<Map<String, Any?>> {
        if (sourceNormalizerConfiguration.isDisabled && frameProcessorConfiguration.isDisabled) {
            return emptyList()
        }
        VesperNativeLibrary.ensureLoaded()
        val json = VesperNativeJni.probeMobilePlugins(
            source.uri,
            sourceNormalizerConfiguration.modeOrdinal,
            sourceNormalizerConfiguration.pluginLibraryPaths.toTypedArray(),
            sourceNormalizerConfiguration.runtimeProfile,
            frameProcessorConfiguration.modeOrdinal,
            frameProcessorConfiguration.pluginLibraryPaths.toTypedArray(),
        )
        return parsePluginDiagnosticsJson(json)
    }

    override fun prepareSourceNormalizerForPlayback(
        source: VesperPlayerSource,
        enabled: Boolean,
    ): NativeSourceNormalizerResourcePreparedOpenOutcome =
        prepareSourceNormalizerResourceForPlayback(source, enabled)

    override fun disposePreparedSourceNormalizerResource(
        prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
    ) = disposePreparedSourceNormalizerResourceForPlayback(prepared)

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
        preparedSourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
    ): NativeBridgeStartup {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "initialize source=${source.uri} kind=${source.kind} protocol=${source.protocol}")
        val existingSubtitleCuesListener = subtitleCuesListener
        val existingTrackSelectionFailureListener = trackSelectionFailureListener
        dispose(stopSourceNormalizerLoopbackServer = preparedSourceNormalizer.resource == null)
        // initialize() reuses the binding object across source epochs. The
        // callbacks belong to the bridge, so restore them after dispose()
        // clears player-owned listeners; otherwise the next source silently
        // drops cue delivery and JNI selection warnings.
        subtitleCuesListener = existingSubtitleCuesListener
        trackSelectionFailureListener = existingTrackSelectionFailureListener
        isDisposed.set(false)
        val callbackGeneration = systemPlaybackCallbackGeneration.incrementAndGet()
        var preparedSourceNormalizerConsumed = false
        try {
            currentBenchmarkSourceProtocol = source.protocol
            currentSourceProtocol = source.protocol
            hasObservedTrackCatalog = false
            currentSubtitleCatalogFailure = null
            terminalErrorReportedForCurrentSource = false
            currentDrmRuntimeErrorCount = 0
            cancelFirstFrameWatchdog()
            firstFrameRenderedForCurrentSource = false
            firstFrameGate.advanceEpoch()
            recordBenchmark("source_load_start")
            VesperNativeLibrary.ensureLoaded()

            val handle = VesperNativeJni.createSession(source.uri)
            check(handle != 0L) { "native session handle must not be zero" }
            sessionHandle = handle
            val sourceNormalizerOpen =
                openPreparedSourceNormalizerResourceForPlayback(
                    source,
                    prepared = preparedSourceNormalizer,
                )
            preparedSourceNormalizerConsumed = true
            val normalizedResource = sourceNormalizerOpen.resource
            val playbackSource = normalizedResource?.playbackSource ?: source
            currentDrmDiagnosticsSource = playbackSource
            firstFrameWatchdogSource = playbackSource
            val resolvedResiliencePolicy = resolveResiliencePolicy(source, resiliencePolicy)
            currentRetryMaxAttempts = resolvedResiliencePolicy.retry.resolvedMaxAttempts()
            val resolvedTrackPreferences = resolveTrackPreferences(trackPreferencePolicy)
            currentDeclaredExternalSubtitleCount = playbackSource.externalSubtitles.size
            currentDeclaredExternalSubtitleIds = playbackSource.externalSubtitles.map { it.id }
            currentDeclaredExternalSubtitleDefaultCount =
                playbackSource.externalSubtitles.count { it.isDefault }
            currentSubtitleSelectionModeOrdinal =
                resolvedTrackPreferences.subtitleSelection.toNativePayload().modeOrdinal
            val renderersFactory =
                VesperExternalSubtitleRenderersFactory(appContext)
                    .setExtensionRendererMode(decoderBackend.toExtensionRendererMode())
                    .setMediaCodecSelector(VesperHardwareMediaCodecSelector)

            val loadErrorHandlingPolicy =
                buildLoadErrorHandlingPolicy(playbackSource, resolvedResiliencePolicy.retry) { attempt, delayMs ->
                    VesperNativeJni.reportRetryScheduled(handle, attempt, delayMs)
                }
            val mediaSourceFactory =
                DefaultMediaSourceFactory(appContext)
                    .setDataSourceFactory(
                        buildDataSourceFactory(
                            appContext,
                            resolvedResiliencePolicy.cache,
                            playbackSource.headers,
                        )
                    )
                    .setLoadErrorHandlingPolicy(loadErrorHandlingPolicy)
            val preparedExternalSubtitles =
                prepareExternalSubtitleMediaSources(
                    appContext = appContext,
                    cachePolicy = resolvedResiliencePolicy.cache,
                    sources = playbackSource.externalSubtitles,
                    loadErrorHandlingPolicy = loadErrorHandlingPolicy,
                    primaryUri = playbackSource.uri,
                    primaryHeaders = playbackSource.headers,
                )
            currentExternalSubtitleSources = preparedExternalSubtitles.activeSources
            currentExternalSubtitleIds = preparedExternalSubtitles.activeSources.map { it.id }
            failedExternalSubtitleIds.clear()
            failedExternalSubtitleIds += preparedExternalSubtitles.failures.mapNotNull { it.trackId }
            currentSubtitleResourceFailure = preparedExternalSubtitles.failures.firstOrNull()
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "using decoderBackend=$decoderBackend extensionRendererMode=${decoderBackend.toExtensionRendererMode()} sourceNormalizerRoute=${normalizedResource?.outputRoute ?: "native"}",
            )
            val exoPlayer =
                ExoPlayer.Builder(appContext, renderersFactory)
                    .setLoadControl(buildLoadControl(resolvedResiliencePolicy.buffering))
                    .setMediaSourceFactory(mediaSourceFactory)
                    .build()
            exoPlayer.setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MOVIE)
                    .build(),
                false,
            )
            applyTrackPreferenceDefaults(
                exoPlayer = exoPlayer,
                policy = resolvedTrackPreferences,
                videoEnabled = systemPlaybackVideoEnabled,
            )
            val listener = buildPlayerListener(resolvedTrackPreferences, callbackGeneration)
            val analytics = buildAnalyticsListener(callbackGeneration)
            exoPlayer.addListener(listener)
            exoPlayer.addAnalyticsListener(analytics)
            val mainMediaSource =
                mediaSourceFactory.createMediaSource(
                    buildMediaItem(playbackSource.copy(externalSubtitles = emptyList()))
                )
            val mediaSources = listOf(mainMediaSource) + preparedExternalSubtitles.mediaSources
            exoPlayer.setMediaSource(
                if (mediaSources.size == 1) {
                    mainMediaSource
                } else {
                    MergingMediaSource(*mediaSources.toTypedArray())
                }
            )
            attachedSurface?.takeIf { systemPlaybackVideoEnabled }?.let { surface ->
                Log.i(NATIVE_JNI_BINDINGS_TAG, "reusing attached surface for source=${source.uri}")
                exoPlayer.setVideoSurface(surface)
            }
            exoPlayer.prepare()
            val firstFrameWatchdogRoute =
                FirstFrameWatchdogRoute.systemPlayback(systemPlaybackVideoEnabled)
            scheduleFirstFrameWatchdog(playbackSource, firstFrameGate.currentEpoch, firstFrameWatchdogRoute)
            recordBenchmark("source_load_configured")
            executePreloadWarmupCommands(source)

            player = exoPlayer
            playerListener = listener
            analyticsListener = analytics
            systemPlaybackCoordinator.attachPlayer(exoPlayer)

            pushSnapshotToRust()
            pushTrackStateToRust()
            notifyNativeUpdate()

            return NativeBridgeStartup(
                subtitle = normalizedResource?.subtitle ?: i18n.sourceSubtitle(source),
                pluginDiagnostics =
                    normalizedResource?.diagnostics
                        ?: sourceNormalizerOpen.diagnostics,
            )
        } catch (error: Throwable) {
            if (!preparedSourceNormalizerConsumed) {
                runCatching {
                    disposePreparedSourceNormalizerResource(preparedSourceNormalizer)
                }.onFailure { disposeError ->
                    Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to dispose unconsumed source normalizer resource", disposeError)
                }
            }
            throw error
        }
    }

    override fun openNativeFramePipeline(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?>? {
        closeNativeFramePipeline()
        VesperNativeLibrary.ensureLoaded()
        val packetSource = openNativeFramePacketSource(source)
        var keepPacketSource = false
        try {
            val json =
                VesperNativeJni.openNativeFramePipeline(
                    packetSource.source.uri,
                    sourceNormalizerConfiguration.modeOrdinal,
                    sourceNormalizerConfiguration.pluginLibraryPaths.toTypedArray(),
                    sourceNormalizerConfiguration.runtimeProfile,
                    nativeFramePipelineConfiguration.modeWireName,
                    nativeFramePipelineConfiguration.decoderPluginLibraryPaths.toTypedArray(),
                    nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths.toTypedArray(),
                    nativeFramePipelineConfiguration.maxInFlightFrames ?: 0,
                    surfaceKind.nativeFramePresenterProfileWireName,
                ) ?: return null
            val opened = parseNativeFramePipelineJson(json) ?: return null
            val handle = (opened["handle"] as? Number)?.toLong() ?: 0L
            check(handle != 0L) { "native-frame pipeline handle must not be zero" }
            nativeFramePipelineHandle = handle
            nativeFramePipelineStatus = opened
            currentNativeFramePacketSource = packetSource
            keepPacketSource = true
            nativeFramePipelineOwnsSurface = true
            player?.clearVideoSurface()
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "opened native-frame pipeline handle=$handle route=${opened["route"]}",
            )
            attachedSurface?.let { surface ->
                attachNativeFramePipelineSurface(surface, surfaceKind)
            }
            return opened
        } finally {
            if (!keepPacketSource) {
                packetSource.close()
            }
        }
    }

    override fun advanceNativeFramePipeline(): Map<String, Any?>? {
        val handle = nativeFramePipelineHandle ?: return null
        val json = VesperNativeJni.advanceNativeFramePipeline(handle) ?: return null
        return parseNativeFramePipelineJson(json)?.let(::rememberNativeFramePipelineStatus)
    }

    override fun releaseNativeFramePipelineFrame(
        frameHandle: Long,
        presented: Boolean,
    ): Map<String, Any?>? {
        val handle = nativeFramePipelineHandle ?: return null
        val json =
            VesperNativeJni.releaseNativeFramePipelineFrame(handle, frameHandle, presented)
                ?: return null
        return parseNativeFramePipelineJson(json)?.let(::rememberNativeFramePipelineStatus)
    }

    override fun attachNativeFramePipelineSurface(
        surface: Surface,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?>? {
        val handle = nativeFramePipelineHandle ?: return null
        val json =
                VesperNativeJni.attachNativeFramePipelineSurface(
                    handle,
                    surface,
                    surfaceKind.nativeFramePresenterProfileWireName,
                ) ?: return null
        return parseNativeFramePipelineJson(json)?.let { status ->
            val mergedStatus = rememberNativeFramePipelineStatus(status)
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "native-frame pipeline surface attached " +
                    "surfaceAttached=${mergedStatus["surfaceAttached"]} " +
                    "presenterReady=${mergedStatus["presenterReady"]} " +
                    "presenterState=${mergedStatus["presenterState"]}",
            )
            mergedStatus
        }
    }

    override fun detachNativeFramePipelineSurface(): Map<String, Any?>? {
        val handle = nativeFramePipelineHandle ?: return null
        val json = VesperNativeJni.detachNativeFramePipelineSurface(handle) ?: return null
        return parseNativeFramePipelineJson(json)?.let(::rememberNativeFramePipelineStatus)
    }

    override fun flushNativeFramePipeline(): Map<String, Any?>? {
        val handle = nativeFramePipelineHandle ?: return null
        val json = VesperNativeJni.flushNativeFramePipeline(handle) ?: return null
        return parseNativeFramePipelineJson(json)?.let(::rememberNativeFramePipelineStatus)
    }

    override fun seekNativeFramePipeline(positionMs: Long): Map<String, Any?>? {
        val handle = nativeFramePipelineHandle ?: return null
        val json = VesperNativeJni.seekNativeFramePipeline(handle, positionMs) ?: return null
        return parseNativeFramePipelineJson(json)?.let(::rememberNativeFramePipelineStatus)
    }

    override fun currentNativeFramePipelineStatus(): Map<String, Any?>? =
        nativeFramePipelineStatus

    private fun rememberNativeFramePipelineStatus(status: Map<String, Any?>): Map<String, Any?> {
        val previous = nativeFramePipelineStatus.orEmpty()
        val retainedSurfaceState =
            listOf(
                "presenterReady",
                "presenterConfigured",
                "presenterState",
                "surfaceAttached",
                "surfaceProfile",
            )
                .mapNotNull { key ->
                    if (status.containsKey(key)) null else previous[key]?.let { key to it }
                }
                .toMap()
        val mergedStatus = retainedSurfaceState + status
        nativeFramePipelineStatus = mergedStatus
        return mergedStatus
    }

    override fun closeNativeFramePipeline() {
        val handle: Long?
        synchronized(this) {
            handle = nativeFramePipelineHandle
            if (handle == null) {
                return
            }
            nativeFramePipelineHandle = null
        }
        nativeFramePipelineStatus = null
        nativeFramePipelineOwnsSurface = false
        if (handle != null) {
            runCatching { VesperNativeJni.closeNativeFramePipeline(handle) }
                .onFailure { error ->
                    Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to close native-frame pipeline session", error)
                }
            closeCurrentNativeFramePacketSource()
            attachedSurface?.let { surface ->
                if (surface.isValid) {
                    player?.setVideoSurface(surface)
                } else {
                    Log.i(NATIVE_JNI_BINDINGS_TAG, "native-frame close skipped restoring invalid Surface")
                    player?.clearVideoSurface()
                    attachedSurface = null
                }
            }
        } else {
            closeCurrentNativeFramePacketSource()
        }
    }

    override fun setOnSubtitleCuesListener(listener: ((List<Cue>) -> Unit)?) {
        subtitleCuesListener = listener
        if (listener != null) {
            listener(player?.currentCues?.cues.orEmpty())
        }
    }

    /**
     * Installs the structured track-selection failure callback. Set to `null`
     * to clear. See [trackSelectionFailureListener].
     */
    override fun setOnTrackSelectionFailureListener(
        listener: ((NativeTrackSelectionFailure) -> Unit)?,
    ) {
        trackSelectionFailureListener = listener
    }

    override fun dispose() {
        dispose(stopSourceNormalizerLoopbackServer = true)
    }

    override fun invalidateSystemPlaybackCallbacks() {
        systemPlaybackCallbackGeneration.incrementAndGet()
        localBridgeEvents.clear()
    }

    private fun dispose(stopSourceNormalizerLoopbackServer: Boolean) {
        if (!isDisposed.compareAndSet(false, true)) {
            return
        }
        systemPlaybackCallbackGeneration.incrementAndGet()
        Log.i(NATIVE_JNI_BINDINGS_TAG, "dispose")
        closeNativeFramePipeline()
        preloadCoordinator.dispose()
        detachSurface()
        playerListener?.let { listener ->
            player?.removeListener(listener)
        }
        playerListener = null
        subtitleCuesListener = null
        trackSelectionFailureListener = null
        currentSubtitleCatalogFailure = null
        analyticsListener?.let { listener ->
            player?.removeAnalyticsListener(listener)
        }
        analyticsListener = null
        systemPlaybackCoordinator.attachPlayer(null)
        val handle = sessionHandle
        val playerToRelease = player
        currentSourceNormalizerResource?.let { resource ->
            detachPlayerFromSourceNormalizerResource(resource, playerToRelease)
        }
        try {
            runCatching { playerToRelease?.release() }
                .onFailure { error -> Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to release ExoPlayer", error) }
        } finally {
            player = null
            if (handle != null) {
                // Null sessionHandle before the JNI call so that concurrent
                // dispatchRustCommand invocations see a null handle and bail
                // out rather than passing a stale handle to native code.
                sessionHandle = null
                runCatching { VesperNativeJni.disposeSession(handle) }
                    .onFailure { error -> Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to dispose native session", error) }
            }
            closeCurrentSourceNormalizerResource()
            if (stopSourceNormalizerLoopbackServer) {
                sourceNormalizerLoopbackServer.stop()
            }
        }
        currentTrackCatalogState = VesperTrackCatalog.Empty
        currentTrackSelectionState = VesperTrackSelectionSnapshot()
        currentAppliedSubtitleSelectionState = VesperTrackSelection.disabled()
        currentAdvertisedSubtitleTrackCountState = 0
        currentExternalSubtitleIds = emptyList()
        currentDeclaredExternalSubtitleIds = emptyList()
        currentExternalSubtitleSources = emptyList()
        failedExternalSubtitleIds.clear()
        currentSubtitleResourceFailure = null
        currentDeclaredExternalSubtitleCount = 0
        currentDeclaredExternalSubtitleDefaultCount = 0
        currentSubtitleSelectionModeOrdinal = NativeTrackSelectionMode.Disabled.ordinal
        trackSelectionChangeGenerationState = 0L
        hasObservedTrackCatalog = false
        currentEffectiveVideoTrackIdState = null
        currentVideoVariantObservationState = null
        currentVideoLayoutState = null
        currentVideoDecoderName = null
        currentRuntimeHdrEvidence = null
        currentRuntimeSessionProbe = null
        currentDrmDiagnosticsSource = null
        currentRetryMaxAttempts = null
        currentDrmRuntimeErrorCount = 0
        terminalErrorReportedForCurrentSource = false
        cancelFirstFrameWatchdog()
        firstFrameRenderedForCurrentSource = false
        currentBenchmarkSourceProtocol = null
    }

    override fun refreshSnapshot() {
        if (isDisposed.get()) {
            return
        }
        Trace.beginSection("VesperRefresh#pushSnapshotToRust")
        try {
            pushSnapshotToRust()
        } finally {
            Trace.endSection()
        }
    }

    override fun sampleTimeline(): TimelineUiState? {
        if (isDisposed.get()) {
            return null
        }
        val handle = sessionHandle ?: return null
        val exoPlayer = player ?: return null
        val sample = exoPlayer.currentTimelineSample()
        return VesperNativeJni.sampleTimeline(
            sessionHandle = handle,
            positionMs = sample.timelinePositionMs,
            durationMs = sample.durationMs,
            isLive = sample.isLive,
            isSeekable = sample.isSeekable,
            seekableStartMs = sample.seekableStartMs,
            seekableEndMs = sample.seekableEndMs,
            liveEdgeMs = sample.liveEdgeMs,
        )
    }

    override fun currentTrackCatalog(): VesperTrackCatalog = currentTrackCatalogState

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = currentTrackSelectionState

    override fun currentAppliedSubtitleSelection(): VesperTrackSelection =
        currentAppliedSubtitleSelectionState

    override fun currentAdvertisedSubtitleTrackCount(): Int =
        currentAdvertisedSubtitleTrackCountState

    override val sourceCallbackGeneration: Long
        get() = systemPlaybackCallbackGeneration.get()

    override val subtitleSelectionCommandGeneration: Long
        get() = subtitleSelectionCommandGenerationState.get()

    override val trackSelectionChangeGeneration: Long
        get() = trackSelectionChangeGenerationState

    override fun isTrackCatalogReady(): Boolean = hasObservedTrackCatalog

    override fun currentSubtitleCatalogFailure(): NativeTrackSelectionFailure? {
        // A filtered DASH/HLS Tracks snapshot can arrive before its typed
        // manifest. Do not expose an identity/resource failure from that
        // provisional snapshot as a terminal catalog failure; the catalog is
        // still loading and the manifest remains the source of truth.
        if (subtitleManifestIsRequired(currentSourceProtocol) && !hasObservedTrackCatalog) {
            return null
        }
        return currentSubtitleCatalogFailure ?: currentSubtitleResourceFailure
    }

    override fun currentEffectiveVideoTrackId(): String? = currentEffectiveVideoTrackIdState

    override fun currentVideoVariantObservation(): VesperVideoVariantObservation? =
        currentVideoVariantObservationState

    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = currentVideoLayoutState

    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) {
        updateListener = listener
    }

    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) {
        if (isDisposed.get()) {
            return
        }
        Log.i(NATIVE_JNI_BINDINGS_TAG, "attachSurface kind=$surfaceKind")
        recordBenchmark("surface_attach", mapOf("surfaceKind" to surfaceKind.name))
        attachedSurface = surface
        if (nativeFramePipelineOwnsSurface) {
            player?.clearVideoSurface()
        } else {
            player?.setVideoSurface(surface)
        }
        sessionHandle?.let { handle ->
            VesperNativeJni.attachSurface(handle, surface, surfaceKind.ordinal)
        }
        attachNativeFramePipelineSurface(surface, surfaceKind)
        pushSnapshotToRust()
        notifyNativeUpdate()
    }

    override fun detachSurface() {
        if (isDisposed.get()) {
            return
        }
        Log.i(NATIVE_JNI_BINDINGS_TAG, "detachSurface")
        recordBenchmark("surface_detach")
        player?.clearVideoSurface()
        attachedSurface = null
        sessionHandle?.let(VesperNativeJni::detachSurface)
        detachNativeFramePipelineSurface()
        notifyNativeUpdate()
    }

    override fun pollSnapshot(): NativeBridgeSnapshot? =
        if (isDisposed.get()) {
            null
        } else {
            sessionHandle?.let(VesperNativeJni::pollSnapshot)
        }

    override fun drainEvents(): List<NativeBridgeEvent> =
        if (isDisposed.get()) {
            emptyList()
        } else {
            val localEvents = localBridgeEvents.toList()
            localBridgeEvents.clear()
            localEvents + (sessionHandle?.let { VesperNativeJni.drainEvents(it).toList() } ?: emptyList())
        }

    override fun play() {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "play")
        recordBenchmark("native_play_command")
        dispatchRustCommand { handle -> VesperNativeJni.play(handle) }
    }

    override fun pause() {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "pause")
        recordBenchmark("native_pause_command")
        dispatchRustCommand { handle -> VesperNativeJni.pause(handle) }
    }

    override fun stop() {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "stop")
        recordBenchmark("native_stop_command")
        dispatchRustCommand { handle -> VesperNativeJni.stop(handle) }
    }

    override fun seekTo(positionMs: Long) {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "seekTo positionMs=$positionMs")
        recordBenchmark("native_seek_command", mapOf("positionMs" to positionMs.toString()))
        cancelFirstFrameWatchdog()
        dispatchRustCommand { handle -> VesperNativeJni.seekTo(handle, positionMs) }
    }

    override fun setPlaybackRate(rate: Float) {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "setPlaybackRate rate=$rate")
        recordBenchmark("native_set_playback_rate_command", mapOf("rate" to rate.toString()))
        dispatchRustCommand { handle -> VesperNativeJni.setPlaybackRate(handle, rate) }
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "setVideoTrackSelection mode=${selection.mode} trackId=${selection.trackId}")
        recordBenchmark("native_set_video_track_selection_command")
        dispatchRustCommand { handle ->
            VesperNativeJni.setVideoTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "setAudioTrackSelection mode=${selection.mode} trackId=${selection.trackId}")
        recordBenchmark("native_set_audio_track_selection_command")
        dispatchRustCommand { handle ->
            VesperNativeJni.setAudioTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        Log.i(
            NATIVE_JNI_BINDINGS_TAG,
            "setSubtitleTrackSelection mode=${selection.mode} trackId=${selection.trackId}",
        )
        recordBenchmark("native_set_subtitle_track_selection_command")
        val commandGeneration = subtitleSelectionCommandGenerationState.incrementAndGet()
        dispatchRustCommand(subtitleCommandGeneration = commandGeneration) { handle ->
            val errorJson =
                VesperNativeJni.setSubtitleTrackSelection(handle, selection.toNativePayload())
            if (!errorJson.isNullOrBlank()) {
                throw subtitleNativeErrorFromJson(errorJson)
            }
        }
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        Log.i(
            NATIVE_JNI_BINDINGS_TAG,
            "setAbrPolicy mode=${policy.mode} trackId=${policy.trackId} maxBitRate=${policy.maxBitRate} maxWidth=${policy.maxWidth} maxHeight=${policy.maxHeight}",
        )
        recordBenchmark("native_set_abr_policy_command", mapOf("mode" to policy.mode.name))
        dispatchRustCommand { handle ->
            VesperNativeJni.setAbrPolicy(handle, policy.toNativePayload())
        }
    }

    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) {
        Log.i(
            NATIVE_JNI_BINDINGS_TAG,
            "configureSystemPlayback enabled=${configuration.enabled} backgroundMode=${configuration.backgroundMode} showSystemControls=${configuration.showSystemControls}",
        )
        systemPlaybackCoordinator.configure(configuration)
        notifyNativeUpdate()
    }

    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "updateSystemPlaybackMetadata title=${metadata.title}")
        systemPlaybackCoordinator.updateMetadata(metadata)
        notifyNativeUpdate()
    }

    override fun clearSystemPlayback() {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "clearSystemPlayback")
        systemPlaybackCoordinator.clear()
        notifyNativeUpdate()
    }

}

/**
 * Enables Media3's render-time decoding for Vesper's side-loaded subtitle
 * sources. Media3 1.9 keeps this legacy path disabled by default even though
 * external WebVTT, SRT, and ASS samples still use it.
 */
@OptIn(UnstableApi::class)
internal class VesperExternalSubtitleRenderersFactory(
    context: Context,
) : DefaultRenderersFactory(context) {
    @Suppress("DEPRECATION")
    override fun buildTextRenderers(
        context: Context,
        output: TextOutput,
        outputLooper: Looper,
        extensionRendererMode: Int,
        out: ArrayList<Renderer>,
    ) {
        out.add(
            TextRenderer(output, outputLooper).apply {
                experimentalSetLegacyDecodingEnabled(true)
            },
        )
    }
}
