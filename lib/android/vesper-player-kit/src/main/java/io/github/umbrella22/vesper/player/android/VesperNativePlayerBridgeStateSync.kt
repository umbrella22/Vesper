package io.github.umbrella22.vesper.player.android

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
import kotlin.math.absoluteValue
import kotlin.math.pow
import kotlin.math.roundToLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

internal inline fun VesperNativePlayerBridge.updateState(crossinline transform: PlayerHostUiState.() -> PlayerHostUiState) {
    _uiState.update { it.transform() }
    syncKeepScreenOn()
}

internal fun VesperNativePlayerBridge.syncKeepScreenOn() {
    surfaceHost.setKeepScreenOn(
        !isDisposed.get() &&
            keepScreenOnDuringPlayback &&
            _uiState.value.playbackState == PlaybackStateUi.Playing,
    )
}

internal fun VesperNativePlayerBridge.recordBenchmark(
    eventName: String,
    attributes: Map<String, String> = emptyMap(),
) {
    benchmarkRecorder.record(eventName, currentSource?.protocol, attributes)
}

internal suspend fun VesperNativePlayerBridge.restorePlaybackState(
    source: VesperPlayerSource,
    preservedState: PreservedPlaybackState,
): Unit = withContext(Dispatchers.Main.immediate) {
    if (!hasInitializedSource || currentSource != source) return@withContext

    _confirmedSubtitleSelection.value = preservedState.subtitleSelection
    // The old item's effective track is not valid until the Coordinator has
    // applied and confirmed the selection on the replacement item.
    _effectiveSubtitleTrackId.value = null
    _trackSelection.value = _trackSelection.value.copy(
        confirmedSubtitle = preservedState.subtitleSelection,
        effectiveSubtitleTrackId = null,
    )

    when {
        preservedState.seekToLiveEdge &&
            _uiState.value.timeline.kind == TimelineKind.LiveDvr -> {
            val liveEdge =
                _uiState.value.timeline.goLivePositionMs ?: _uiState.value.timeline.positionMs
            if (!seekBindingsTo(liveEdge)) {
                return@withContext
            }
        }
        preservedState.restorePosition &&
            (source.kind == VesperPlayerSourceKind.Local ||
                source.kind == VesperPlayerSourceKind.Remote) -> {
            if (!seekBindingsTo(preservedState.positionMs.coerceAtLeast(0L))) {
                return@withContext
            }
        }
    }

    if ((preservedState.playbackRate - 1.0f).absoluteValue > 0.001f) {
        if (isRequiredNativeFramePipelineFailureActive()) {
            return@withContext
        }
        bindings.setPlaybackRate(preservedState.playbackRate)
    }

    if (preservedState.videoSelection.mode != VesperTrackSelectionMode.Auto) {
        if (isRequiredNativeFramePipelineFailureActive()) {
            return@withContext
        }
        bindings.setVideoTrackSelection(preservedState.videoSelection)
    }
    if (preservedState.audioSelection.mode != VesperTrackSelectionMode.Auto) {
        if (isRequiredNativeFramePipelineFailureActive()) {
            return@withContext
        }
        bindings.setAudioTrackSelection(preservedState.audioSelection)
    }
    if (!isRequiredNativeFramePipelineFailureActive()) {
        try {
            applySubtitleSelectionTransaction(preservedState.subtitleSelection)
        } catch (error: VesperPlayerUnsupportedOperation) {
            Log.w(
                NATIVE_PLAYER_BRIDGE_TAG,
                "failed to restore confirmed subtitle selection",
                error,
            )
        }
    }
    if (!hasInitializedSource || currentSource != source || isRequiredNativeFramePipelineFailureActive()) {
        return@withContext
    }
    bindings.setAbrPolicy(preservedState.abrPolicy)

    if (preservedState.shouldResumePlayback) {
        if (isRequiredNativeFramePipelineFailureActive()) {
            return@withContext
        }
        bindings.play()
        nativeFramePipelinePlaybackRequested = true
        startNativeFramePipelinePump("restore-playback")
    } else if (preservedState.playbackState == PlaybackStateUi.Paused) {
        if (isRequiredNativeFramePipelineFailureActive()) {
            return@withContext
        }
        stopNativeFramePipelinePump()
        nativeFramePipelinePlaybackRequested = false
        bindings.pause()
    }

    refreshFromNative()
}

