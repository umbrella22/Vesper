package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import androidx.media3.common.text.Cue
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
internal const val NATIVE_FRAME_PIPELINE_RUNTIME_COMMAND_QUEUE_CAPACITY = 32

internal data class TimedNativeFrameRelease(
    val handle: Long,
    val presentationTimeUs: Long,
    val pumpEpoch: Long,
)

internal interface NativeFramePipelinePumpScheduler {
    val inlineCallbacksForTests: Boolean
        get() = false
    fun schedule(delayMs: Long, action: () -> Unit)
    fun execute(action: () -> Unit) = schedule(delayMs = 0L, action)
    fun executeCommand(command: NativeFramePipelineRuntimeCommand) = execute(command.action)
    fun cancel()
    fun close() = cancel()
    fun quitLooperSafely() = Unit
}

internal data class NativeFramePipelineRuntimeCommand(
    val operation: String,
    val coalescingKey: String? = null,
    val runsDuringClose: Boolean = false,
    val replacesPendingCommands: Boolean = false,
    val action: () -> Unit,
    val onRejected: (() -> Unit)? = null,
)

internal class BoundedNativeFramePipelineRuntimeCommandQueue(
    private val capacity: Int = NATIVE_FRAME_PIPELINE_RUNTIME_COMMAND_QUEUE_CAPACITY,
) {
    private val commands = ArrayDeque<NativeFramePipelineRuntimeCommand>()

    init {
        require(capacity > 0) { "native-frame runtime command queue capacity must be positive" }
    }

    val size: Int
        get() = commands.size

    fun enqueue(command: NativeFramePipelineRuntimeCommand): Boolean {
        if (command.replacesPendingCommands) {
            commands.clear()
            commands.addLast(command)
            return true
        }
        command.coalescingKey?.let { key ->
            val replaced = replacePendingCoalescedCommand(key, command)
            if (replaced) {
                return true
            }
        }
        if (commands.size < capacity) {
            commands.addLast(command)
            return true
        }
        if (evictOldestCoalescibleCommand()) {
            commands.addLast(command)
            return true
        }
        if (command.runsDuringClose && evictOldestNonCleanupCommand()) {
            commands.addLast(command)
            return true
        }
        return false
    }

    fun removeFirstOrNull(): NativeFramePipelineRuntimeCommand? =
        commands.removeFirstOrNull()

    fun retainCommandsAllowedDuringClose() {
        if (commands.isEmpty()) {
            return
        }
        val retained = commands.filter { it.runsDuringClose }
        commands.clear()
        retained.takeLast(capacity).forEach(commands::addLast)
    }

    fun clear() {
        commands.clear()
    }

    private fun replacePendingCoalescedCommand(
        key: String,
        command: NativeFramePipelineRuntimeCommand,
    ): Boolean {
        if (commands.isEmpty()) {
            return false
        }
        val retained = ArrayDeque<NativeFramePipelineRuntimeCommand>(commands.size)
        var replaced = false
        while (true) {
            val next = commands.removeFirstOrNull() ?: break
            if (!replaced && next.coalescingKey == key) {
                retained.addLast(command)
                replaced = true
            } else {
                retained.addLast(next)
            }
        }
        commands.addAll(retained)
        return replaced
    }

    private fun evictOldestCoalescibleCommand(): Boolean =
        evictFirst { it.coalescingKey != null && !it.runsDuringClose }

    private fun evictOldestNonCleanupCommand(): Boolean =
        evictFirst { !it.runsDuringClose }

    private fun evictFirst(predicate: (NativeFramePipelineRuntimeCommand) -> Boolean): Boolean {
        if (commands.isEmpty()) {
            return false
        }
        val retained = ArrayDeque<NativeFramePipelineRuntimeCommand>(commands.size)
        var evicted = false
        while (true) {
            val next = commands.removeFirstOrNull() ?: break
            if (!evicted && predicate(next)) {
                evicted = true
            } else {
                retained.addLast(next)
            }
        }
        commands.addAll(retained)
        return evicted
    }
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
    private val runtimeCommands = BoundedNativeFramePipelineRuntimeCommandQueue()
    private var runtimeCommandDrainScheduled = false
    private val runtimeCommandDrainRunnable = Runnable { drainRuntimeCommands() }
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
        executeCommand(
            NativeFramePipelineRuntimeCommand(
                operation = "generic",
                action = action,
            )
        )
    }

    override fun executeCommand(command: NativeFramePipelineRuntimeCommand) {
        if (inlineRuntimeCommandsForLocalTests) {
            val shouldRun = synchronized(this) { !closed }
            if (shouldRun) {
                command.action()
            } else {
                command.onRejected?.invoke()
            }
            return
        }
        val accepted =
            synchronized(this) {
                if (closed) {
                    false
                } else {
                    started = true
                    runtimeCommands.enqueue(command).also { didEnqueue ->
                        if (didEnqueue) {
                            postRuntimeCommandDrainLocked()
                        }
                    }
                }
            }
        if (!accepted) {
            Log.w(
                NATIVE_PLAYER_BRIDGE_TAG,
                "native-frame runtime command queue rejected operation=${command.operation}",
            )
            command.onRejected?.invoke()
        }
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
            runtimeCommands.retainCommandsAllowedDuringClose()
            postRuntimeCommandDrainLocked()
        }
    }

    override fun quitLooperSafely() {
        // Called after close() releases the monitor so that the blocking
        // quitSafely() does not hold a lock (AGENTS.md rule).
        if (started && thread.isAlive) {
            thread.quitSafely()
        }
    }

    private fun postRuntimeCommandDrainLocked() {
        if (!runtimeCommandDrainScheduled) {
            runtimeCommandDrainScheduled = handler.post(runtimeCommandDrainRunnable)
        }
    }

    private fun drainRuntimeCommands() {
        while (true) {
            val command =
                synchronized(this) {
                    runtimeCommands.removeFirstOrNull()
                        ?: run {
                            runtimeCommandDrainScheduled = false
                            return
                        }
                }
            command.action()
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
    val effectiveSubtitleTrackId: String?,
    val abrPolicy: VesperAbrPolicy,
) {
    companion object {
        fun capture(
            uiState: PlayerHostUiState,
            trackSelection: VesperTrackSelectionSnapshot,
            confirmedSubtitleSelection: VesperTrackSelection = trackSelection.subtitle,
            effectiveSubtitleTrackId: String? = trackSelection.effectiveSubtitleTrackId,
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
                subtitleSelection = confirmedSubtitleSelection,
                effectiveSubtitleTrackId = effectiveSubtitleTrackId,
                abrPolicy = trackSelection.abrPolicy,
            )
        }
    }
}

