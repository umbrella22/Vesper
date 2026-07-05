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

internal const val NATIVE_PLAYER_BRIDGE_TAG = "VesperPlayerAndroidHost"
internal const val NATIVE_FRAME_PIPELINE_ACTIVE_PUMP_DELAY_MS = 8L
internal const val NATIVE_FRAME_PIPELINE_IDLE_PUMP_DELAY_MS = 16L
internal const val NATIVE_FRAME_PIPELINE_BACKPRESSURE_PUMP_DELAY_MS = 32L
internal const val NATIVE_FRAME_PIPELINE_MAX_FRAME_DELAY_MS = 100L
internal const val NATIVE_FRAME_PIPELINE_FIRST_FRAME_TIMEOUT_MS = 2_500L
internal const val NATIVE_FRAME_PIPELINE_LOG_COUNTER_BUCKET_SIZE = 30L

internal data class TimedNativeFrameRelease(
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

internal class HandlerNativeFramePipelinePumpScheduler(
    private val inlineRuntimeCommandsForLocalTests: Boolean = isLocalUnitTestRuntime(),
) : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = inlineRuntimeCommandsForLocalTests
    private val thread: HandlerThread by lazy {
        HandlerThread("VesperNativeFramePump").apply {
            // Mark as daemon so a host that forgets to call close() cannot
            // keep the JVM alive past player release.
            isDaemon = true
            start()
        }
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
    }

    fun quitLooperSafely() {
        // Called after close() releases the monitor so that the blocking
        // quitSafely() does not hold a lock (AGENTS.md rule).
        if (started && thread.isAlive) {
            thread.quitSafely()
        }
    }
}

internal fun isLocalUnitTestRuntime(): Boolean =
    System.getProperty("java.vm.name")
        ?.contains("Dalvik", ignoreCase = true) != true

internal data class PreservedPlaybackState(
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

internal class MissingVesperNativeBindings : VesperNativeBindings {
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
