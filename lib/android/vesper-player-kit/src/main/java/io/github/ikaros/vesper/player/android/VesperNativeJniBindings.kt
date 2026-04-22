package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo
import java.io.File
import java.net.URI
import kotlin.math.pow
import kotlin.math.roundToLong

class VesperNativeJniBindings(
    context: Context,
    preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    private val decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
) : VesperNativeBindings {
    private val appContext = context.applicationContext
    private val i18n = VesperPlayerI18n.fromContext(appContext)
    private val mainHandler = Handler(Looper.getMainLooper())

    private var sessionHandle: Long? = null
    private var player: ExoPlayer? = null
    private var playerListener: Player.Listener? = null
    private var analyticsListener: AnalyticsListener? = null
    private var attachedSurface: Surface? = null
    private var updateListener: (() -> Unit)? = null
    private var currentTrackCatalogState: VesperTrackCatalog = VesperTrackCatalog.Empty
    private var currentTrackSelectionState: VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
    private var currentEffectiveVideoTrackIdState: String? = null
    private var currentVideoVariantObservationState: VesperVideoVariantObservation? = null
    private var currentVideoLayoutState: NativeVideoLayoutInfo? = null
    private val preloadCoordinator =
        VesperNativePreloadCoordinator(
            bindings = VesperNativePreloadCoordinator.NativeJniPreloadBindings,
            preloadBudgetPolicy = preloadBudgetPolicy,
        )

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
    ): NativeBridgeStartup {
        Log.i(TAG, "initialize source=${source.uri} kind=${source.kind} protocol=${source.protocol}")
        dispose()
        VesperNativeLibrary.ensureLoaded()

        val handle = VesperNativeJni.createSession(source.uri)
        check(handle != 0L) { "native session handle must not be zero" }
        sessionHandle = handle
        val resolvedResiliencePolicy = resolveResiliencePolicy(source, resiliencePolicy)
        val resolvedTrackPreferences = resolveTrackPreferences(trackPreferencePolicy)
        val renderersFactory =
            DefaultRenderersFactory(appContext)
                .setExtensionRendererMode(decoderBackend.toExtensionRendererMode())

        val mediaSourceFactory =
            DefaultMediaSourceFactory(appContext)
                .setDataSourceFactory(
                    buildDataSourceFactory(appContext, resolvedResiliencePolicy.cache)
                )
                .setLoadErrorHandlingPolicy(
                    buildLoadErrorHandlingPolicy(source, resolvedResiliencePolicy.retry) { attempt, delayMs ->
                        VesperNativeJni.reportRetryScheduled(handle, attempt, delayMs)
                    }
                )
        Log.i(
            TAG,
            "using decoderBackend=$decoderBackend extensionRendererMode=${decoderBackend.toExtensionRendererMode()}",
        )
        val exoPlayer =
            ExoPlayer.Builder(appContext, renderersFactory)
                .setLoadControl(buildLoadControl(resolvedResiliencePolicy.buffering))
                .setMediaSourceFactory(mediaSourceFactory)
                .build()
        applyTrackPreferenceDefaults(exoPlayer, resolvedTrackPreferences)
        val listener = buildPlayerListener(resolvedTrackPreferences)
        val analytics = buildAnalyticsListener()
        exoPlayer.addListener(listener)
        exoPlayer.addAnalyticsListener(analytics)
        exoPlayer.setMediaItem(buildMediaItem(source))
        attachedSurface?.let { surface ->
            Log.i(TAG, "reusing attached surface for source=${source.uri}")
            exoPlayer.setVideoSurface(surface)
        }
        exoPlayer.prepare()
        executePreloadWarmupCommands(source)

        player = exoPlayer
        playerListener = listener
        analyticsListener = analytics

        pushSnapshotToRust()
        pushTrackStateToRust()
        notifyNativeUpdate()

        return NativeBridgeStartup(
            subtitle = i18n.sourceSubtitle(source),
        )
    }

    override fun dispose() {
        Log.i(TAG, "dispose")
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
        player?.release()
        player = null
        sessionHandle?.let(VesperNativeJni::disposeSession)
        sessionHandle = null
        currentTrackCatalogState = VesperTrackCatalog.Empty
        currentTrackSelectionState = VesperTrackSelectionSnapshot()
        currentEffectiveVideoTrackIdState = null
        currentVideoVariantObservationState = null
        currentVideoLayoutState = null
    }

    override fun refreshSnapshot() {
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
        Log.i(TAG, "attachSurface kind=$surfaceKind")
        attachedSurface = surface
        player?.setVideoSurface(surface)
        sessionHandle?.let { handle ->
            VesperNativeJni.attachSurface(handle, surface, surfaceKind.ordinal)
        }
        pushSnapshotToRust()
        notifyNativeUpdate()
    }

    override fun detachSurface() {
        Log.i(TAG, "detachSurface")
        player?.clearVideoSurface()
        attachedSurface = null
        sessionHandle?.let(VesperNativeJni::detachSurface)
        notifyNativeUpdate()
    }

    override fun pollSnapshot(): NativeBridgeSnapshot? =
        sessionHandle?.let(VesperNativeJni::pollSnapshot)

    override fun drainEvents(): List<NativeBridgeEvent> =
        sessionHandle?.let { VesperNativeJni.drainEvents(it).toList() } ?: emptyList()

    override fun play() {
        Log.i(TAG, "play")
        dispatchRustCommand { handle -> VesperNativeJni.play(handle) }
    }

    override fun pause() {
        Log.i(TAG, "pause")
        dispatchRustCommand { handle -> VesperNativeJni.pause(handle) }
    }

    override fun stop() {
        Log.i(TAG, "stop")
        dispatchRustCommand { handle -> VesperNativeJni.stop(handle) }
    }

    override fun seekTo(positionMs: Long) {
        Log.i(TAG, "seekTo positionMs=$positionMs")
        dispatchRustCommand { handle -> VesperNativeJni.seekTo(handle, positionMs) }
    }

    override fun setPlaybackRate(rate: Float) {
        Log.i(TAG, "setPlaybackRate rate=$rate")
        dispatchRustCommand { handle -> VesperNativeJni.setPlaybackRate(handle, rate) }
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        Log.i(TAG, "setVideoTrackSelection mode=${selection.mode} trackId=${selection.trackId}")
        dispatchRustCommand { handle ->
            VesperNativeJni.setVideoTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        Log.i(TAG, "setAudioTrackSelection mode=${selection.mode} trackId=${selection.trackId}")
        dispatchRustCommand { handle ->
            VesperNativeJni.setAudioTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        Log.i(
            TAG,
            "setSubtitleTrackSelection mode=${selection.mode} trackId=${selection.trackId}",
        )
        dispatchRustCommand { handle ->
            VesperNativeJni.setSubtitleTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        Log.i(
            TAG,
            "setAbrPolicy mode=${policy.mode} trackId=${policy.trackId} maxBitRate=${policy.maxBitRate} maxWidth=${policy.maxWidth} maxHeight=${policy.maxHeight}",
        )
        dispatchRustCommand { handle ->
            VesperNativeJni.setAbrPolicy(handle, policy.toNativePayload())
        }
    }

    private fun dispatchRustCommand(action: (Long) -> Unit) {
        val handle = sessionHandle ?: return
        action(handle)
        drainAndApplyNativeCommands()
        pushSnapshotToRust()
        pushTrackStateToRust()
        notifyNativeUpdate()
    }

    private fun drainAndApplyNativeCommands() {
        val handle = sessionHandle ?: return
        val exoPlayer = player ?: return

        VesperNativeJni.drainNativeCommands(handle).forEach { command ->
            when (command) {
                NativePlayerCommand.Play -> {
                    Log.d(TAG, "apply native command: Play")
                    exoPlayer.play()
                }
                NativePlayerCommand.Pause -> {
                    Log.d(TAG, "apply native command: Pause")
                    exoPlayer.pause()
                }
                is NativePlayerCommand.SeekTo -> {
                    Log.d(TAG, "apply native command: SeekTo positionMs=${command.positionMs}")
                    exoPlayer.seekTo(command.positionMs)
                }
                NativePlayerCommand.Stop -> {
                    Log.d(TAG, "apply native command: Stop")
                    exoPlayer.pause()
                    exoPlayer.seekTo(0L)
                }
                is NativePlayerCommand.SetPlaybackRate -> {
                    Log.d(TAG, "apply native command: SetPlaybackRate rate=${command.rate}")
                    exoPlayer.setPlaybackParameters(PlaybackParameters(command.rate))
                }
                is NativePlayerCommand.SetVideoTrackSelection -> {
                    Log.d(
                        TAG,
                        "apply native command: SetVideoTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                    )
                    applyTrackSelectionCommand(
                        exoPlayer = exoPlayer,
                        kind = NativeTrackKind.Video,
                        selection = command.selection,
                    )
                }
                is NativePlayerCommand.SetAudioTrackSelection -> {
                    Log.d(
                        TAG,
                        "apply native command: SetAudioTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                    )
                    applyTrackSelectionCommand(
                        exoPlayer = exoPlayer,
                        kind = NativeTrackKind.Audio,
                        selection = command.selection,
                    )
                }
                is NativePlayerCommand.SetSubtitleTrackSelection -> {
                    Log.d(
                        TAG,
                        "apply native command: SetSubtitleTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                    )
                    applyTrackSelectionCommand(
                        exoPlayer = exoPlayer,
                        kind = NativeTrackKind.Subtitle,
                        selection = command.selection,
                    )
                }
                is NativePlayerCommand.SetAbrPolicy -> {
                    Log.d(
                        TAG,
                        "apply native command: SetAbrPolicy mode=${command.policy.modeOrdinal} trackId=${command.policy.trackId}",
                    )
                    applyAbrPolicyCommand(exoPlayer, command.policy)
                }
            }
        }
    }

    private fun buildPlayerListener(
        trackPreferencePolicy: VesperTrackPreferencePolicy,
    ): Player.Listener =
        object : Player.Listener {
            private var pendingTrackOverrides =
                trackPreferencePolicy.takeIf(::hasTrackBasedPreferenceOverrides)

            override fun onPlaybackStateChanged(playbackState: Int) {
                Log.d(
                    TAG,
                    "onPlaybackStateChanged state=${exoPlaybackStateName(playbackState)} playWhenReady=${player?.playWhenReady}",
                )
                pushSnapshotToRust()
                notifyNativeUpdate()
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                Log.d(TAG, "onPlayWhenReadyChanged playWhenReady=$playWhenReady reason=$reason")
                pushSnapshotToRust()
                notifyNativeUpdate()
            }

            override fun onPlaybackParametersChanged(playbackParameters: PlaybackParameters) {
                Log.d(TAG, "onPlaybackParametersChanged speed=${playbackParameters.speed}")
                pushSnapshotToRust()
                pushTrackStateToRust()
                notifyNativeUpdate()
            }

            override fun onTracksChanged(tracks: Tracks) {
                Log.d(TAG, "onTracksChanged groups=${tracks.groups.size}")
                player?.let { exoPlayer ->
                    pendingTrackOverrides?.let { defaults ->
                        applyTrackPreferenceTrackOverrides(exoPlayer, defaults)
                        pendingTrackOverrides = null
                    }
                }
                pushTrackStateToRust()
                notifyNativeUpdate()
            }

            override fun onTrackSelectionParametersChanged(parameters: TrackSelectionParameters) {
                Log.d(TAG, "onTrackSelectionParametersChanged overrides=${parameters.overrides.size}")
                pushTrackStateToRust()
                notifyNativeUpdate()
            }

            override fun onVideoSizeChanged(videoSize: VideoSize) {
                Log.d(
                    TAG,
                    "onVideoSizeChanged width=${videoSize.width} height=${videoSize.height} pixelRatio=${videoSize.pixelWidthHeightRatio}",
                )
                currentVideoLayoutState = videoSize.toNativeVideoLayoutInfo()
                notifyNativeUpdate()
            }

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int,
            ) {
                if (reason == Player.DISCONTINUITY_REASON_SEEK) {
                    sessionHandle?.let { handle ->
                        VesperNativeJni.reportSeekCompleted(handle, newPosition.positionMs)
                    }
                }
                Log.d(
                    TAG,
                    "onPositionDiscontinuity reason=$reason positionMs=${newPosition.positionMs}",
                )
                pushSnapshotToRust()
                notifyNativeUpdate()
            }

            override fun onPlayerError(error: PlaybackException) {
                Log.e(TAG, "onPlayerError ${error.errorCodeName}: ${error.message}", error)
                val classified = classifyPlaybackException(error)
                sessionHandle?.let { handle ->
                    VesperNativeJni.reportError(
                        handle,
                        classified.codeOrdinal,
                        classified.categoryOrdinal,
                        classified.retriable,
                        error.message ?: error.errorCodeName,
                    )
                }
                pushSnapshotToRust()
                notifyNativeUpdate()
            }
        }

    private fun buildAnalyticsListener(): AnalyticsListener =
        object : AnalyticsListener {
            override fun onVideoInputFormatChanged(
                eventTime: AnalyticsListener.EventTime,
                format: Format,
                decoderReuseEvaluation: DecoderReuseEvaluation?,
            ) {
                Log.d(
                    TAG,
                    "onVideoInputFormatChanged formatId=${format.id} bitrate=${format.bitrate} width=${format.width} height=${format.height}",
                )
                pushTrackStateToRust()
                notifyNativeUpdate()
            }
        }

    private fun pushSnapshotToRust() {
        val handle = sessionHandle ?: return
        val exoPlayer = player ?: return
        val durationMs = exoPlayer.duration.normalizedDurationMs()
        val isLive = exoPlayer.isCurrentMediaItemLive
        val isSeekable = exoPlayer.isCurrentMediaItemSeekable
        val seekableEndMs = if (isLive && isSeekable && durationMs >= 0L) {
            durationMs
        } else {
            C.TIME_UNSET
        }
        val liveEdgeMs = when {
            !isLive -> C.TIME_UNSET
            seekableEndMs >= 0L -> seekableEndMs
            else -> exoPlayer.currentLiveOffset.normalizedOptionalMs()?.let {
                (exoPlayer.currentPosition.coerceAtLeast(0L) + it).coerceAtLeast(0L)
            } ?: C.TIME_UNSET
        }
        Log.d(
            TAG,
            "pushSnapshotToRust state=${exoPlaybackStateName(exoPlayer.playbackState)} live=$isLive seekable=$isSeekable positionMs=${exoPlayer.currentPosition} durationMs=$durationMs liveEdgeMs=$liveEdgeMs",
        )
        VesperNativeJni.applyExoSnapshot(
            handle,
            exoPlaybackStateOrdinal(exoPlayer.playbackState),
            exoPlayer.playWhenReady,
            exoPlayer.playbackParameters.speed,
            exoPlayer.currentPosition.coerceAtLeast(0L),
            durationMs,
            isLive,
            isSeekable,
            if (seekableEndMs >= 0L) 0L else C.TIME_UNSET,
            seekableEndMs,
            liveEdgeMs,
        )
    }

    private fun pushTrackStateToRust() {
        val handle = sessionHandle ?: return
        val exoPlayer = player ?: return
        val trackCatalog = collectTrackCatalog(exoPlayer.currentTracks)
        val trackSelection =
            collectTrackSelection(exoPlayer.currentTracks, exoPlayer.trackSelectionParameters)
        val publicTrackCatalog = trackCatalog.toPublicTrackCatalog()
        val videoVariantObservation = resolveVideoVariantObservation(exoPlayer.videoFormat)
        val effectiveVideoTrackId = resolveEffectiveVideoTrackId(
            publicTrackCatalog.videoTracks,
            exoPlayer.videoFormat,
        )
        currentTrackCatalogState = publicTrackCatalog
        currentTrackSelectionState = trackSelection.toPublicTrackSelectionSnapshot()
        currentEffectiveVideoTrackIdState = effectiveVideoTrackId
        currentVideoVariantObservationState = videoVariantObservation
        Log.d(
            TAG,
            "pushTrackStateToRust tracks=${trackCatalog.tracks.size} adaptiveVideo=${trackCatalog.adaptiveVideo} adaptiveAudio=${trackCatalog.adaptiveAudio} videoMode=${trackSelection.video.modeOrdinal} audioMode=${trackSelection.audio.modeOrdinal} subtitleMode=${trackSelection.subtitle.modeOrdinal} abrMode=${trackSelection.abrPolicy.modeOrdinal} effectiveVideoTrackId=$effectiveVideoTrackId observation=$videoVariantObservation",
        )
        VesperNativeJni.applyTrackState(handle, trackCatalog, trackSelection)
    }

    private fun executePreloadWarmupCommands(source: VesperPlayerSource) {
        preloadCoordinator.planCurrentSource(source).forEach { command ->
            when (command) {
                is NativePreloadCommand.Start -> runWarmup(command.task)
                is NativePreloadCommand.Cancel -> Unit
            }
        }
    }

    private fun runWarmup(task: NativePreloadTask) {
        val source = currentSourceOrFallback(task.sourceUri)
        val resolvedResiliencePolicy = resolveResiliencePolicy(source, VesperPlaybackResiliencePolicy())
        val dataSourceFactory = buildDataSourceFactory(appContext, resolvedResiliencePolicy.cache)
        val dataSource = dataSourceFactory.createDataSource()

        val readLength =
            task.expectedMemoryBytes.coerceAtLeast(1L).coerceAtMost(DEFAULT_PRELOAD_WARMUP_READ_BYTES.toLong())
        val dataSpec =
            DataSpec.Builder()
                .setUri(task.sourceUri)
                .setLength(readLength)
                .build()

        runCatching {
            dataSource.open(dataSpec)
            val buffer = ByteArray(DEFAULT_PRELOAD_WARMUP_READ_BYTES)
            dataSource.read(buffer, 0, buffer.size)
        }.onSuccess {
            preloadCoordinator.complete(task.taskId)
        }.onFailure { error ->
            preloadCoordinator.fail(
                task.taskId,
                NativeBridgeEvent.Error(
                    message = error.message ?: "android preload warmup failed",
                    codeOrdinal = BACKEND_FAILURE_ORDINAL,
                    categoryOrdinal = PLATFORM_CATEGORY_ORDINAL,
                    retriable = false,
                ),
            )
        }

        runCatching { dataSource.close() }
    }

    private fun currentSourceOrFallback(uri: String): VesperPlayerSource {
        return VesperPlayerSource(
            uri = uri,
            label = URI(uri).path.substringAfterLast('/').ifBlank { uri },
            kind = inferSourceKind(uri),
            protocol = inferSourceProtocol(uri),
        )
    }

    private fun notifyNativeUpdate() {
        val listener = updateListener ?: return
        if (Looper.myLooper() == Looper.getMainLooper()) {
            listener.invoke()
        } else {
            mainHandler.post { listener.invoke() }
        }
    }
}

private fun buildLoadControl(
    bufferingPolicy: NativeBufferingPolicy,
): DefaultLoadControl {
    val builder = DefaultLoadControl.Builder()
    val resolved = resolveBufferingPolicy(bufferingPolicy) ?: return builder.build()
    return builder
        .setBufferDurationsMs(
            resolved.minBufferMs,
            resolved.maxBufferMs,
            resolved.bufferForPlaybackMs,
            resolved.bufferForPlaybackAfterRebufferMs,
        )
        .build()
}

private fun buildLoadErrorHandlingPolicy(
    source: VesperPlayerSource,
    retryPolicy: NativeRetryPolicy,
    onRetryScheduled: (attempt: Int, delayMs: Long) -> Unit,
): DefaultLoadErrorHandlingPolicy =
    when (source.kind) {
        VesperPlayerSourceKind.Local -> DefaultLoadErrorHandlingPolicy(0)
        VesperPlayerSourceKind.Remote -> VesperLoadErrorHandlingPolicy(retryPolicy, onRetryScheduled)
    }

private fun resolveBufferingPolicy(
    bufferingPolicy: NativeBufferingPolicy,
): ResolvedBufferingPolicy? {
    val minBufferMs = bufferingPolicy.minBufferMs.takeIf { bufferingPolicy.hasMinBufferMs }
    val maxBufferMs = bufferingPolicy.maxBufferMs.takeIf { bufferingPolicy.hasMaxBufferMs }
    val bufferForPlaybackMs =
        bufferingPolicy.bufferForPlaybackMs.takeIf { bufferingPolicy.hasBufferForPlaybackMs }
    val bufferForPlaybackAfterRebufferMs =
        bufferingPolicy.bufferForPlaybackAfterRebufferMs.takeIf {
            bufferingPolicy.hasBufferForPlaybackAfterRebufferMs
        }

    if (
        minBufferMs == null ||
        maxBufferMs == null ||
        bufferForPlaybackMs == null ||
        bufferForPlaybackAfterRebufferMs == null
    ) {
        return null
    }

    return ResolvedBufferingPolicy(
        minBufferMs = minBufferMs.coerceAtLeast(0),
        maxBufferMs = maxBufferMs.coerceAtLeast(minBufferMs),
        bufferForPlaybackMs = bufferForPlaybackMs.coerceAtLeast(0),
        bufferForPlaybackAfterRebufferMs = bufferForPlaybackAfterRebufferMs.coerceAtLeast(0),
    )
}

private data class ResolvedBufferingPolicy(
    val minBufferMs: Int,
    val maxBufferMs: Int,
    val bufferForPlaybackMs: Int,
    val bufferForPlaybackAfterRebufferMs: Int,
)

private fun media3MinimumRetryCount(retryPolicy: NativeRetryPolicy): Int {
    val maxAttempts = retryPolicy.maxAttempts.takeIf { retryPolicy.hasMaxAttempts }
    return when {
        maxAttempts == null -> Int.MAX_VALUE
        maxAttempts <= 0 -> 0
        else -> maxAttempts
    }
}

private class VesperLoadErrorHandlingPolicy(
    private val retryPolicy: NativeRetryPolicy,
    private val onRetryScheduled: (attempt: Int, delayMs: Long) -> Unit,
) : DefaultLoadErrorHandlingPolicy(media3MinimumRetryCount(retryPolicy)) {
    override fun getRetryDelayMsFor(loadErrorInfo: LoadErrorInfo): Long {
        val superDelayMs = super.getRetryDelayMsFor(loadErrorInfo)
        if (superDelayMs == C.TIME_UNSET) {
            return C.TIME_UNSET
        }

        val maxAttempts = retryPolicy.maxAttempts.takeIf { retryPolicy.hasMaxAttempts }
        if (maxAttempts != null && loadErrorInfo.errorCount > maxAttempts) {
            return C.TIME_UNSET
        }

        val backoff =
            if (retryPolicy.hasBackoff) {
                VesperRetryBackoff.entries.getOrElse(retryPolicy.backoffOrdinal) {
                    VesperRetryBackoff.Linear
                }
            } else {
                VesperRetryBackoff.Linear
            }
        val step = when (backoff) {
            VesperRetryBackoff.Fixed -> 1.0
            VesperRetryBackoff.Linear -> loadErrorInfo.errorCount.toDouble()
            VesperRetryBackoff.Exponential ->
                2.0.pow((loadErrorInfo.errorCount - 1).coerceAtLeast(0).toDouble())
        }
        val baseDelayMs = retryPolicy.baseDelayMs.takeIf { retryPolicy.hasBaseDelayMs } ?: 1_000L
        val maxDelayMs = retryPolicy.maxDelayMs.takeIf { retryPolicy.hasMaxDelayMs } ?: 5_000L
        val computedDelay = (baseDelayMs.toDouble() * step).roundToLong()
        val resolvedDelay = computedDelay.coerceAtMost(maxDelayMs).coerceAtLeast(0L)
        onRetryScheduled(loadErrorInfo.errorCount, resolvedDelay)
        return resolvedDelay
    }
}

private fun VideoSize.toNativeVideoLayoutInfo(): NativeVideoLayoutInfo? {
    if (width <= 0 || height <= 0) {
        return null
    }

    return NativeVideoLayoutInfo(
        width = width,
        height = height,
        pixelWidthHeightRatio = pixelWidthHeightRatio.takeIf { it > 0f } ?: 1.0f,
    )
}

private fun exoPlaybackStateOrdinal(playbackState: Int): Int =
    when (playbackState) {
        Player.STATE_BUFFERING -> 1
        Player.STATE_READY -> 2
        Player.STATE_ENDED -> 3
        else -> 0
    }

private fun VesperTrackSelection.toNativePayload(): NativeTrackSelectionPayload =
    NativeTrackSelectionPayload(
        modeOrdinal =
            when (mode) {
                VesperTrackSelectionMode.Auto -> NativeTrackSelectionMode.Auto.ordinal
                VesperTrackSelectionMode.Disabled -> NativeTrackSelectionMode.Disabled.ordinal
                VesperTrackSelectionMode.Track -> NativeTrackSelectionMode.Track.ordinal
            },
        trackId = trackId,
    )

private fun NativeTrackKind.toPublicKind(): VesperMediaTrackKind =
    when (this) {
        NativeTrackKind.Video -> VesperMediaTrackKind.Video
        NativeTrackKind.Audio -> VesperMediaTrackKind.Audio
        NativeTrackKind.Subtitle -> VesperMediaTrackKind.Subtitle
    }

private fun NativeTrackInfo.toPublicTrack(): VesperMediaTrack? {
    val kind = NativeTrackKind.entries.getOrNull(kindOrdinal)?.toPublicKind() ?: return null
    return VesperMediaTrack(
        id = id,
        kind = kind,
        label = label,
        language = language,
        codec = codec,
        bitRate = bitRate.takeIf { hasBitRate },
        width = width.takeIf { hasWidth },
        height = height.takeIf { hasHeight },
        frameRate = frameRate.takeIf { hasFrameRate },
        channels = channels.takeIf { hasChannels },
        sampleRate = sampleRate.takeIf { hasSampleRate },
        isDefault = isDefault,
        isForced = isForced,
    )
}

private fun NativeTrackCatalog.toPublicTrackCatalog(): VesperTrackCatalog =
    VesperTrackCatalog(
        tracks = tracks.mapNotNull { it.toPublicTrack() },
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
    )

private fun NativeTrackSelectionPayload.toPublicTrackSelection(): VesperTrackSelection {
    val mode = NativeTrackSelectionMode.entries.getOrNull(modeOrdinal) ?: NativeTrackSelectionMode.Auto
    return when (mode) {
        NativeTrackSelectionMode.Auto -> VesperTrackSelection.auto()
        NativeTrackSelectionMode.Disabled -> VesperTrackSelection.disabled()
        NativeTrackSelectionMode.Track -> trackId?.let(VesperTrackSelection::track) ?: VesperTrackSelection.auto()
    }
}

private fun NativeAbrPolicyPayload.toPublicAbrPolicy(): VesperAbrPolicy {
    val mode = NativeAbrMode.entries.getOrNull(modeOrdinal) ?: NativeAbrMode.Auto
    return when (mode) {
        NativeAbrMode.Auto -> VesperAbrPolicy.auto()
        NativeAbrMode.Constrained ->
            VesperAbrPolicy.constrained(
                maxBitRate = maxBitRate.takeIf { hasMaxBitRate },
                maxWidth = maxWidth.takeIf { hasMaxWidth },
                maxHeight = maxHeight.takeIf { hasMaxHeight },
            )
        NativeAbrMode.FixedTrack ->
            trackId?.let(VesperAbrPolicy::fixedTrack) ?: VesperAbrPolicy.auto()
    }
}

private fun NativeTrackSelectionSnapshotPayload.toPublicTrackSelectionSnapshot():
    VesperTrackSelectionSnapshot =
    VesperTrackSelectionSnapshot(
        video = video.toPublicTrackSelection(),
        audio = audio.toPublicTrackSelection(),
        subtitle = subtitle.toPublicTrackSelection(),
        abrPolicy = abrPolicy.toPublicAbrPolicy(),
    )

private fun NativeTrackPreferencePolicy.toPublicTrackPreferencePolicy():
    VesperTrackPreferencePolicy =
    VesperTrackPreferencePolicy(
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectSubtitlesByDefault = selectSubtitlesByDefault,
        selectUndeterminedSubtitleLanguage = selectUndeterminedSubtitleLanguage,
        audioSelection = audioSelection.toPublicTrackSelection(),
        subtitleSelection = subtitleSelection.toPublicTrackSelection(),
        abrPolicy = abrPolicy.toPublicAbrPolicy(),
    )

private fun VesperAbrPolicy.toNativePayload(): NativeAbrPolicyPayload =
    NativeAbrPolicyPayload(
        modeOrdinal =
            when (mode) {
                VesperAbrMode.Auto -> NativeAbrMode.Auto.ordinal
                VesperAbrMode.Constrained -> NativeAbrMode.Constrained.ordinal
                VesperAbrMode.FixedTrack -> NativeAbrMode.FixedTrack.ordinal
            },
        trackId = trackId,
        hasMaxBitRate = maxBitRate != null,
        maxBitRate = maxBitRate ?: 0L,
        hasMaxWidth = maxWidth != null,
        maxWidth = maxWidth ?: 0,
        hasMaxHeight = maxHeight != null,
        maxHeight = maxHeight ?: 0,
    )

private fun VesperTrackPreferencePolicy.toNativePayload(): NativeTrackPreferencePolicy =
    NativeTrackPreferencePolicy(
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectSubtitlesByDefault = selectSubtitlesByDefault,
        selectUndeterminedSubtitleLanguage = selectUndeterminedSubtitleLanguage,
        audioSelection = audioSelection.toNativePayload(),
        subtitleSelection = subtitleSelection.toNativePayload(),
        abrPolicy = abrPolicy.toNativePayload(),
    )

private fun hasTrackBasedPreferenceOverrides(policy: VesperTrackPreferencePolicy): Boolean =
    policy.audioSelection.mode == VesperTrackSelectionMode.Track ||
        policy.subtitleSelection.mode == VesperTrackSelectionMode.Track ||
        policy.abrPolicy.mode == VesperAbrMode.FixedTrack

private fun applyTrackPreferenceDefaults(
    exoPlayer: ExoPlayer,
    policy: VesperTrackPreferencePolicy,
) {
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    applyAudioPreferenceDefaults(builder, policy)
    applySubtitlePreferenceDefaults(builder, policy)
    applyAbrPreferenceDefaults(builder, policy.abrPolicy)
    exoPlayer.setTrackSelectionParameters(builder.build())
}

private fun applyAudioPreferenceDefaults(
    builder: TrackSelectionParameters.Builder,
    policy: VesperTrackPreferencePolicy,
) {
    when (policy.audioSelection.mode) {
        VesperTrackSelectionMode.Disabled -> builder.setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, true)
        VesperTrackSelectionMode.Auto,
        VesperTrackSelectionMode.Track,
        -> builder.setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
    }
    builder.setPreferredAudioLanguage(policy.preferredAudioLanguage)
}

private fun applySubtitlePreferenceDefaults(
    builder: TrackSelectionParameters.Builder,
    policy: VesperTrackPreferencePolicy,
) {
    val shouldEnableText =
        when (policy.subtitleSelection.mode) {
            VesperTrackSelectionMode.Disabled -> false
            VesperTrackSelectionMode.Track -> true
            VesperTrackSelectionMode.Auto ->
                policy.selectSubtitlesByDefault ||
                    policy.selectUndeterminedSubtitleLanguage ||
                    !policy.preferredSubtitleLanguage.isNullOrBlank()
        }

    builder.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, !shouldEnableText)
    builder.setPreferredTextLanguage(policy.preferredSubtitleLanguage)
    builder.setSelectUndeterminedTextLanguage(policy.selectUndeterminedSubtitleLanguage)
    builder.setIgnoredTextSelectionFlags(0)
}

private fun applyAbrPreferenceDefaults(
    builder: TrackSelectionParameters.Builder,
    policy: VesperAbrPolicy,
) {
    builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
    builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, false)
    resetAbrConstraints(builder)

    when (policy.mode) {
        VesperAbrMode.Auto,
        VesperAbrMode.FixedTrack,
        -> Unit
        VesperAbrMode.Constrained -> {
            policy.maxBitRate?.let { builder.setMaxVideoBitrate(it.clampToIntMax()) }
            if (policy.maxWidth != null || policy.maxHeight != null) {
                builder.setMaxVideoSize(
                    policy.maxWidth?.coerceAtLeast(0) ?: Int.MAX_VALUE,
                    policy.maxHeight?.coerceAtLeast(0) ?: Int.MAX_VALUE,
                )
            }
        }
    }
}

private fun applyTrackPreferenceTrackOverrides(
    exoPlayer: ExoPlayer,
    policy: VesperTrackPreferencePolicy,
) {
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    var hasChanges = false

    if (policy.audioSelection.mode == VesperTrackSelectionMode.Track) {
        val trackId = policy.audioSelection.trackId
        val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_AUDIO, it) }
        if (override != null) {
            builder.clearOverridesOfType(C.TRACK_TYPE_AUDIO)
            builder.setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
            builder.setOverrideForType(override)
            hasChanges = true
        } else {
            Log.w(TAG, "failed to apply default audio track preference id=$trackId")
        }
    }

    if (policy.subtitleSelection.mode == VesperTrackSelectionMode.Track) {
        val trackId = policy.subtitleSelection.trackId
        val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_TEXT, it) }
        if (override != null) {
            builder.clearOverridesOfType(C.TRACK_TYPE_TEXT)
            builder.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            builder.setOverrideForType(override)
            hasChanges = true
        } else {
            Log.w(TAG, "failed to apply default subtitle track preference id=$trackId")
        }
    }

    if (policy.abrPolicy.mode == VesperAbrMode.FixedTrack) {
        val trackId = policy.abrPolicy.trackId
        val override =
            trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_VIDEO, it) }
        if (override != null) {
            builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
            builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, false)
            resetAbrConstraints(builder)
            builder.setOverrideForType(override)
            hasChanges = true
        } else {
            Log.w(TAG, "failed to apply default fixed ABR track preference id=$trackId")
        }
    }

    if (hasChanges) {
        exoPlayer.setTrackSelectionParameters(builder.build())
    }
}

