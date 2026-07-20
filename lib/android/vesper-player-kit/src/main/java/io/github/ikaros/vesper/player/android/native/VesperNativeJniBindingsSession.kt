package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import android.view.ViewGroup
import androidx.media3.common.C
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
import androidx.media3.common.text.CueGroup
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
import androidx.media3.exoplayer.drm.KeyRequestInfo
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistTracker
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.LoadEventInfo
import androidx.media3.exoplayer.source.MediaLoadData
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo
import java.io.File
import java.io.IOException
import java.net.URI
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.absoluteValue
import kotlin.math.pow
import kotlin.math.roundToLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject

private const val SOURCE_NORMALIZER_DISPOSE_QUEUE_CAPACITY = 64
private const val SOURCE_NORMALIZER_DISPOSE_MAX_THREADS = 4
private const val PRELOAD_WARMUP_QUEUE_CAPACITY = 64
private const val PRELOAD_WARMUP_MAX_THREADS = 2

private object VesperSourceNormalizerResourceDisposer {
    private val threadIndex = AtomicInteger(0)
    private val executor =
        ThreadPoolExecutor(
            1,
            SOURCE_NORMALIZER_DISPOSE_MAX_THREADS,
            15L,
            TimeUnit.SECONDS,
            ArrayBlockingQueue(SOURCE_NORMALIZER_DISPOSE_QUEUE_CAPACITY),
            { runnable ->
                Thread(
                    runnable,
                    "vesper-source-normalizer-close-${threadIndex.incrementAndGet()}",
                ).apply {
                    isDaemon = true
                }
            },
            ThreadPoolExecutor.AbortPolicy(),
        )

    fun disposeAsync(handle: Long) {
        if (handle == 0L) {
            return
        }
        val task = Runnable {
            disposeSourceNormalizerResourceHandle(handle, "source normalizer resource session")
        }
        try {
            executor.execute(task)
        } catch (error: RejectedExecutionException) {
            disposeAfterRejectedExecution(task, error)
        }
    }

    private fun disposeAfterRejectedExecution(
        task: Runnable,
        error: RejectedExecutionException,
    ) {
        Log.w(
            NATIVE_JNI_BINDINGS_TAG,
            "source normalizer dispose queue saturated; running fallback close path",
            error,
        )
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Log.w(
                NATIVE_JNI_BINDINGS_TAG,
                "source normalizer fallback close is running on the main thread after bounded queues were exhausted",
            )
        }
        task.run()
    }
}

private object VesperPreloadWarmupDispatcher {
    private val threadIndex = AtomicInteger(0)
    private val executor =
        ThreadPoolExecutor(
            1,
            PRELOAD_WARMUP_MAX_THREADS,
            15L,
            TimeUnit.SECONDS,
            ArrayBlockingQueue(PRELOAD_WARMUP_QUEUE_CAPACITY),
            { runnable ->
                Thread(
                    runnable,
                    "vesper-preload-warmup-${threadIndex.incrementAndGet()}",
                ).apply {
                    isDaemon = true
                }
            },
            ThreadPoolExecutor.AbortPolicy(),
        )

    fun execute(task: Runnable): Boolean =
        try {
            executor.execute(task)
            true
        } catch (_: RejectedExecutionException) {
            false
        }
}

internal fun VesperNativeJniBindings.dispatchRustCommand(action: (Long) -> Unit) {
    // Read sessionHandle first so that if dispose() has already nulled it we
    // bail out before checking isDisposed and avoid passing a stale handle.
    val handle = sessionHandle ?: return
    if (isDisposed.get()) {
        return
    }
    action(handle)
    drainAndApplyNativeCommands()
    pushSnapshotToRust()
    pushTrackStateToRust()
    notifyNativeUpdate()
}

