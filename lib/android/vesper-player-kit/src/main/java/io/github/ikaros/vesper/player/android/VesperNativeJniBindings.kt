package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
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
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistTracker
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo
import java.io.File
import java.net.URI
import java.util.concurrent.atomic.AtomicBoolean
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
    internal var player: ExoPlayer? = null
    internal var playerListener: Player.Listener? = null
    internal var analyticsListener: AnalyticsListener? = null
    @Volatile
    internal var attachedSurface: Surface? = null
    internal var nativeFramePipelineOwnsSurface = false
    internal var updateListener: (() -> Unit)? = null
    internal var currentTrackCatalogState: VesperTrackCatalog = VesperTrackCatalog.Empty
    internal var currentTrackSelectionState: VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
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
    internal val localBridgeEvents = ArrayDeque<NativeBridgeEvent>()

    internal fun addLocalBridgeEvent(event: NativeBridgeEvent) {
        if (localBridgeEvents.size >= MAX_LOCAL_BRIDGE_EVENTS) {
            localBridgeEvents.removeFirst()
        }
        localBridgeEvents += event
    }

    companion object {
        private const val MAX_LOCAL_BRIDGE_EVENTS = 256
    }
    internal val preloadCoordinator =
        VesperNativePreloadCoordinator(
            bindings = VesperNativePreloadCoordinator.NativeJniPreloadBindings,
            preloadBudgetPolicy = preloadBudgetPolicy,
        )
    internal val systemPlaybackCoordinator = VesperAndroidSystemPlaybackCoordinator(appContext)
    internal val sourceNormalizerLoopbackServer = VesperSourceNormalizerLoopbackServer()
    internal var currentBenchmarkSourceProtocol: VesperPlayerSourceProtocol? = null
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

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
    ): NativeBridgeStartup {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "initialize source=${source.uri} kind=${source.kind} protocol=${source.protocol}")
        dispose()
        isDisposed.set(false)
        currentBenchmarkSourceProtocol = source.protocol
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
            openSourceNormalizerResourceForPlayback(
                source,
                enabled = systemPlaybackUsesSourceNormalizerResource,
            )
        val normalizedResource = sourceNormalizerOpen.resource
        val playbackSource = normalizedResource?.playbackSource ?: source
        currentDrmDiagnosticsSource = playbackSource
        firstFrameWatchdogSource = playbackSource
        val resolvedResiliencePolicy = resolveResiliencePolicy(source, resiliencePolicy)
        currentRetryMaxAttempts = resolvedResiliencePolicy.retry.resolvedMaxAttempts()
        val resolvedTrackPreferences = resolveTrackPreferences(trackPreferencePolicy)
        val renderersFactory =
            DefaultRenderersFactory(appContext)
                .setExtensionRendererMode(decoderBackend.toExtensionRendererMode())
                .setMediaCodecSelector(VesperHardwareMediaCodecSelector)

        val mediaSourceFactory =
            DefaultMediaSourceFactory(appContext)
                .setDataSourceFactory(
                    buildDataSourceFactory(appContext, resolvedResiliencePolicy.cache, playbackSource.headers)
                )
                .setLoadErrorHandlingPolicy(
                    buildLoadErrorHandlingPolicy(playbackSource, resolvedResiliencePolicy.retry) { attempt, delayMs ->
                        VesperNativeJni.reportRetryScheduled(handle, attempt, delayMs)
                    }
                )
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
        val listener = buildPlayerListener(resolvedTrackPreferences)
        val analytics = buildAnalyticsListener()
        exoPlayer.addListener(listener)
        exoPlayer.addAnalyticsListener(analytics)
        exoPlayer.setMediaItem(buildMediaItem(playbackSource))
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

    override fun dispose() {
        if (!isDisposed.compareAndSet(false, true)) {
            return
        }
        Log.i(NATIVE_JNI_BINDINGS_TAG, "dispose")
        closeNativeFramePipeline()
        preloadCoordinator.dispose()
        detachSurface()
        playerListener?.let { listener ->
            player?.removeListener(listener)
        }
        playerListener = null
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
            sourceNormalizerLoopbackServer.stop()
        }
        currentTrackCatalogState = VesperTrackCatalog.Empty
        currentTrackSelectionState = VesperTrackSelectionSnapshot()
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
        pushSnapshotToRust()
    }

    override fun currentTrackCatalog(): VesperTrackCatalog = currentTrackCatalogState

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = currentTrackSelectionState

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
        dispatchRustCommand { handle ->
            VesperNativeJni.setSubtitleTrackSelection(handle, selection.toNativePayload())
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