private fun applyTrackSelectionCommand(
    exoPlayer: ExoPlayer,
    kind: NativeTrackKind,
    selection: NativeTrackSelectionPayload,
) {
    val trackType = media3TrackType(kind)
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    builder.clearOverridesOfType(trackType)

    when (selection.modeOrdinal) {
        NativeTrackSelectionMode.Auto.ordinal -> {
            builder.setTrackTypeDisabled(trackType, false)
        }
        NativeTrackSelectionMode.Disabled.ordinal -> {
            builder.setTrackTypeDisabled(trackType, true)
        }
        NativeTrackSelectionMode.Track.ordinal -> {
            val trackId = selection.trackId
            val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, trackType, it) }
            if (override == null) {
                Log.w(TAG, "failed to find $kind track for id=${selection.trackId}")
                return
            }
            builder.setTrackTypeDisabled(trackType, false)
            if (kind == NativeTrackKind.Video) {
                resetAbrConstraints(builder)
            }
            builder.setOverrideForType(override)
        }
        else -> return
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
}

private fun applyAbrPolicyCommand(
    exoPlayer: ExoPlayer,
    policy: NativeAbrPolicyPayload,
) {
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
    builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, false)
    resetAbrConstraints(builder)

    when (policy.modeOrdinal) {
        NativeAbrMode.Auto.ordinal -> Unit
        NativeAbrMode.Constrained.ordinal -> {
            if (policy.hasMaxBitRate) {
                builder.setMaxVideoBitrate(policy.maxBitRate.clampToIntMax())
            }
            if (policy.hasMaxWidth || policy.hasMaxHeight) {
                builder.setMaxVideoSize(
                    if (policy.hasMaxWidth) policy.maxWidth.coerceAtLeast(0) else Int.MAX_VALUE,
                    if (policy.hasMaxHeight) policy.maxHeight.coerceAtLeast(0) else Int.MAX_VALUE,
                )
            }
        }
        NativeAbrMode.FixedTrack.ordinal -> {
            val trackId = policy.trackId
            val override =
                trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_VIDEO, it) }
            if (override == null) {
                Log.w(TAG, "failed to find fixed ABR video track for id=${policy.trackId}")
                return
            }
            builder.setOverrideForType(override)
        }
        else -> return
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
}