internal fun VesperNativeJniBindings.drainAndApplyNativeCommands() {
    if (isDisposed.get()) {
        return
    }
    val handle = sessionHandle ?: return
    val exoPlayer = player ?: return

    VesperNativeJni.drainNativeCommands(handle).forEach { command ->
        when (command) {
            NativePlayerCommand.Play -> {
                Log.d(NATIVE_JNI_BINDINGS_TAG, "apply native command: Play")
                exoPlayer.play()
            }
            NativePlayerCommand.Pause -> {
                Log.d(NATIVE_JNI_BINDINGS_TAG, "apply native command: Pause")
                exoPlayer.pause()
            }
            is NativePlayerCommand.SeekTo -> {
                val windowPositionMs =
                    exoPlayer.windowPositionForTimelinePosition(command.positionMs)
                Log.d(
                    NATIVE_JNI_BINDINGS_TAG,
                    "apply native command: SeekTo timelinePositionMs=${command.positionMs} windowPositionMs=$windowPositionMs",
                )
                exoPlayer.seekTo(windowPositionMs)
            }
            NativePlayerCommand.Stop -> {
                Log.d(NATIVE_JNI_BINDINGS_TAG, "apply native command: Stop")
                exoPlayer.pause()
                exoPlayer.seekTo(0L)
            }
            is NativePlayerCommand.SetPlaybackRate -> {
                Log.d(NATIVE_JNI_BINDINGS_TAG, "apply native command: SetPlaybackRate rate=${command.rate}")
                exoPlayer.setPlaybackParameters(PlaybackParameters(command.rate))
            }
            is NativePlayerCommand.SetVideoTrackSelection -> {
                Log.d(
                    NATIVE_JNI_BINDINGS_TAG,
                    "apply native command: SetVideoTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                )
                if (nativeFramePipelineOwnsSurface) {
                    return@forEach
                }
                applyTrackSelectionCommand(
                    exoPlayer = exoPlayer,
                    kind = NativeTrackKind.Video,
                    selection = command.selection,
                    onTrackSelectionFailure = trackSelectionFailureListener?.let { cb -> { f -> cb(f) } },
                )
            }
            is NativePlayerCommand.SetAudioTrackSelection -> {
                Log.d(
                    NATIVE_JNI_BINDINGS_TAG,
                    "apply native command: SetAudioTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                )
                applyTrackSelectionCommand(
                    exoPlayer = exoPlayer,
                    kind = NativeTrackKind.Audio,
                    selection = command.selection,
                    onTrackSelectionFailure = trackSelectionFailureListener?.let { cb -> { f -> cb(f) } },
                )
            }
            is NativePlayerCommand.SetSubtitleTrackSelection -> {
                Log.d(
                    NATIVE_JNI_BINDINGS_TAG,
                    "apply native command: SetSubtitleTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                )
                applyTrackSelectionCommand(
                    exoPlayer = exoPlayer,
                    kind = NativeTrackKind.Subtitle,
                    selection = command.selection,
                    onTrackSelectionFailure = trackSelectionFailureListener?.let { cb -> { f -> cb(f) } },
                    sourceProtocol = currentSourceProtocol,
                )
            }
            is NativePlayerCommand.SetAbrPolicy -> {
                Log.d(
                    NATIVE_JNI_BINDINGS_TAG,
                    "apply native command: SetAbrPolicy mode=${command.policy.modeOrdinal} trackId=${command.policy.trackId}",
                )
                if (nativeFramePipelineOwnsSurface) {
                    return@forEach
                }
                applyAbrPolicyCommand(exoPlayer, command.policy)
            }
        }
    }
}

internal fun VesperNativeJniBindings.buildPlayerListener(
    trackPreferencePolicy: VesperTrackPreferencePolicy,
): Player.Listener =
    object : Player.Listener {
        private var pendingTrackOverrides =
            trackPreferencePolicy.takeIf(::hasTrackBasedPreferenceOverrides)

        override fun onPlaybackStateChanged(playbackState: Int) {
            Log.d(
                NATIVE_JNI_BINDINGS_TAG,
                "onPlaybackStateChanged state=${exoPlaybackStateName(playbackState)} playWhenReady=${player?.playWhenReady}",
            )
            recordBenchmark(
                "playback_state_changed",
                mapOf("state" to exoPlaybackStateName(playbackState)),
            )
            pushSnapshotToRust()
            notifyNativeUpdate()
        }

        override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
            Log.d(NATIVE_JNI_BINDINGS_TAG, "onPlayWhenReadyChanged playWhenReady=$playWhenReady reason=$reason")
            recordBenchmark(
                "play_when_ready_changed",
                mapOf(
                    "playWhenReady" to playWhenReady.toString(),
                    "reason" to reason.toString(),
                ),
            )
            pushSnapshotToRust()
            notifyNativeUpdate()
        }

        override fun onPlaybackParametersChanged(playbackParameters: PlaybackParameters) {
            Log.d(NATIVE_JNI_BINDINGS_TAG, "onPlaybackParametersChanged speed=${playbackParameters.speed}")
            recordBenchmark(
                "playback_parameters_changed",
                mapOf("speed" to playbackParameters.speed.toString()),
            )
            pushSnapshotToRust()
            pushTrackStateToRust()
            notifyNativeUpdate()
        }

        override fun onCues(cueGroup: CueGroup) {
            subtitleCuesListener?.invoke(cueGroup.cues)
        }

        override fun onTracksChanged(tracks: Tracks) {
            Log.d(NATIVE_JNI_BINDINGS_TAG, "onTracksChanged groups=${tracks.groups.size}")
            recordBenchmark("tracks_changed", mapOf("groups" to tracks.groups.size.toString()))
            hasObservedTrackCatalog = true
            player?.let { exoPlayer ->
                pendingTrackOverrides
                    ?.takeIf { !nativeFramePipelineOwnsSurface }
                    ?.let { defaults ->
                    applyTrackPreferenceTrackOverrides(exoPlayer, defaults)
                    pendingTrackOverrides = null
                }
            }
            pushTrackStateToRust()
            notifyNativeUpdate()
        }

        override fun onTrackSelectionParametersChanged(parameters: TrackSelectionParameters) {
            Log.d(NATIVE_JNI_BINDINGS_TAG, "onTrackSelectionParametersChanged overrides=${parameters.overrides.size}")
            recordBenchmark(
                "track_selection_parameters_changed",
                mapOf("overrides" to parameters.overrides.size.toString()),
            )
            pushTrackStateToRust()
            notifyNativeUpdate()
        }

        override fun onVideoSizeChanged(videoSize: VideoSize) {
            Log.d(
                NATIVE_JNI_BINDINGS_TAG,
                "onVideoSizeChanged width=${videoSize.width} height=${videoSize.height} pixelRatio=${videoSize.pixelWidthHeightRatio}",
            )
            val layoutInfo = videoSize.toNativeVideoLayoutInfo()
            if (layoutInfo == null) {
                Log.d(NATIVE_JNI_BINDINGS_TAG, "ignoring transient empty video size during renderer switch")
                return
            }
            recordBenchmark(
                "video_size_changed",
                mapOf(
                    "width" to videoSize.width.toString(),
                    "height" to videoSize.height.toString(),
                ),
            )
            currentVideoLayoutState = layoutInfo
            notifyNativeUpdate()
        }

        override fun onPositionDiscontinuity(
            oldPosition: Player.PositionInfo,
            newPosition: Player.PositionInfo,
            reason: Int,
        ) {
            if (reason == Player.DISCONTINUITY_REASON_SEEK) {
                sessionHandle?.let { handle ->
                    val completedPositionMs =
                        player?.timelinePositionForWindowPosition(newPosition.positionMs)
                            ?: newPosition.positionMs
                    recordBenchmark(
                        "seek_completed",
                        mapOf("positionMs" to completedPositionMs.toString()),
                    )
                    VesperNativeJni.reportSeekCompleted(handle, completedPositionMs)
                }
            }
            Log.d(
                NATIVE_JNI_BINDINGS_TAG,
                "onPositionDiscontinuity reason=$reason positionMs=${newPosition.positionMs}",
            )
            pushSnapshotToRust()
            notifyNativeUpdate()
        }

        override fun onPlayerError(error: PlaybackException) {
            Log.e(NATIVE_JNI_BINDINGS_TAG, "onPlayerError ${error.errorCodeName}: ${error.message}", error)
            recordBenchmark(
                "playback_error",
                mapOf(
                    "code" to error.errorCodeName,
                    "message" to (error.message ?: ""),
                ),
            )
            val classified = classifyPlaybackException(error)
            enqueueHdrFailureHintIfNeeded(error, classified)
            sessionHandle?.let { handle ->
                VesperNativeJni.reportError(
                    handle,
                    classified.codeOrdinal,
                    classified.categoryOrdinal,
                    classified.retriable,
                    error.message ?: error.errorCodeName,
                )
            }
            enqueueTerminalPlaybackError(
                message = error.message ?: error.errorCodeName,
                classified = classified,
                reason = error.terminalPlaybackErrorReason(),
                errorCodeName = error.errorCodeName,
                error = error,
            )
            pushSnapshotToRust()
            notifyNativeUpdate()
        }
    }

internal fun VesperNativeJniBindings.buildAnalyticsListener(): AnalyticsListener =
    object : AnalyticsListener {
        override fun onVideoDecoderInitialized(
            eventTime: AnalyticsListener.EventTime,
            decoderName: String,
            initializedTimestampMs: Long,
            initializationDurationMs: Long,
        ) {
            currentVideoDecoderName = decoderName
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "onVideoDecoderInitialized decoder=$decoderName durationMs=$initializationDurationMs",
            )
            recordBenchmark(
                "video_decoder_initialized",
                mapOf(
                    "decoderName" to decoderName,
                    "initializationDurationMs" to initializationDurationMs.toString(),
                    "selectionReason" to "hardware_decode_required",
                ),
            )
        }

        override fun onVideoInputFormatChanged(
            eventTime: AnalyticsListener.EventTime,
            format: Format,
            decoderReuseEvaluation: DecoderReuseEvaluation?,
        ) {
            val codec = nativeTrackCodec(format) ?: ""
            val mimeType = videoMimeType(format)
            val decoderDiagnostics = VesperHardwareMediaCodecSelector.decoderDiagnostics(mimeType)
            val hardwareDecodeSupported =
                decoderDiagnostics["hardwareDecoderCount"]?.toIntOrNull()?.let { it > 0 }
                    ?: VesperHardwareMediaCodecSelector.hasHardwareDecoder(mimeType)
            Log.d(
                NATIVE_JNI_BINDINGS_TAG,
                "onVideoInputFormatChanged formatId=${format.id} sampleMimeType=${format.sampleMimeType} codecs=${format.codecs} " +
                    "bitrate=${format.bitrate} width=${format.width} height=${format.height} " +
                    "hardwareDecoders=${decoderDiagnostics["hardwareDecoderCount"] ?: "unknown"} " +
                    "secureHardwareDecoders=${decoderDiagnostics["secureHardwareDecoderCount"] ?: "unknown"} " +
                    "decoderName=${currentVideoDecoderName ?: "pending"}",
            )
            recordBenchmark(
                "video_input_format_changed",
                mapOf(
                    "formatId" to (format.id ?: ""),
                    "sampleMimeType" to (format.sampleMimeType ?: ""),
                    "codecs" to (format.codecs ?: ""),
                    "codecFamily" to vesperAndroidVideoCodecFamily(codec).toBenchmarkValue(),
                    "hardwareDecodeSupported" to hardwareDecodeSupported.toString(),
                    "selectionReason" to "hardware_decode_source_selection",
                    "bitrate" to format.bitrate.toString(),
                    "width" to format.width.toString(),
                    "height" to format.height.toString(),
                ) + decoderDiagnostics + (currentVideoDecoderName?.let { mapOf("decoderName" to it) } ?: emptyMap()),
            )
            currentRuntimeHdrEvidence = format.androidRuntimeHdrEvidence()
            currentRuntimeSessionProbe = buildRuntimeSessionProbeSnapshot(format)
            enqueueHdrCapabilityWarningIfNeeded(format)
            pushTrackStateToRust()
            notifyNativeUpdate()
        }

        override fun onRenderedFirstFrame(
            eventTime: AnalyticsListener.EventTime,
            output: Any,
            renderTimeMs: Long,
        ) {
            firstFrameRenderedForCurrentSource = true
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "onRenderedFirstFrame renderTimeMs=$renderTimeMs output=${output::class.java.name}",
            )
            val firstFrameMark = firstFrameGate.markFirstFrameRendered()
            if (!firstFrameMark.isFirstForEpoch) {
                return
            }
            recordBenchmark(
                "first_frame_rendered",
                mapOf(
                    "renderTimeMs" to renderTimeMs.toString(),
                    "isFirstForEpoch" to firstFrameMark.isFirstForEpoch.toString(),
                ),
            )
        }

        @Suppress("DEPRECATION")
        override fun onDrmSessionAcquired(
            eventTime: AnalyticsListener.EventTime,
            state: Int,
        ) {
            val source = currentDrmDiagnosticsSource ?: return
            val drm = source.drmConfiguration ?: return
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "onDrmSessionAcquired keySystem=${drm.keySystem} state=$state source=${source.uri}",
            )
            recordBenchmark(
                "drm_session_acquired",
                mapOf(
                    "keySystem" to drm.keySystem,
                    "state" to state.toString(),
                    "licenseUriHost" to drm.licenseUri.hostForDiagnostics(),
                ),
            )
        }

        override fun onDrmKeysLoaded(
            eventTime: AnalyticsListener.EventTime,
            keyRequestInfo: KeyRequestInfo,
        ) {
            val source = currentDrmDiagnosticsSource ?: return
            val drm = source.drmConfiguration ?: return
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "onDrmKeysLoaded keySystem=${drm.keySystem} source=${source.uri}",
            )
            recordBenchmark(
                "drm_keys_loaded",
                mapOf(
                    "keySystem" to drm.keySystem,
                    "licenseUriHost" to drm.licenseUri.hostForDiagnostics(),
                ),
            )
        }

        override fun onDrmSessionManagerError(
            eventTime: AnalyticsListener.EventTime,
            error: Exception,
        ) {
            val source = currentDrmDiagnosticsSource
            val drm = source?.drmConfiguration
            currentDrmRuntimeErrorCount += 1
            val maxAttempts = currentRetryMaxAttempts
            val attemptsExhausted = maxAttempts != null && currentDrmRuntimeErrorCount >= maxAttempts
            val maxAttemptsLabel = maxAttempts?.toString() ?: "unlimited"
            Log.e(
                NATIVE_JNI_BINDINGS_TAG,
                "onDrmSessionManagerError keySystem=${drm?.keySystem ?: "none"} source=${source?.uri ?: ""} " +
                    "attempt=$currentDrmRuntimeErrorCount/$maxAttemptsLabel exhausted=$attemptsExhausted " +
                    "error=${error::class.java.name}: ${error.message}",
                error,
            )
            recordBenchmark(
                "drm_session_manager_error",
                mapOf(
                    "keySystem" to (drm?.keySystem ?: ""),
                    "licenseUriHost" to (drm?.licenseUri?.hostForDiagnostics() ?: ""),
                    "attempt" to currentDrmRuntimeErrorCount.toString(),
                    "maxAttempts" to maxAttemptsLabel,
                    "attemptsExhausted" to attemptsExhausted.toString(),
                    "errorClass" to error::class.java.name,
                    "errorMessage" to (error.message ?: ""),
                ),
            )
            addLocalBridgeEvent(
                NativeBridgeEvent.Warning(
                    VesperRuntimeWarning(
                        domain = "drm",
                        payload =
                            linkedMapOf(
                                "reason" to "drmSessionManagerError",
                                "keySystem" to (drm?.keySystem ?: ""),
                                "licenseUriHost" to (drm?.licenseUri?.hostForDiagnostics() ?: ""),
                                "sourceUri" to (source?.uri ?: ""),
                                "attempt" to currentDrmRuntimeErrorCount.toString(),
                                "maxAttempts" to maxAttemptsLabel,
                                "attemptsExhausted" to attemptsExhausted.toString(),
                                "errorClass" to error::class.java.name,
                                "errorMessage" to (error.message ?: ""),
                            ),
                    ),
                )
            )
            if (!attemptsExhausted) {
                notifyNativeUpdate()
                return
            }
            enqueueTerminalPlaybackError(
                message = error.message ?: "Widevine DRM license/provisioning failed.",
                classified =
                    NativePlaybackError(
                        codeOrdinal = BACKEND_FAILURE_ORDINAL,
                        categoryOrdinal = NETWORK_CATEGORY_ORDINAL,
                        retriable = true,
                    ),
                reason = "drmSessionManagerError",
                errorCodeName = null,
                error = error,
                extraDetails =
                    mapOf(
                        "attempt" to currentDrmRuntimeErrorCount,
                        "attemptsExhausted" to true,
                    ),
            )
            notifyNativeUpdate()
        }

        override fun onDrmSessionReleased(eventTime: AnalyticsListener.EventTime) {
            val source = currentDrmDiagnosticsSource ?: return
            val drm = source.drmConfiguration ?: return
            Log.i(
                NATIVE_JNI_BINDINGS_TAG,
                "onDrmSessionReleased keySystem=${drm.keySystem} source=${source.uri}",
            )
            recordBenchmark(
                "drm_session_released",
                mapOf("keySystem" to drm.keySystem),
            )
        }

        override fun onLoadError(
            eventTime: AnalyticsListener.EventTime,
            loadEventInfo: LoadEventInfo,
            mediaLoadData: MediaLoadData,
            error: IOException,
            wasCanceled: Boolean,
        ) {
            val source = currentDrmDiagnosticsSource
            val drm = source?.drmConfiguration
            Log.w(
                NATIVE_JNI_BINDINGS_TAG,
                "onLoadError dataType=${mediaLoadData.dataType.media3DataTypeName()} " +
                    "trackType=${mediaLoadData.trackType.media3TrackTypeName()} canceled=$wasCanceled " +
                    "uri=${loadEventInfo.uri} bytesLoaded=${loadEventInfo.bytesLoaded} " +
                    "error=${error::class.java.name}: ${error.message}",
                error,
            )
            recordBenchmark(
                "load_error",
                mapOf(
                    "dataType" to mediaLoadData.dataType.media3DataTypeName(),
                    "trackType" to mediaLoadData.trackType.media3TrackTypeName(),
                    "wasCanceled" to wasCanceled.toString(),
                    "keySystem" to (drm?.keySystem ?: ""),
                    "uriHost" to loadEventInfo.uri.toString().hostForDiagnostics(),
                    "errorClass" to error::class.java.name,
                    "errorMessage" to (error.message ?: ""),
                ),
            )
        }
    }