internal interface VesperNativeBindings {
    /** Whether the bindings already own an active system-playback item. */
    val isSystemPlaybackActive: Boolean
        get() = false

    fun probeMobilePlugins(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
    ): List<Map<String, Any?>>

    fun prepareSourceNormalizerForPlayback(
        source: VesperPlayerSource,
        enabled: Boolean,
    ): NativeSourceNormalizerResourcePreparedOpenOutcome

    fun disposePreparedSourceNormalizerResource(
        prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
    )

    fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
        preparedSourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
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
    /** Invalidates callbacks and queued events owned by the current system player. */
    fun invalidateSystemPlaybackCallbacks() = Unit
    fun dispose()
    fun refreshSnapshot()
    /** Refreshes the track catalog without sampling the playback timeline. */
    fun refreshTrackCatalog() = Unit
    fun currentTrackCatalog(): VesperTrackCatalog
    fun currentTrackSelection(): VesperTrackSelectionSnapshot
    /**
     * Returns the subtitle choice accepted by Media3's track-selection
     * parameters. This is distinct from [currentTrackSelection], whose
     * subtitle value represents only a renderer-active track.
     */
    fun currentAppliedSubtitleSelection(): VesperTrackSelection = currentTrackSelection().subtitle
    /** Whether the current Media3 Tracks snapshot has one selectable TEXT target for this stable id. */
    fun isSubtitleTrackSelectable(trackId: String): Boolean =
        currentTrackCatalog().subtitleTracks.any { it.id == trackId }
    fun currentAdvertisedSubtitleTrackCount(): Int = currentTrackCatalog().subtitleTracks.size
    /**
     * Increments only when Media3 reports a track or track-parameter change.
     * Subtitle transactions use this to distinguish a confirmation callback
     * from an unrelated player update.
     */
    val trackSelectionChangeGeneration: Long
        get() = 0L
    /** Monotonic identity of the currently active Media3 player callback set. */
    val sourceCallbackGeneration: Long
        get() = 0L
    /** Monotonic identity assigned to each subtitle selection command. */
    val subtitleSelectionCommandGeneration: Long
        get() = 0L
    fun isTrackCatalogReady(): Boolean = true
    fun currentSubtitleCatalogFailure(): NativeTrackSelectionFailure? = null
    fun currentEffectiveVideoTrackId(): String?
    fun currentVideoVariantObservation(): VesperVideoVariantObservation?
    fun currentVideoLayoutInfo(): NativeVideoLayoutInfo?
    fun setOnNativeUpdateListener(listener: (() -> Unit)?)
    fun setOnVideoLayoutInfoListener(listener: ((NativeVideoLayoutInfo?) -> Unit)?) = Unit
    fun setOnSubtitleCuesListener(listener: ((List<Cue>) -> Unit)?) = Unit
    /**
     * Installs the structured track-selection failure callback. Default
     * no-op so test stubs and `MissingVesperNativeBindings` do not need to
     * override it. See `VesperNativeJniBindings.setOnTrackSelectionFailureListener`.
     */
    fun setOnTrackSelectionFailureListener(
        listener: ((NativeTrackSelectionFailure) -> Unit)?,
    ) = Unit
    fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind)
    fun detachSurface()
    fun pollSnapshot(): NativeBridgeSnapshot?
    fun sampleTimeline(): TimelineUiState? = null
    fun drainEvents(): List<NativeBridgeEvent>
    /** Returns and clears structured reports emitted by playback EventHooks. */
    fun drainPipelineEventHookReports(): VesperPipelineEventHookReportBatch =
        VesperPipelineEventHookReportBatch()
    fun play()
    fun pause()
    fun stop()
    fun seekTo(positionMs: Long)
    fun setPlaybackRate(rate: Float)
    fun setVideoTrackSelection(selection: VesperTrackSelection)
    fun setAudioTrackSelection(selection: VesperTrackSelection)
    fun setSubtitleTrackSelection(selection: VesperTrackSelection)
    fun setAbrPolicy(
        policy: VesperAbrPolicy,
        expectedCatalogRevision: Long? = null,
    )
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

    override fun prepareSourceNormalizerForPlayback(
        source: VesperPlayerSource,
        enabled: Boolean,
    ): NativeSourceNormalizerResourcePreparedOpenOutcome =
        NativeSourceNormalizerResourcePreparedOpenOutcome()

    override fun disposePreparedSourceNormalizerResource(
        prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
    ) = Unit

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
        preparedSourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
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
    override fun refreshTrackCatalog() = Unit
    override fun currentTrackCatalog(): VesperTrackCatalog = VesperTrackCatalog.Empty
    override fun currentTrackSelection(): VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
    override fun currentAdvertisedSubtitleTrackCount(): Int = 0
    override fun currentSubtitleCatalogFailure(): NativeTrackSelectionFailure? = null
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
    override fun setAbrPolicy(
        policy: VesperAbrPolicy,
        expectedCatalogRevision: Long?,
    ) = Unit
    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) = Unit
    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) = Unit
    override fun clearSystemPlayback() = Unit
}