private fun resetAbrConstraints(builder: TrackSelectionParameters.Builder) {
    builder.setForceLowestBitrate(false)
    builder.setForceHighestSupportedBitrate(false)
    builder.setMaxVideoBitrate(Int.MAX_VALUE)
    builder.setMaxVideoSize(Int.MAX_VALUE, Int.MAX_VALUE)
}

private fun findTrackOverride(
    tracks: Tracks,
    trackType: Int,
    trackId: String,
): TrackSelectionOverride? {
    tracks.groups.forEach { group ->
        if (group.type != trackType) return@forEach
        for (trackIndex in 0 until group.length) {
            val format = group.getTrackFormat(trackIndex)
            if (nativeTrackId(group.mediaTrackGroup, trackIndex, format) == trackId) {
                return TrackSelectionOverride(group.mediaTrackGroup, trackIndex)
            }
        }
    }
    return null
}

private fun media3TrackType(kind: NativeTrackKind): Int =
    when (kind) {
        NativeTrackKind.Video -> C.TRACK_TYPE_VIDEO
        NativeTrackKind.Audio -> C.TRACK_TYPE_AUDIO
        NativeTrackKind.Subtitle -> C.TRACK_TYPE_TEXT
    }

private fun Long.clampToIntMax(): Int =
    coerceAtLeast(0L).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