internal fun VesperNativeJniBindings.scheduleFirstFrameWatchdog(
    source: VesperPlayerSource,
    playbackEpoch: Long,
    route: FirstFrameWatchdogRoute,
) {
    cancelFirstFrameWatchdog()
    if (!route.enabled) {
        return
    }
    firstFrameWatchdogSource = source
    val runnable =
        Runnable {
            val exoPlayer = player ?: return@Runnable
            if (isDisposed.get() || firstFrameWatchdogSource != source || firstFrameGate.currentEpoch != playbackEpoch) {
                return@Runnable
            }
            if (firstFrameRenderedForCurrentSource || exoPlayer.playbackState != Player.STATE_BUFFERING) {
                return@Runnable
            }
            val videoFormat = exoPlayer.videoFormat
            val mimeType = videoFormat?.let(::videoMimeType)
            val decoderDiagnostics = VesperHardwareMediaCodecSelector.decoderDiagnostics(mimeType)
            val drm = source.drmConfiguration
            Log.w(
                NATIVE_JNI_BINDINGS_TAG,
                "firstFrameWatchdog state=${exoPlaybackStateName(exoPlayer.playbackState)} " +
                    "positionMs=${exoPlayer.currentPosition} durationMs=${exoPlayer.duration.normalizedDurationMs()} " +
                    "keySystem=${drm?.keySystem ?: "none"} surfaceAttached=${attachedSurface?.isValid == true} " +
                    "formatId=${videoFormat?.id ?: ""} sampleMimeType=${videoFormat?.sampleMimeType ?: ""} " +
                    "codecs=${videoFormat?.codecs ?: ""} decoderName=${currentVideoDecoderName ?: "pending"} " +
                    "hardwareDecoders=${decoderDiagnostics["hardwareDecoderCount"] ?: "unknown"} " +
                    "secureHardwareDecoders=${decoderDiagnostics["secureHardwareDecoderCount"] ?: "unknown"}",
            )
            addLocalBridgeEvent(
                NativeBridgeEvent.Warning(
                    VesperRuntimeWarning(
                        domain = "playback",
                        payload =
                            linkedMapOf(
                                "reason" to "firstFrameTimeout",
                                "route" to route.payloadValue,
                                "sourceUri" to source.uri,
                                "keySystem" to (drm?.keySystem ?: ""),
                                "playbackState" to exoPlaybackStateName(exoPlayer.playbackState),
                                "positionMs" to exoPlayer.currentPosition,
                                "durationMs" to exoPlayer.duration.normalizedDurationMs(),
                                "surfaceAttached" to (attachedSurface?.isValid == true),
                                "formatId" to (videoFormat?.id ?: ""),
                                "sampleMimeType" to (videoFormat?.sampleMimeType ?: ""),
                                "codecs" to (videoFormat?.codecs ?: ""),
                                "decoderName" to (currentVideoDecoderName ?: ""),
                            ) + decoderDiagnostics,
                    ),
                )
            )
            notifyNativeUpdate()
        }
    firstFrameWatchdogRunnable = runnable
    mainHandler.postDelayed(runnable, FIRST_FRAME_WATCHDOG_DELAY_MS)
}