internal fun VesperNativePlayerBridge.refreshFromNative() {
    if (isDisposed.get() ||
        isRequiredNativeFramePipelineFailureActive() ||
        !hasInitializedSource ||
        activeNativeItemEpoch != nativeUpdateEpoch
    ) {
        return
    }
    surfaceHost.updateVideoLayout(bindings.currentVideoLayoutInfo())
    _trackCatalog.value = bindings.currentTrackCatalog()
    val nativeTrackSelection = bindings.currentTrackSelection()
    _trackSelection.value =
        nativeTrackSelection.copy(
            subtitle = _requestedSubtitleSelection.value,
            confirmedSubtitle = _confirmedSubtitleSelection.value,
            effectiveSubtitleTrackId = _effectiveSubtitleTrackId.value,
        )
    observeSubtitleSelectionConfirmation(bindings.currentAppliedSubtitleSelection())
    if (pendingSubtitleSelection == null &&
        _subtitleState.value.selectionState != VesperSubtitleSelectionState.Failed
    ) {
        val confirmedSubtitle = _confirmedSubtitleSelection.value
        val rendererSelectionMatchesConfirmed =
            when (subtitleSelectionCoordinatorMode) {
                VesperTrackSelectionMode.Track ->
                    nativeTrackSelection.subtitle.trackId == confirmedSubtitle.trackId
                VesperTrackSelectionMode.Disabled ->
                    nativeTrackSelection.subtitle.mode == VesperTrackSelectionMode.Disabled
                VesperTrackSelectionMode.Auto,
                null,
                -> true
            }
        if (rendererSelectionMatchesConfirmed) {
            _effectiveSubtitleTrackId.value =
                if (nativeTrackSelection.subtitle.mode == VesperTrackSelectionMode.Disabled) {
                    null
                } else {
                    nativeTrackSelection.subtitle.trackId
                }
            _trackSelection.value = _trackSelection.value.copy(
                effectiveSubtitleTrackId = _effectiveSubtitleTrackId.value,
            )
        }
    }
    _effectiveVideoTrackId.value = bindings.currentEffectiveVideoTrackId()
    _videoVariantObservation.value = bindings.currentVideoVariantObservation()
    // Derive the first-class subtitle state from the refreshed catalog and
    // preserve structured command failures. Identity failures are produced
    // by the DASH catalog owner and must not be hidden by an empty catalog.
    val subtitleCount = _trackCatalog.value.subtitleTracks.size
    val currentSubtitleState = _subtitleState.value
    val catalogFailure = bindings.currentSubtitleCatalogFailure()
    if (catalogFailure != null) {
        val phase = VesperSubtitleErrorPhase.fromWire(catalogFailure.phase)
        val error =
            VesperSubtitleError(
                code = catalogFailure.code,
                phase = phase,
                phaseRawValue = catalogFailure.phase.takeIf {
                    phase == VesperSubtitleErrorPhase.Unknown
                },
                trackId = catalogFailure.trackId,
                // Retryability is part of the structured native error. Do not
                // infer it from the display phase, or explicit non-retriable
                // resource failures and unknown phases are corrupted.
                retriable = catalogFailure.retriable,
                message = catalogFailure.message,
            )
        val advertisedCount =
            maxOf(
                catalogFailure.advertisedTrackCount ?: 0,
                bindings.currentAdvertisedSubtitleTrackCount(),
            )
        _subtitleState.value =
            if (catalogFailure.phase == VesperSubtitleErrorPhase.Identity.wireName || subtitleCount == 0) {
                currentSubtitleState.copy(
                    catalogState = VesperSubtitleCatalogState.Failed,
                    catalogError = error,
                    advertisedTrackCount = advertisedCount,
                    selectableTrackCount = subtitleCount,
                )
            } else {
                currentSubtitleState.copy(
                    catalogState = VesperSubtitleCatalogState.Ready,
                    catalogError = error,
                    advertisedTrackCount = advertisedCount,
                    selectableTrackCount = subtitleCount,
                )
            }
    } else if (!bindings.isTrackCatalogReady()) {
        _subtitleState.value =
            VesperSubtitleState.loading(
                advertisedTrackCount = bindings.currentAdvertisedSubtitleTrackCount(),
            )
    } else {
        val advertisedCount = bindings.currentAdvertisedSubtitleTrackCount()
        _subtitleState.value =
            if (subtitleCount > 0) {
                currentSubtitleState.copy(
                    catalogState = VesperSubtitleCatalogState.Ready,
                    advertisedTrackCount = advertisedCount,
                    selectableTrackCount = subtitleCount,
                    catalogError = null,
                    selectionState = when {
                        currentSubtitleState.selectionError != null ->
                            VesperSubtitleSelectionState.Failed
                        currentSubtitleState.selectionState ==
                            VesperSubtitleSelectionState.Applying ->
                            VesperSubtitleSelectionState.Applying
                        currentSubtitleState.selectionState ==
                            VesperSubtitleSelectionState.Confirmed ->
                            VesperSubtitleSelectionState.Confirmed
                        else -> VesperSubtitleSelectionState.Idle
                    },
                )
            } else if (advertisedCount > 0) {
                currentSubtitleState.copy(
                    catalogState = VesperSubtitleCatalogState.Failed,
                    advertisedTrackCount = advertisedCount,
                    selectableTrackCount = 0,
                    catalogError =
                        VesperSubtitleError(
                            code = "subtitle_platform_track_unavailable",
                            phase = VesperSubtitleErrorPhase.Discovery,
                            retriable = false,
                            message = "the platform cannot select any advertised subtitle track",
                        ),
                )
            } else {
                currentSubtitleState.copy(
                    catalogState = VesperSubtitleCatalogState.Unavailable,
                    advertisedTrackCount = advertisedCount,
                    selectableTrackCount = 0,
                    catalogError = null,
                )
            }
    }
    currentNativeFramePipelineStatusOnRuntime()

    bindings.pollSnapshot()?.let { snapshot ->
        updateState {
            val terminalErrorActive = lastError != null
            copy(
                playbackState = if (terminalErrorActive) PlaybackStateUi.Paused else snapshot.playbackState,
                playbackRate = snapshot.playbackRate,
                isBuffering = if (terminalErrorActive) false else snapshot.isBuffering,
                isInterrupted = if (terminalErrorActive) false else snapshot.isInterrupted,
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
                val retryStatusMessage = activeRetryStatusMessage
                if (event.state == PlaybackStateUi.Playing) {
                    activeRetryStatusMessage = null
                }
                nativeFramePipelinePlaybackRequested =
                    event.state == PlaybackStateUi.Playing && _uiState.value.lastError == null
                updateState {
                    if (lastError != null) {
                        copy(playbackState = PlaybackStateUi.Paused)
                    } else {
                        copy(
                            subtitle =
                                if (
                                    event.state == PlaybackStateUi.Playing &&
                                    retryStatusMessage != null &&
                                    subtitle == retryStatusMessage
                                ) {
                                    currentSource?.let(::sourceSubtitle) ?: i18n.nativeBridgeReady()
                                } else {
                                    subtitle
                                },
                            playbackState = event.state,
                        )
                    }
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
                    copy(isBuffering = if (lastError != null) false else event.isBuffering)
                }
            }
            is NativeBridgeEvent.InterruptionChanged -> {
                recordBenchmark(
                    "interruption_changed",
                    mapOf("isInterrupted" to event.isInterrupted.toString()),
                )
                updateState {
                    copy(isInterrupted = if (lastError != null) false else event.isInterrupted)
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
                val retryStatusMessage =
                    i18n.retryScheduled(
                        i18n.retryDelay(event.delayMs),
                        event.attempt,
                    )
                activeRetryStatusMessage = retryStatusMessage
                updateState { copy(subtitle = retryStatusMessage) }
            }
            is NativeBridgeEvent.Ended -> {
                recordBenchmark("playback_ended")
                updateState {
                    copy(playbackState = PlaybackStateUi.Finished, isBuffering = false)
                }
            }
            is NativeBridgeEvent.Warning -> {
                // Synchronize on `runtimeWarnings` to match the producer
                // paths: the JNI track-selection failure listener added in
                // The native selection callback can fire from any thread that drives
                // `setSubtitleTrackSelection`, and this branch historically
                // ran only on the main thread. Without the lock the two
                // producers can corrupt the ArrayDeque concurrently.
                synchronized(runtimeWarnings) {
                    if (runtimeWarnings.size >= VesperNativePlayerBridge.MAX_RUNTIME_WARNINGS) {
                        runtimeWarnings.removeFirst()
                    }
                    runtimeWarnings += event.warning
                }
            }
            is NativeBridgeEvent.Error -> {
                recordBenchmark(
                    "playback_error",
                    mapOf(
                        "categoryOrdinal" to event.categoryOrdinal.toString(),
                        "retriable" to event.retriable.toString(),
                    ),
                )
                if (_uiState.value.lastError != null) {
                    return@forEach
                }
                activeRetryStatusMessage = null
                val terminalError = event.toPlayerErrorState()
                stopNativeFramePipelinePump()
                nativeFramePipelinePlaybackRequested = false
                runCatching { bindings.pause() }
                updateState {
                    copy(
                        subtitle = i18n.nativeError(event.message),
                        playbackState = PlaybackStateUi.Paused,
                        isBuffering = false,
                        isInterrupted = false,
                        lastError = terminalError,
                    )
                }
            }
        }
    }

    syncNativeFramePipelinePumpWithPlaybackState()
}

internal fun VesperNativePlayerBridge.installNativeUpdateListener() {
    val epoch = nativeUpdateEpoch
    val subtitleEpoch = subtitleSourceEpoch
    bindings.setOnNativeUpdateListener {
        if (isDisposed.get() ||
            epoch != nativeUpdateEpoch ||
            subtitleEpoch != subtitleSourceEpoch ||
            !hasInitializedSource ||
            activeNativeItemEpoch != epoch
        ) {
            return@setOnNativeUpdateListener
        }
        refreshFromNative()
    }
}

internal fun VesperNativePlayerBridge.advanceNativeUpdateEpoch(clearListener: Boolean = false) {
    nativeUpdateEpoch += 1
    if (clearListener) {
        bindings.setOnNativeUpdateListener(null)
    } else {
        installNativeUpdateListener()
    }
}

internal fun VesperNativePlayerBridge.clearTrackState() {
    hasInitializedSource = false
    activeNativeItemEpoch = null
    advanceNativeUpdateEpoch(clearListener = true)
    advanceSubtitleSourceEpoch()
    _trackCatalog.value = VesperTrackCatalog.Empty
    _trackSelection.value = VesperTrackSelectionSnapshot()
    _effectiveVideoTrackId.value = null
    _videoVariantObservation.value = null
    _subtitleState.value = VesperSubtitleState.EMPTY
}

internal fun VesperNativePlayerBridge.sourceSubtitle(source: VesperPlayerSource): String = i18n.sourceSubtitle(source)
