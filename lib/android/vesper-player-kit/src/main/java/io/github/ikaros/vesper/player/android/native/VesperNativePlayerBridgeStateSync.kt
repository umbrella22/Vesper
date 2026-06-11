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
import org.json.JSONArray
import org.json.JSONObject

internal inline fun VesperNativePlayerBridge.updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
    _uiState.value = _uiState.value.transform()
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

internal fun VesperNativePlayerBridge.restorePlaybackState(
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

internal fun VesperNativePlayerBridge.refreshFromNative() {
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

internal fun VesperNativePlayerBridge.installNativeUpdateListener() {
    val epoch = nativeUpdateEpoch
    bindings.setOnNativeUpdateListener {
        if (isDisposed.get() || epoch != nativeUpdateEpoch) {
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
    _trackCatalog.value = VesperTrackCatalog.Empty
    _trackSelection.value = VesperTrackSelectionSnapshot()
    _effectiveVideoTrackId.value = null
    _videoVariantObservation.value = null
}

internal fun VesperNativePlayerBridge.sourceSubtitle(source: VesperPlayerSource): String = i18n.sourceSubtitle(source)