internal fun VesperNativeJniBindings.cancelFirstFrameWatchdog() {
    firstFrameWatchdogRunnable?.let(mainHandler::removeCallbacks)
    firstFrameWatchdogRunnable = null
    firstFrameWatchdogSource = null
}

internal data class FirstFrameWatchdogRoute(
    val enabled: Boolean,
    val payloadValue: String,
) {
    companion object {
        fun systemPlayback(videoEnabled: Boolean): FirstFrameWatchdogRoute =
            FirstFrameWatchdogRoute(
                enabled = videoEnabled,
                payloadValue = "systemPlayer",
            )
    }
}

internal fun Int.media3DataTypeName(): String =
    when (this) {
        C.DATA_TYPE_MEDIA -> "media"
        C.DATA_TYPE_MEDIA_INITIALIZATION -> "mediaInitialization"
        C.DATA_TYPE_DRM -> "drm"
        C.DATA_TYPE_MANIFEST -> "manifest"
        C.DATA_TYPE_TIME_SYNCHRONIZATION -> "timeSynchronization"
        C.DATA_TYPE_AD -> "ad"
        C.DATA_TYPE_MEDIA_PROGRESSIVE_LIVE -> "mediaProgressiveLive"
        C.DATA_TYPE_UNKNOWN -> "unknown"
        else -> "custom($this)"
    }