private fun collectTrackCatalog(tracks: Tracks): NativeTrackCatalog {
    val trackInfos = mutableListOf<NativeTrackInfo>()
    var adaptiveVideo = false
    var adaptiveAudio = false

    tracks.groups.forEach { group ->
        val kind = nativeTrackKind(group.type) ?: return@forEach
        if (kind == NativeTrackKind.Video && group.isAdaptiveSupported) {
            adaptiveVideo = true
        }
        if (kind == NativeTrackKind.Audio && group.isAdaptiveSupported) {
            adaptiveAudio = true
        }

        for (trackIndex in 0 until group.length) {
            if (!group.isTrackSupported(trackIndex, true)) {
                continue
            }
            val format = group.getTrackFormat(trackIndex)
            trackInfos +=
                NativeTrackInfo(
                    id = nativeTrackId(group.mediaTrackGroup, trackIndex, format),
                    kindOrdinal = kind.ordinal,
                    label = format.label,
                    language = format.language?.takeUnless { it.equals("und", ignoreCase = true) },
                    codec = nativeTrackCodec(format),
                    hasBitRate = format.bitrate != Format.NO_VALUE,
                    bitRate = format.bitrate.coerceAtLeast(0).toLong(),
                    hasWidth = format.width != Format.NO_VALUE,
                    width = format.width.coerceAtLeast(0),
                    hasHeight = format.height != Format.NO_VALUE,
                    height = format.height.coerceAtLeast(0),
                    hasFrameRate = format.frameRate != FORMAT_NO_VALUE_FLOAT,
                    frameRate =
                        if (format.frameRate != FORMAT_NO_VALUE_FLOAT) format.frameRate else 0f,
                    hasChannels = format.channelCount != Format.NO_VALUE,
                    channels = format.channelCount.coerceAtLeast(0),
                    hasSampleRate = format.sampleRate != Format.NO_VALUE,
                    sampleRate = format.sampleRate.coerceAtLeast(0),
                    isDefault = (format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0,
                    isForced = (format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0,
                )
        }
    }

    return NativeTrackCatalog(
        tracks = trackInfos.toTypedArray(),
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
    )
}

private fun collectTrackSelection(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeTrackSelectionSnapshotPayload =
    NativeTrackSelectionSnapshotPayload(
        video = collectTrackSelectionForType(C.TRACK_TYPE_VIDEO, tracks, parameters),
        audio = collectTrackSelectionForType(C.TRACK_TYPE_AUDIO, tracks, parameters),
        subtitle = collectTrackSelectionForType(C.TRACK_TYPE_TEXT, tracks, parameters),
        abrPolicy = collectAbrPolicy(tracks, parameters),
    )

private fun collectTrackSelectionForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeTrackSelectionPayload {
    if (parameters.disabledTrackTypes.contains(trackType)) {
        return NativeTrackSelectionPayload(
            modeOrdinal = NativeTrackSelectionMode.Disabled.ordinal,
            trackId = null,
        )
    }

    val override = currentOverrideForType(trackType, tracks, parameters)
    if (override != null) {
        val selectedTrackIndex = override.trackIndices.firstOrNull()
        return if (selectedTrackIndex != null) {
            NativeTrackSelectionPayload(
                modeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                trackId = nativeTrackId(
                    override.mediaTrackGroup,
                    selectedTrackIndex,
                    override.mediaTrackGroup.getFormat(selectedTrackIndex),
                ),
            )
        } else {
            NativeTrackSelectionPayload(
                modeOrdinal = NativeTrackSelectionMode.Disabled.ordinal,
                trackId = null,
            )
        }
    }

    val selectedTrackId = currentSelectedTrackId(trackType, tracks)
    val defaultMode =
        if (trackType == C.TRACK_TYPE_TEXT && selectedTrackId == null) {
            NativeTrackSelectionMode.Disabled
        } else {
            NativeTrackSelectionMode.Auto
        }

    return NativeTrackSelectionPayload(
        modeOrdinal = defaultMode.ordinal,
        trackId = selectedTrackId,
    )
}

private fun collectAbrPolicy(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeAbrPolicyPayload {
    val videoOverride = currentOverrideForType(C.TRACK_TYPE_VIDEO, tracks, parameters)
    if (videoOverride != null) {
        val selectedTrackIndex = videoOverride.trackIndices.firstOrNull()
        return NativeAbrPolicyPayload(
            modeOrdinal = NativeAbrMode.FixedTrack.ordinal,
            trackId = selectedTrackIndex?.let {
                nativeTrackId(
                    videoOverride.mediaTrackGroup,
                    it,
                    videoOverride.mediaTrackGroup.getFormat(it),
                )
            },
            hasMaxBitRate = parameters.maxVideoBitrate != Int.MAX_VALUE,
            maxBitRate = parameters.maxVideoBitrate.coerceAtLeast(0).toLong(),
            hasMaxWidth = parameters.maxVideoWidth != Int.MAX_VALUE,
            maxWidth = parameters.maxVideoWidth.coerceAtLeast(0),
            hasMaxHeight = parameters.maxVideoHeight != Int.MAX_VALUE,
            maxHeight = parameters.maxVideoHeight.coerceAtLeast(0),
        )
    }

    val hasConstraints =
        parameters.forceLowestBitrate ||
            parameters.forceHighestSupportedBitrate ||
            parameters.maxVideoBitrate != Int.MAX_VALUE ||
            parameters.maxVideoWidth != Int.MAX_VALUE ||
            parameters.maxVideoHeight != Int.MAX_VALUE

    return NativeAbrPolicyPayload(
        modeOrdinal = if (hasConstraints) NativeAbrMode.Constrained.ordinal else NativeAbrMode.Auto.ordinal,
        trackId = null,
        hasMaxBitRate = parameters.maxVideoBitrate != Int.MAX_VALUE,
        maxBitRate = parameters.maxVideoBitrate.coerceAtLeast(0).toLong(),
        hasMaxWidth = parameters.maxVideoWidth != Int.MAX_VALUE,
        maxWidth = parameters.maxVideoWidth.coerceAtLeast(0),
        hasMaxHeight = parameters.maxVideoHeight != Int.MAX_VALUE,
        maxHeight = parameters.maxVideoHeight.coerceAtLeast(0),
    )
}

private fun currentOverrideForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): TrackSelectionOverride? =
    parameters.overrides.values.firstOrNull { override ->
        override.type == trackType && currentTracksContainGroup(tracks, override.mediaTrackGroup)
    }

private fun currentSelectedTrackId(trackType: Int, tracks: Tracks): String? {
    tracks.groups.forEach { group ->
        if (group.type != trackType) return@forEach
        for (trackIndex in 0 until group.length) {
            if (group.isTrackSelected(trackIndex)) {
                return nativeTrackId(group.mediaTrackGroup, trackIndex, group.getTrackFormat(trackIndex))
            }
        }
    }
    return null
}

fun resolveVideoVariantObservation(
    currentVideoFormat: Format?,
): VesperVideoVariantObservation? {
    val format = currentVideoFormat ?: return null
    val width = format.width.takeIf { it != Format.NO_VALUE && it > 0 }
    val height = format.height.takeIf { it != Format.NO_VALUE && it > 0 }
    val bitRate = format.bitrate.takeIf { it != Format.NO_VALUE && it > 0 }?.toLong()
    if (width == null && height == null && bitRate == null) {
        return null
    }
    return VesperVideoVariantObservation(
        bitRate = bitRate,
        width = width,
        height = height,
    )
}

fun resolveEffectiveVideoTrackId(
    videoTracks: List<VesperMediaTrack>,
    currentVideoFormat: Format?,
): String? {
    val format = currentVideoFormat ?: return null
    if (videoTracks.isEmpty()) {
        return null
    }

    val currentFormatId = format.id?.takeIf(String::isNotBlank)
    val exactFormatIdMatches =
        currentFormatId?.let { formatId ->
            videoTracks.filter { trackFormatIdComponent(it.id) == formatId }
        }.orEmpty()
    selectBestEffectiveVideoTrackMatch(exactFormatIdMatches, format)?.let { track ->
        return track.id
    }

    val width = format.width.takeIf { it != Format.NO_VALUE && it > 0 }
    val height = format.height.takeIf { it != Format.NO_VALUE && it > 0 }
    val bitRate = format.bitrate.takeIf { it != Format.NO_VALUE && it > 0 }?.toLong()
    val codec = nativeTrackCodec(format)

    if (width != null && height != null && bitRate != null) {
        val exactSizeAndBitRateMatches =
            videoTracks.filter { track ->
                track.width == width &&
                    track.height == height &&
                    track.bitRate == bitRate
            }
        selectBestEffectiveVideoTrackMatch(exactSizeAndBitRateMatches, format)?.let { track ->
            return track.id
        }
    }

    if (width != null && height != null && codec != null) {
        val exactSizeAndCodecMatches =
            videoTracks.filter { track ->
                track.width == width &&
                    track.height == height &&
                    track.codec == codec
            }
        selectBestEffectiveVideoTrackMatch(exactSizeAndCodecMatches, format)?.let { track ->
            return track.id
        }
    }

    if (bitRate != null && codec != null) {
        val exactBitRateAndCodecMatches =
            videoTracks.filter { track ->
                track.bitRate == bitRate &&
                    track.codec == codec
            }
        selectBestEffectiveVideoTrackMatch(exactBitRateAndCodecMatches, format)?.let { track ->
            return track.id
        }
    }

    return null
}

private fun selectBestEffectiveVideoTrackMatch(
    candidates: List<VesperMediaTrack>,
    currentVideoFormat: Format,
): VesperMediaTrack? {
    if (candidates.isEmpty()) {
        return null
    }
    if (candidates.size == 1) {
        return candidates.first()
    }

    return candidates.minWithOrNull(
        compareBy<VesperMediaTrack> { track ->
            effectiveVideoTrackDistance(track.width, currentVideoFormat.width)
        }.thenBy { track ->
            effectiveVideoTrackDistance(track.height, currentVideoFormat.height)
        }.thenBy { track ->
            effectiveVideoTrackDistance(track.bitRate, currentVideoFormat.bitrate)
        }.thenBy { track ->
            effectiveVideoFrameRateDistance(track.frameRate, currentVideoFormat.frameRate)
        }.thenByDescending { track ->
            if (track.codec == nativeTrackCodec(currentVideoFormat)) 1 else 0
        }.thenBy { track ->
            track.id
        },
    )
}

private fun effectiveVideoTrackDistance(trackValue: Int?, formatValue: Int): Long {
    if (formatValue == Format.NO_VALUE || formatValue <= 0) {
        return 0
    }
    val candidate = trackValue ?: return Long.MAX_VALUE / 4
    return kotlin.math.abs(candidate.toLong() - formatValue.toLong())
}

private fun effectiveVideoTrackDistance(trackValue: Long?, formatValue: Int): Long {
    if (formatValue == Format.NO_VALUE || formatValue <= 0) {
        return 0
    }
    val candidate = trackValue ?: return Long.MAX_VALUE / 4
    return kotlin.math.abs(candidate - formatValue.toLong())
}

private fun effectiveVideoFrameRateDistance(trackValue: Float?, formatValue: Float): Long {
    if (formatValue == FORMAT_NO_VALUE_FLOAT || !formatValue.isFinite() || formatValue <= 0f) {
        return 0
    }
    val candidate = trackValue ?: return Long.MAX_VALUE / 4
    return kotlin.math.abs(((candidate - formatValue) * 100).toLong())
}

private fun trackFormatIdComponent(trackId: String): String? {
    val lastSeparatorIndex = trackId.lastIndexOf(':')
    if (lastSeparatorIndex <= 0) {
        return null
    }
    val secondLastSeparatorIndex = trackId.lastIndexOf(':', lastSeparatorIndex - 1)
    if (secondLastSeparatorIndex < 0 || secondLastSeparatorIndex + 1 >= lastSeparatorIndex) {
        return null
    }
    return trackId.substring(secondLastSeparatorIndex + 1, lastSeparatorIndex)
}

private fun currentTracksContainGroup(tracks: Tracks, trackGroup: TrackGroup): Boolean =
    tracks.groups.any { group -> group.mediaTrackGroup == trackGroup }

private fun nativeTrackKind(trackType: Int): NativeTrackKind? =
    when (trackType) {
        C.TRACK_TYPE_VIDEO -> NativeTrackKind.Video
        C.TRACK_TYPE_AUDIO -> NativeTrackKind.Audio
        C.TRACK_TYPE_TEXT -> NativeTrackKind.Subtitle
        else -> null
    }

private fun nativeTrackId(trackGroup: TrackGroup, trackIndex: Int, format: Format): String {
    val groupId =
        trackGroup.id.takeIf { it.isNotBlank() }
            ?: "type${trackGroup.type}"
    val formatId = format.id?.takeIf { it.isNotBlank() } ?: "track$trackIndex"
    return "$groupId:$formatId:$trackIndex"
}

private fun nativeTrackCodec(format: Format): String? =
    format.codecs ?: format.sampleMimeType ?: format.containerMimeType

private fun buildMediaItem(source: VesperPlayerSource): MediaItem {
    val builder = MediaItem.Builder()
        .setUri(source.uri)

    when (source.protocol) {
        VesperPlayerSourceProtocol.Hls -> builder.setMimeType(MimeTypes.APPLICATION_M3U8)
        VesperPlayerSourceProtocol.Dash -> builder.setMimeType(MimeTypes.APPLICATION_MPD)
        else -> Unit
    }

    return builder.build()
}

private fun buildDataSourceFactory(
    appContext: Context,
    cachePolicy: NativeCachePolicy,
): androidx.media3.datasource.DataSource.Factory {
    val upstreamFactory = DefaultDataSource.Factory(appContext)
    val resolvedCachePolicy = resolveCachePolicy(cachePolicy)
    if (!resolvedCachePolicy.enabled) {
        return upstreamFactory
    }

    val cache =
        VesperMediaCacheStore.cache(
            appContext = appContext,
            maxDiskBytes = resolvedCachePolicy.maxDiskBytes,
        )

    return CacheDataSource.Factory()
        .setCache(cache)
        .setUpstreamDataSourceFactory(upstreamFactory)
        .setFlags(CacheDataSource.FLAG_IGNORE_CACHE_ON_ERROR)
}

private fun resolveCachePolicy(
    cachePolicy: NativeCachePolicy,
): ResolvedCachePolicy {
    val maxDiskBytes = cachePolicy.maxDiskBytes.takeIf { cachePolicy.hasMaxDiskBytes } ?: 0L
    return ResolvedCachePolicy(enabled = maxDiskBytes > 0L, maxDiskBytes = maxDiskBytes)
}

private fun resolveResiliencePolicy(
    source: VesperPlayerSource,
    resiliencePolicy: VesperPlaybackResiliencePolicy,
): NativeResolvedResiliencePolicy =
    VesperNativeJni.resolveResiliencePolicy(
        sourceKindOrdinal = source.kind.ordinal,
        sourceProtocolOrdinal = source.protocol.ordinal,
        bufferingPolicy = resiliencePolicy.buffering.toNativePayload(),
        retryPolicy = resiliencePolicy.retry.toNativePayload(),
        cachePolicy = resiliencePolicy.cache.toNativePayload(),
    )

private fun resolveTrackPreferences(
    trackPreferencePolicy: VesperTrackPreferencePolicy,
): VesperTrackPreferencePolicy =
    VesperNativeJni.resolveTrackPreferences(trackPreferencePolicy.toNativePayload())
        .toPublicTrackPreferencePolicy()

private fun Long.normalizedOptionalMs(): Long? =
    if (this == C.TIME_UNSET || this < 0L) {
        null
    } else {
        this
    }

private fun Long.normalizedDurationMs(): Long =
    if (this == C.TIME_UNSET || this < 0L) {
        -1L
    } else {
        this
    }

private data class NativePlaybackError(
    val codeOrdinal: Int,
    val categoryOrdinal: Int,
    val retriable: Boolean,
)

private data class ResolvedCachePolicy(
    val enabled: Boolean,
    val maxDiskBytes: Long,
)

private object VesperMediaCacheStore {
    private val caches = mutableMapOf<Long, SimpleCache>()
    private val databaseProviders = mutableMapOf<Long, StandaloneDatabaseProvider>()

    @Synchronized
    fun cache(
        appContext: Context,
        maxDiskBytes: Long,
    ): SimpleCache {
        return caches.getOrPut(maxDiskBytes) {
            val cacheDir =
                File(appContext.cacheDir, "vesper-media-cache/$maxDiskBytes").apply { mkdirs() }
            val databaseProvider =
                databaseProviders.getOrPut(maxDiskBytes) { StandaloneDatabaseProvider(appContext) }
            SimpleCache(
                cacheDir,
                LeastRecentlyUsedCacheEvictor(maxDiskBytes),
                databaseProvider,
            )
        }
    }
}

private fun classifyPlaybackException(error: PlaybackException): NativePlaybackError =
    when (error.errorCode) {
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED,
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
        PlaybackException.ERROR_CODE_IO_INVALID_HTTP_CONTENT_TYPE,
        PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS,
        -> NativePlaybackError(
            codeOrdinal = BACKEND_FAILURE_ORDINAL,
            categoryOrdinal = NETWORK_CATEGORY_ORDINAL,
            retriable = true,
        )

        PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND,
        PlaybackException.ERROR_CODE_IO_READ_POSITION_OUT_OF_RANGE,
        -> NativePlaybackError(
            codeOrdinal = INVALID_SOURCE_ORDINAL,
            categoryOrdinal = SOURCE_CATEGORY_ORDINAL,
            retriable = false,
        )

        PlaybackException.ERROR_CODE_IO_NO_PERMISSION,
        PlaybackException.ERROR_CODE_IO_CLEARTEXT_NOT_PERMITTED,
        PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED,
        -> NativePlaybackError(
            codeOrdinal = UNSUPPORTED_ORDINAL,
            categoryOrdinal = CAPABILITY_CATEGORY_ORDINAL,
            retriable = false,
        )

        PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_MALFORMED,
        -> NativePlaybackError(
            codeOrdinal = INVALID_SOURCE_ORDINAL,
            categoryOrdinal = SOURCE_CATEGORY_ORDINAL,
            retriable = false,
        )

        PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
        PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED,
        PlaybackException.ERROR_CODE_DECODING_FAILED,
        -> NativePlaybackError(
            codeOrdinal = DECODE_FAILURE_ORDINAL,
            categoryOrdinal = DECODE_CATEGORY_ORDINAL,
            retriable = false,
        )

        PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_WRITE_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_WRITE_FAILED,
        -> NativePlaybackError(
            codeOrdinal = AUDIO_OUTPUT_UNAVAILABLE_ORDINAL,
            categoryOrdinal = AUDIO_OUTPUT_CATEGORY_ORDINAL,
            retriable = false,
        )

        else ->
            NativePlaybackError(
                codeOrdinal = BACKEND_FAILURE_ORDINAL,
                categoryOrdinal = PLATFORM_CATEGORY_ORDINAL,
                retriable = false,
            )
    }

private const val INVALID_SOURCE_ORDINAL = 2
private const val BACKEND_FAILURE_ORDINAL = 3
private const val AUDIO_OUTPUT_UNAVAILABLE_ORDINAL = 4
private const val DECODE_FAILURE_ORDINAL = 5
private const val UNSUPPORTED_ORDINAL = 7
private const val SOURCE_CATEGORY_ORDINAL = 1
private const val NETWORK_CATEGORY_ORDINAL = 2
private const val DECODE_CATEGORY_ORDINAL = 3
private const val AUDIO_OUTPUT_CATEGORY_ORDINAL = 4
private const val CAPABILITY_CATEGORY_ORDINAL = 6
private const val PLATFORM_CATEGORY_ORDINAL = 7
private const val TAG = "VesperPlayerAndroidHost"
private val FORMAT_NO_VALUE_FLOAT = Format.NO_VALUE.toFloat()

private fun exoPlaybackStateName(playbackState: Int): String =
    when (playbackState) {
        Player.STATE_IDLE -> "IDLE"
        Player.STATE_BUFFERING -> "BUFFERING"
        Player.STATE_READY -> "READY"
        Player.STATE_ENDED -> "ENDED"
        else -> "UNKNOWN($playbackState)"
    }

private fun inferSourceKind(uri: String): VesperPlayerSourceKind =
    if (
        uri.startsWith("file://", ignoreCase = true) ||
            uri.startsWith("content://", ignoreCase = true) ||
            uri.startsWith("/") ||
            (!uri.contains("://") && !uri.startsWith("content:", ignoreCase = true))
    ) {
        VesperPlayerSourceKind.Local
    } else {
        VesperPlayerSourceKind.Remote
    }

private fun inferSourceProtocol(uri: String): VesperPlayerSourceProtocol {
    val normalized = uri.lowercase()
    val normalizedPath = normalized.substringBefore('#').substringBefore('?')
    return when {
        normalized.startsWith("file://") || uri.startsWith("/") -> VesperPlayerSourceProtocol.File
        normalized.startsWith("content://") -> VesperPlayerSourceProtocol.Content
        normalizedPath.endsWith(".m3u8") -> VesperPlayerSourceProtocol.Hls
        normalizedPath.endsWith(".mpd") -> VesperPlayerSourceProtocol.Dash
        normalized.startsWith("http://") || normalized.startsWith("https://") -> VesperPlayerSourceProtocol.Progressive
        else -> VesperPlayerSourceProtocol.Unknown
    }
}

private const val DEFAULT_PRELOAD_WARMUP_READ_BYTES = 32 * 1024