internal fun Int.media3TrackTypeName(): String =
    when (this) {
        C.TRACK_TYPE_NONE -> "none"
        C.TRACK_TYPE_UNKNOWN -> "unknown"
        C.TRACK_TYPE_DEFAULT -> "default"
        C.TRACK_TYPE_AUDIO -> "audio"
        C.TRACK_TYPE_VIDEO -> "video"
        C.TRACK_TYPE_TEXT -> "text"
        C.TRACK_TYPE_IMAGE -> "image"
        C.TRACK_TYPE_METADATA -> "metadata"
        C.TRACK_TYPE_CAMERA_MOTION -> "cameraMotion"
        else -> "custom($this)"
    }

internal fun String.hostForDiagnostics(): String =
    runCatching { URI(this).host.orEmpty() }
        .getOrDefault("")

internal fun VesperNativeJniBindings.enqueueTerminalPlaybackError(
    message: String,
    classified: NativePlaybackError,
    reason: String,
    errorCodeName: String?,
    error: Throwable,
    extraDetails: Map<String, Any?> = emptyMap(),
) {
    if (terminalErrorReportedForCurrentSource) {
        return
    }
    terminalErrorReportedForCurrentSource = true
    addLocalBridgeEvent(
        NativeBridgeEvent.Error(
            message = message,
            codeOrdinal = classified.codeOrdinal,
            categoryOrdinal = classified.categoryOrdinal,
            retriable = classified.retriable,
            details =
                terminalPlaybackErrorDetails(
                    classified = classified,
                    reason = reason,
                    errorCodeName = errorCodeName,
                    error = error,
                    extraDetails = extraDetails,
                ),
        )
    )
}

internal fun VesperNativeJniBindings.terminalPlaybackErrorDetails(
    classified: NativePlaybackError,
    reason: String,
    errorCodeName: String?,
    error: Throwable,
    extraDetails: Map<String, Any?> = emptyMap(),
): Map<String, Any?> {
    val output = linkedMapOf<String, Any?>(
        "reason" to reason,
        "errorClass" to error::class.java.name,
        "errorMessage" to (error.message?.boundedFailureMessage() ?: ""),
    )
    errorCodeName?.takeIf(String::isNotBlank)?.let { output["errorCodeName"] = it }
    val drm = currentDrmDiagnosticsSource?.drmConfiguration
    if (drm != null) {
        output["keySystem"] = drm.keySystem
        output["licenseUriHost"] = drm.licenseUri.hostForDiagnostics()
        if (classified.categoryOrdinal == NETWORK_CATEGORY_ORDINAL) {
            output["attemptsExhausted"] = true
            currentRetryMaxAttempts?.let { output["maxAttempts"] = it }
        }
    }
    val videoFormat = player?.videoFormat
    videoFormat?.id?.takeIf(String::isNotBlank)?.let { output["formatId"] = it }
    videoFormat?.sampleMimeType?.takeIf(String::isNotBlank)?.let { output["sampleMimeType"] = it }
    videoFormat?.let(::nativeTrackCodec)?.takeIf(String::isNotBlank)?.let { output["codec"] = it }
    videoFormat?.width?.takeIf { it > 0 }?.let { output["width"] = it }
    videoFormat?.height?.takeIf { it > 0 }?.let { output["height"] = it }
    output["decoderName"] = currentVideoDecoderName ?: "pending"
    output.putAll(VesperHardwareMediaCodecSelector.decoderDiagnostics(videoFormat?.let(::videoMimeType)))
    classified.capabilityFailureCause?.let { output["capabilityFailureCause"] = it.wireName }
    classified.capabilityFailureAxis?.let { output["capabilityFailureAxis"] = it.wireName }
    output.putAll(classified.causeEvidence?.diagnostics().orEmpty())
    if (classified.likelyCapabilityIssue) {
        currentRuntimeHdrEvidence
            ?.failureHintPayload(errorCodeName ?: reason, classified, currentRuntimeSessionProbe)
            ?.let { hdrPayload ->
                output.putAll(hdrPayload)
                output["reason"] = reason
            }
    }
    output.putAll(extraDetails)
    return output
}

internal fun PlaybackException.terminalPlaybackErrorReason(): String =
    when (errorCode) {
        PlaybackException.ERROR_CODE_DRM_PROVISIONING_FAILED -> "drmProvisioningFailed"
        PlaybackException.ERROR_CODE_DRM_LICENSE_ACQUISITION_FAILED -> "drmLicenseAcquisitionFailed"
        PlaybackException.ERROR_CODE_DRM_SYSTEM_ERROR -> "drmSystemError"
        PlaybackException.ERROR_CODE_DRM_LICENSE_EXPIRED -> "drmLicenseExpired"
        PlaybackException.ERROR_CODE_DRM_CONTENT_ERROR -> "drmContentError"
        PlaybackException.ERROR_CODE_DRM_UNSPECIFIED -> "drmRuntimeError"
        PlaybackException.ERROR_CODE_DRM_SCHEME_UNSUPPORTED -> "drmUnsupportedKeySystem"
        PlaybackException.ERROR_CODE_DRM_DISALLOWED_OPERATION -> "drmDisallowedOperation"
        PlaybackException.ERROR_CODE_DRM_DEVICE_REVOKED -> "drmDeviceRevoked"
        PlaybackException.ERROR_CODE_DECODER_INIT_FAILED -> "decoderInit"
        PlaybackException.ERROR_CODE_DECODING_FAILED -> "decodeFailed"
        PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED -> "unsupportedFormat"
        PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES -> "formatExceedsCapabilities"
        else -> "playbackError"
    }

internal const val FIRST_FRAME_WATCHDOG_DELAY_MS = 15_000L

internal fun VesperNativeJniBindings.pushSnapshotToRust() {
    val handle = sessionHandle ?: return
    val exoPlayer = player ?: return
    val isLive = exoPlayer.isCurrentMediaItemLive
    val isSeekable = exoPlayer.isCurrentMediaItemSeekable
    val liveWindow = if (isLive) exoPlayer.currentLiveTimelineWindow() else null
    val rawDurationMs = exoPlayer.duration.normalizedDurationMs()
    val liveWindowStartMs = liveWindow?.startMs ?: 0L
    val liveWindowDurationMs = liveWindow?.durationMs ?: rawDurationMs.normalizedOptionalMs()
    val timelinePositionMs =
        if (isLive) {
            timelinePositionFromWindowPosition(liveWindowStartMs, exoPlayer.currentPosition)
        } else {
            exoPlayer.currentPosition.coerceAtLeast(0L)
        }
    val durationMs = liveWindowDurationMs ?: rawDurationMs
    val seekableStartMs = if (isLive && isSeekable && liveWindowDurationMs != null) {
        liveWindowStartMs
    } else {
        C.TIME_UNSET
    }
    val seekableEndMs =
        if (seekableStartMs >= 0L && liveWindowDurationMs != null) {
            seekableStartMs + liveWindowDurationMs
        } else {
            C.TIME_UNSET
        }
    val liveEdgeMs = when {
        !isLive -> C.TIME_UNSET
        seekableEndMs >= 0L -> seekableEndMs
        else -> exoPlayer.currentLiveOffset.normalizedOptionalMs()?.let {
            (timelinePositionMs + it).coerceAtLeast(0L)
        } ?: C.TIME_UNSET
    }
    logExoSnapshotToRust(
        playbackState = exoPlayer.playbackState,
        isLive = isLive,
        isSeekable = isSeekable,
        windowPositionMs = exoPlayer.currentPosition,
        timelinePositionMs = timelinePositionMs,
        durationMs = durationMs,
        seekableStartMs = seekableStartMs,
        seekableEndMs = seekableEndMs,
        liveEdgeMs = liveEdgeMs,
    )
    VesperNativeJni.applyExoSnapshot(
        handle,
        exoPlaybackStateOrdinal(exoPlayer.playbackState),
        exoPlayer.playWhenReady,
        exoPlayer.playbackParameters.speed,
        timelinePositionMs,
        durationMs,
        isLive,
        isSeekable,
        seekableStartMs,
        seekableEndMs,
        liveEdgeMs,
    )
}

private fun VesperNativeJniBindings.logExoSnapshotToRust(
    playbackState: Int,
    isLive: Boolean,
    isSeekable: Boolean,
    windowPositionMs: Long,
    timelinePositionMs: Long,
    durationMs: Long,
    seekableStartMs: Long,
    seekableEndMs: Long,
    liveEdgeMs: Long,
) {
    if (!Log.isLoggable(NATIVE_JNI_BINDINGS_TAG, Log.DEBUG)) {
        return
    }
    val nowMs = SystemClock.elapsedRealtime()
    if (nowMs - lastSnapshotLogElapsedMs < VesperNativeJniBindings.EXO_SNAPSHOT_LOG_INTERVAL_MS) {
        return
    }
    lastSnapshotLogElapsedMs = nowMs
    Log.d(
        NATIVE_JNI_BINDINGS_TAG,
        "pushSnapshotToRust state=${exoPlaybackStateName(playbackState)} live=$isLive seekable=$isSeekable windowPositionMs=$windowPositionMs timelinePositionMs=$timelinePositionMs durationMs=$durationMs seekableStartMs=$seekableStartMs seekableEndMs=$seekableEndMs liveEdgeMs=$liveEdgeMs",
    )
}

internal fun VesperNativeJniBindings.pushTrackStateToRust() {
    val handle = sessionHandle ?: return
    val exoPlayer = player ?: return
    val trackCatalog = collectTrackCatalog(exoPlayer.currentTracks, currentSourceProtocol)
    val trackSelection =
        collectTrackSelection(
            exoPlayer.currentTracks,
            exoPlayer.trackSelectionParameters,
            currentSourceProtocol,
        )
    val publicTrackCatalog = trackCatalog.toPublicTrackCatalog()
    currentSubtitleCatalogFailure = trackCatalog.subtitleIdentityFailure
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
        NATIVE_JNI_BINDINGS_TAG,
        "pushTrackStateToRust tracks=${trackCatalog.tracks.size} adaptiveVideo=${trackCatalog.adaptiveVideo} adaptiveAudio=${trackCatalog.adaptiveAudio} videoMode=${trackSelection.video.modeOrdinal} audioMode=${trackSelection.audio.modeOrdinal} subtitleMode=${trackSelection.subtitle.modeOrdinal} abrMode=${trackSelection.abrPolicy.modeOrdinal} effectiveVideoTrackId=$effectiveVideoTrackId observation=$videoVariantObservation",
    )
    VesperNativeJni.applyTrackState(handle, trackCatalog, trackSelection)
}

internal fun VesperNativeJniBindings.executePreloadWarmupCommands(source: VesperPlayerSource) {
    preloadCoordinator.planCurrentSource(source).forEach { command ->
        when (command) {
            is NativePreloadCommand.Start -> dispatchWarmup(command.task, source)
            is NativePreloadCommand.Cancel -> Unit
        }
    }
}

private fun VesperNativeJniBindings.dispatchWarmup(
    task: NativePreloadTask,
    currentSource: VesperPlayerSource,
) {
    val submitted =
        VesperPreloadWarmupDispatcher.execute(
            Runnable {
                runWarmup(task, currentSource)
            }
        )
    if (!submitted) {
        preloadCoordinator.fail(
            task.taskId,
            NativeBridgeEvent.Error(
                message = "android preload warmup queue is full",
                codeOrdinal = BACKEND_FAILURE_ORDINAL,
                categoryOrdinal = PLATFORM_CATEGORY_ORDINAL,
                retriable = true,
            ),
        )
    }
}

internal fun VesperNativeJniBindings.runWarmup(task: NativePreloadTask, currentSource: VesperPlayerSource) {
    val source =
        currentSource.takeIf { it.uri == task.sourceUri }
            ?: currentSourceOrFallback(task.sourceUri)
    val resolvedResiliencePolicy = resolveResiliencePolicy(source, VesperPlaybackResiliencePolicy())
    val dataSourceFactory = buildDataSourceFactory(
        appContext,
        resolvedResiliencePolicy.cache,
        source.headers,
    )
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

internal fun VesperNativeJniBindings.currentSourceOrFallback(uri: String): VesperPlayerSource {
    return VesperPlayerSource(
        uri = uri,
        label = URI(uri).path.substringAfterLast('/').ifBlank { uri },
        kind = inferSourceKind(uri),
        protocol = inferSourceProtocol(uri),
    )
}

internal fun VesperNativeJniBindings.enqueueHdrCapabilityWarningIfNeeded(format: Format) {
    val evidence = format.androidRuntimeHdrEvidence() ?: return
    addLocalBridgeEvent(
        NativeBridgeEvent.Warning(
            VesperRuntimeWarning(
                domain = "capability",
                payload = evidence.capabilityWarningPayload(),
            )
        )
    )
}

internal fun VesperNativeJniBindings.enqueueHdrFailureHintIfNeeded(
    error: PlaybackException,
    classified: NativePlaybackError,
) {
    val evidence = currentRuntimeHdrEvidence ?: return
    if (!classified.likelyCapabilityIssue) {
        return
    }
    addLocalBridgeEvent(
        NativeBridgeEvent.Warning(
            VesperRuntimeWarning(
                domain = "capability",
                payload =
                    evidence.failureHintPayload(
                        error.errorCodeName,
                        classified,
                        currentRuntimeSessionProbe,
                    ),
            )
        )
    )
}

internal fun VesperNativeJniBindings.buildRuntimeSessionProbeSnapshot(format: Format): AndroidRuntimeSessionProbeSnapshot? {
    val codec = nativeTrackCodec(format)?.takeIf(String::isNotBlank) ?: return null
    val result =
        VesperPlaybackCapabilityProbe.probe(
            request =
                VesperPlaybackCapabilityProbeRequest(
                    source = player?.currentMediaItem?.localConfiguration?.uri?.toString()?.let(::currentSourceOrFallback),
                    codec = codec,
                    width = format.width.takeIf { it != Format.NO_VALUE && it > 0 },
                    height = format.height.takeIf { it != Format.NO_VALUE && it > 0 },
                    frameRate = format.frameRate.takeIf { it.isFinite() && it > 0f },
                ),
            sessionProbeProvider = VesperAndroidDisplaySessionProbeProvider.fromContext(appContext),
        )
    return AndroidRuntimeSessionProbeSnapshot(result)
}

internal fun VesperNativeJniBindings.notifyNativeUpdate() {
    systemPlaybackCoordinator.refreshFromPlayer()
    val listener = updateListener ?: return
    if (Looper.myLooper() == Looper.getMainLooper()) {
        listener.invoke()
    } else {
        mainHandler.post { listener.invoke() }
    }
}

internal fun VesperNativeJniBindings.prepareSourceNormalizerResourceForPlayback(
    source: VesperPlayerSource,
    enabled: Boolean,
): NativeSourceNormalizerResourcePreparedOpenOutcome {
    if (!enabled) {
        Log.i(NATIVE_JNI_BINDINGS_TAG, "source normalizer resource playback skipped for SDK-managed native-frame route")
        return NativeSourceNormalizerResourcePreparedOpenOutcome()
    }
    if (!sourceNormalizerConfiguration.shouldOpenNormalizedResourceForPlayback(source)) {
        return NativeSourceNormalizerResourcePreparedOpenOutcome()
    }
    VesperNativeLibrary.ensureLoaded()
    val outputRoot = File(appContext.cacheDir, "vesper-source-normalizer").absolutePath
    val json =
        try {
            VesperNativeJni.openSourceNormalizerResource(
                source.uri,
                sourceNormalizerConfiguration.modeOrdinal,
                sourceNormalizerConfiguration.pluginLibraryPaths.toTypedArray(),
                sourceNormalizerConfiguration.runtimeProfile,
                outputRoot,
                sourceNormalizerConfiguration.mode == VesperSourceNormalizerMode.RequireNormalized,
            )
        } catch (error: Throwable) {
            if (sourceNormalizerConfiguration.mode == VesperSourceNormalizerMode.RequireNormalized) {
                throw error
            }
            Log.w(NATIVE_JNI_BINDINGS_TAG, "source normalizer normalized resource open failed; using original source", error)
            null
        } ?: return NativeSourceNormalizerResourcePreparedOpenOutcome()

    parseSourceNormalizerBypassDiagnostics(json)?.let { diagnostics ->
        val bypassReason = sourceNormalizerBypassReason(diagnostics)
        Log.i(NATIVE_JNI_BINDINGS_TAG, "source normalizer resource bypassed; route=native fallbackReason=$bypassReason")
        return NativeSourceNormalizerResourcePreparedOpenOutcome(diagnostics = diagnostics)
    }
    val resource =
        parseSourceNormalizerResource(json, source, sourceNormalizerLoopbackServer)
            ?: run {
                disposeSourceNormalizerResourceHandle(
                    sourceNormalizerResourceHandle(json),
                    "stale prepared source normalizer resource",
                )
                return NativeSourceNormalizerResourcePreparedOpenOutcome()
            }
    return NativeSourceNormalizerResourcePreparedOpenOutcome(resource = resource)
}

internal fun VesperNativeJniBindings.openPreparedSourceNormalizerResourceForPlayback(
    source: VesperPlayerSource,
    prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
): NativeSourceNormalizerResourceOpenOutcome {
    closeCurrentSourceNormalizerResource()
    val resource = prepared.resource
        ?: return NativeSourceNormalizerResourceOpenOutcome(diagnostics = prepared.diagnostics)
    currentSourceNormalizerResource = resource
    Log.i(
        NATIVE_JNI_BINDINGS_TAG,
        "source normalizer resource selected route=${resource.outputRoute} playbackUri=${resource.playbackSource.uri}",
    )
    return NativeSourceNormalizerResourceOpenOutcome(resource = resource)
}

internal fun VesperNativeJniBindings.disposePreparedSourceNormalizerResourceForPlayback(
    prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
) {
    prepared.resource?.let { resource ->
        resource.loopbackToken?.let(sourceNormalizerLoopbackServer::invalidate)
        disposeSourceNormalizerResourceHandle(
            resource.handle,
            "stale prepared source normalizer resource",
        )
        return
    }
    val handle = prepared.resourceJson?.let(::sourceNormalizerResourceHandle) ?: return
    disposeSourceNormalizerResourceHandle(
        handle,
        "stale prepared source normalizer resource",
    )
}

private fun disposeSourceNormalizerResourceHandle(
    handle: Long,
    context: String,
) {
    if (handle == 0L) {
        return
    }
    runCatching { VesperNativeJni.disposeSourceNormalizerResource(handle) }
        .onFailure { error ->
            Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to dispose $context", error)
        }
}

internal fun VesperNativeJniBindings.closeCurrentSourceNormalizerResource() {
    val resource = currentSourceNormalizerResource ?: return
    currentSourceNormalizerResource = null
    detachPlayerFromSourceNormalizerResource(resource, player)
    resource.loopbackToken?.let(sourceNormalizerLoopbackServer::invalidate)
    VesperSourceNormalizerResourceDisposer.disposeAsync(resource.handle)
}

internal fun VesperNativeJniBindings.detachPlayerFromSourceNormalizerResource(
    resource: NativeSourceNormalizerResource,
    exoPlayer: ExoPlayer?,
) {
    if (exoPlayer == null) {
        return
    }
    val currentUri = exoPlayer.currentMediaItem?.localConfiguration?.uri?.toString()
    if (currentUri != resource.playbackSource.uri) {
        return
    }
    runCatching {
        exoPlayer.stop()
        exoPlayer.clearMediaItems()
    }.onFailure { error ->
        Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to detach ExoPlayer from normalized resource playback", error)
    }
}

internal fun VesperNativeJniBindings.openNativeFramePacketSource(source: VesperPlayerSource): NativeFramePacketSource {
    if (
        source.protocol != VesperPlayerSourceProtocol.Content &&
        !source.uri.startsWith("content://", ignoreCase = true)
    ) {
        return NativeFramePacketSource(source = source)
    }
    error(
        "Android native-frame packet input requires a file:// or app-private file path. " +
            "Copy content:// media with ContentResolver before enabling SDK-managed native-frame playback.",
    )
}

internal fun VesperNativeJniBindings.closeCurrentNativeFramePacketSource() {
    currentNativeFramePacketSource?.close()
    currentNativeFramePacketSource = null
}

internal fun VesperNativeJniBindings.recordBenchmark(
    eventName: String,
    attributes: Map<String, String> = emptyMap(),
) {
    val enrichedAttributes =
        if (firstFrameGate.currentEpoch > 0L) {
            attributes + ("playbackEpoch" to firstFrameGate.currentEpoch.toString())
        } else {
            attributes
        }
    benchmarkRecorder.record(eventName, currentBenchmarkSourceProtocol, enrichedAttributes)
}
