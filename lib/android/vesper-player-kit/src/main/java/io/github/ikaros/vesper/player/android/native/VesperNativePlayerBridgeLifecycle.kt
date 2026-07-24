package io.github.ikaros.vesper.player.android

import android.os.Looper
import android.os.Trace
import android.util.Log
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CountDownLatch
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExecutorCoroutineDispatcher
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

private const val SOURCE_LOAD_CLEANUP_QUEUE_CAPACITY = 64
private const val MAIN_THREAD_BRIDGE_TIMEOUT_MS = 3_000L
private const val MAIN_THREAD_OPERATION_PENDING = 0
private const val MAIN_THREAD_OPERATION_RUNNING = 1
private const val MAIN_THREAD_OPERATION_CANCELLED = 2
private const val MAIN_THREAD_OPERATION_COMPLETED = 3

/** Result of handing a bounded owner-thread mutation to the main looper. */
internal enum class MainThreadRunResult {
    Completed,
    Cancelled,
    Started,
}

private object VesperSourceLoadCleanupDispatcher {
    private val threadIndex = AtomicInteger(0)

    val dispatcher: ExecutorCoroutineDispatcher =
        ThreadPoolExecutor(
            1,
            1,
            0L,
            TimeUnit.MILLISECONDS,
            ArrayBlockingQueue(SOURCE_LOAD_CLEANUP_QUEUE_CAPACITY),
            { runnable ->
                Thread(
                    runnable,
                    "vesper-source-load-cleanup-${threadIndex.incrementAndGet()}",
                ).apply {
                    isDaemon = true
                }
            },
            ThreadPoolExecutor.AbortPolicy(),
        ).asCoroutineDispatcher()
}

private data class NativeSourceLoadPreparation(
    val pluginDiagnostics: List<Map<String, Any?>>,
    val sourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
)

internal fun VesperNativePlayerBridge.initializeNativeBridge() {
    if (runOnMainSynchronously("initialize") {
            if (!isDisposed.get()) {
                launchSourceLoad { initializeNativeBridgeAsync() }
            }
        }
        == MainThreadRunResult.Cancelled
    ) {
        throw mainThreadBridgeTimeout("initialize")
    }
}

internal suspend fun VesperNativePlayerBridge.initializeNativeBridgeAsync(
    preservedConfirmedSubtitleSelection: VesperTrackSelection? = null,
) {
    if (isDisposed.get()) {
        return
    }
    val epoch = sourceLoadEpoch.incrementAndGet()
    val source =
        runOnMainForSourceLoad {
            if (isCurrentSourceLoad(epoch)) currentSource else null
        }
    if (source == null) {
        runOnMainForSourceLoad {
            if (isCurrentSourceLoad(epoch) && currentSource == null) {
                handleInitializeWithoutSource()
            }
        }
        return
    }
    val nativeFrameDecision =
        runOnMainForSourceLoad {
            prepareSourceLoadOnMain(
                epoch,
                source,
                preservedConfirmedSubtitleSelection,
            )
        } ?: return
    if (nativeFrameDecision is NativeFramePipelineRoute.Fail) {
        return
    }
    val preparation: NativeSourceLoadPreparation =
        try {
            withContext(sourceLoadDispatcher) {
                val preparation =
                    NativeSourceLoadPreparation(
                        pluginDiagnostics = probeMobilePluginsForSource(source),
                        sourceNormalizer =
                            bindings.prepareSourceNormalizerForPlayback(
                                source,
                                enabled = nativeFrameDecision != NativeFramePipelineRoute.SdkManaged,
                            ),
                    )
                val backgroundContext = currentCoroutineContext()
                if (!isCurrentSourceLoad(epoch) || !backgroundContext.isActive) {
                    bindings.disposePreparedSourceNormalizerResource(preparation.sourceNormalizer)
                    backgroundContext.ensureActive()
                    return@withContext null
                }
                preparation
            }
        } catch (error: Throwable) {
            if (error is CancellationException) {
                throw error
            }
            if (isCurrentSourceLoad(epoch)) {
                runOnMainForSourceLoad {
                    if (isCurrentSourceLoad(epoch)) {
                        handleInitializeFailureOnMain(source, error)
                    }
                }
            } else {
                Log.i(
                    NATIVE_PLAYER_BRIDGE_TAG,
                    "ignored stale source load failure for source=${source.uri}",
                )
                return
            }
            throw error
        } ?: return
    if (!isCurrentSourceLoad(epoch)) {
        disposePreparedSourceNormalizerOnBackground(preparation.sourceNormalizer)
        return
    }
    val applied =
        runOnMainForSourceLoad {
            if (!isCurrentSourceLoad(epoch)) {
                return@runOnMainForSourceLoad false
            }
            applyPreparedSourceLoadOnMain(epoch, source, nativeFrameDecision, preparation)
            true
        }
    if (!applied) {
        disposePreparedSourceNormalizerOnBackground(preparation.sourceNormalizer)
    }
}

private suspend fun VesperNativePlayerBridge.disposePreparedSourceNormalizerOnBackground(
    prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
) {
    val disposedOnSourceLoadDispatcher =
        runCatching {
            withContext(NonCancellable + sourceLoadDispatcher) {
                bindings.disposePreparedSourceNormalizerResource(prepared)
            }
        }.isSuccess
    if (disposedOnSourceLoadDispatcher) {
        return
    }
    val disposedOnCleanupDispatcher =
        runCatching {
            withContext(NonCancellable + VesperSourceLoadCleanupDispatcher.dispatcher) {
                bindings.disposePreparedSourceNormalizerResource(prepared)
            }
        }.isSuccess
    if (disposedOnCleanupDispatcher) {
        return
    }
    if (Looper.myLooper() == Looper.getMainLooper()) {
        Log.w(
            NATIVE_PLAYER_BRIDGE_TAG,
            "source load cleanup queues rejected stale SourceNormalizer disposal; running final fallback on the main thread",
        )
    }
    runCatching {
        bindings.disposePreparedSourceNormalizerResource(prepared)
    }.onFailure { error ->
        Log.w(
            NATIVE_PLAYER_BRIDGE_TAG,
            "failed to dispose stale SourceNormalizer resource on fallback path",
            error,
        )
    }
}

private fun VesperNativePlayerBridge.handleInitializeWithoutSource() {
    recordBenchmark("initialize_start")
    recordBenchmark("initialize_without_source")
    clearTrackState()
    updateState {
        copy(
            subtitle = i18n.selectSourcePrompt(),
            sourceLabel = i18n.noSourceSelected(),
            playbackState = PlaybackStateUi.Ready,
            isBuffering = false,
            lastError = null,
        )
    }
}

private fun VesperNativePlayerBridge.prepareSourceLoadOnMain(
    epoch: Long,
    source: VesperPlayerSource,
    preservedConfirmedSubtitleSelection: VesperTrackSelection? = null,
): NativeFramePipelineRoute? {
    if (!isCurrentSourceLoad(epoch) || currentSource != source) {
        return null
    }
    // Invalidate the current Media3 item before background preparation. No
    // listener callback or subtitle command may observe the old item while a
    // replacement source/item is being prepared.
    bindings.invalidateSystemPlaybackCallbacks()
    clearTrackState()
    _subtitleState.value =
        VesperSubtitleState.loading(
            advertisedTrackCount = source.externalSubtitles.size,
        )
    preservedConfirmedSubtitleSelection?.let { confirmedSelection ->
        _confirmedSubtitleSelection.value = confirmedSelection
        // Keep native refreshes from publishing a replacement item's default
        // subtitle before the restore transaction confirms the preserved
        // selection. `applySubtitleSelectionTransaction` will retain the same
        // mode (or clear it when the restore fails/source changes).
        subtitleSelectionCoordinatorMode = confirmedSelection.mode
        _trackSelection.value = _trackSelection.value.copy(
            confirmedSubtitle = confirmedSelection,
            effectiveSubtitleTrackId = null,
        )
    }
    recordBenchmark("initialize_start")
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(emptyList())
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
        NATIVE_PLAYER_BRIDGE_TAG,
        "native-frame route decision=${nativeFrameRouteLogLabel(nativeFrameDecision)} " +
            "mode=${nativeFramePipelineConfiguration.mode} surface=$surfaceKind " +
            "sourceNormalizerPlugins=${sourceNormalizerConfiguration.pluginLibraryPaths.size} " +
            "decoderPlugins=${nativeFramePipelineConfiguration.decoderPluginLibraryPaths.size} " +
            "frameProcessors=${nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths.size}",
    )
    source.androidDrmPhase0Failure(nativeFrameDecision)?.let { failure ->
        recordBenchmark(
            "initialize_failed",
            mapOf("error" to failure.message.orEmpty()),
        )
        hasInitializedSource = false
        pendingAutoPlay = false
        clearTrackState()
        val terminalError = failure.toPlayerErrorState()
        updateState {
            copy(
                subtitle = i18n.stubError(failure.message ?: drmUnsupportedRouteMessage("systemPlayer")),
                sourceLabel = source.label,
                playbackState = PlaybackStateUi.Paused,
                isBuffering = false,
                isInterrupted = false,
                lastError = terminalError,
            )
        }
        throw failure
    }
    when (nativeFrameDecision) {
        NativeFramePipelineRoute.SystemPlayer -> Unit
        is NativeFramePipelineRoute.Fallback -> {
            Log.i(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline fallback: ${nativeFrameDecision.reason}")
        }
        is NativeFramePipelineRoute.Fail -> {
            recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to nativeFrameDecision.reason))
            hasInitializedSource = false
            pendingAutoPlay = false
            clearTrackState()
            val terminalError =
                VesperPlayerErrorState(
                    message = nativeFrameDecision.reason,
                    code = VesperPlayerErrorCode.Unsupported,
                    category = VesperPlayerErrorCategory.Capability,
                    retriable = false,
                    details =
                        mapOf(
                            "reason" to "nativeFrameRouteUnavailable",
                            "route" to "nativeFrame",
                        ),
                )
            updateState {
                copy(
                    subtitle = i18n.stubError(nativeFrameDecision.reason),
                    sourceLabel = source.label,
                    playbackState = PlaybackStateUi.Paused,
                    isBuffering = false,
                    isInterrupted = false,
                    lastError = terminalError,
                )
            }
            return nativeFrameDecision
        }
        NativeFramePipelineRoute.SdkManaged -> {
            recordBenchmark("native_frame_pipeline_selected")
        }
    }
    return nativeFrameDecision
}

private fun VesperNativePlayerBridge.applyPreparedSourceLoadOnMain(
    epoch: Long,
    source: VesperPlayerSource,
    nativeFrameDecision: NativeFramePipelineRoute,
    preparation: NativeSourceLoadPreparation,
) {
    if (!isCurrentSourceLoad(epoch) || currentSource != source) {
        return
    }
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(preparation.pluginDiagnostics)
    runCatching {
        bindings.initialize(
            source,
            currentResiliencePolicy,
            trackPreferencePolicy,
            systemPlaybackUsesSourceNormalizerResource =
                nativeFrameDecision != NativeFramePipelineRoute.SdkManaged,
            systemPlaybackVideoEnabled =
                nativeFrameDecision != NativeFramePipelineRoute.SdkManaged,
            preparedSourceNormalizer = preparation.sourceNormalizer,
        )
    }
        .onSuccess {
            if (nativeFrameDecision == NativeFramePipelineRoute.SdkManaged &&
                !openNativeFramePipelineAfterSystemStartup(epoch, source, it.pluginDiagnostics)
            ) {
                return@onSuccess
            }
            if (it.pluginDiagnostics.isNotEmpty() || !nativeFramePipelineConfiguration.isDisabled) {
                currentPluginDiagnostics =
                    pluginDiagnosticsWithNativeFramePipeline(it.pluginDiagnostics)
            }
            recordBenchmark("initialize_completed")
            hasInitializedSource = true
            activeNativeItemEpoch = nativeUpdateEpoch
            installNativeUpdateListener()
            Log.i(
                NATIVE_PLAYER_BRIDGE_TAG,
                "initialized source=${source.uri} label=${source.label} kind=${source.kind} protocol=${source.protocol} decoderBackend=$decoderBackend",
            )
            surfaceHost.reattachIfAvailable()
            val shouldAutoPlay = pendingAutoPlay
            pendingAutoPlay = false
            if (shouldAutoPlay) {
                Log.i(NATIVE_PLAYER_BRIDGE_TAG, "auto-playing selected source=${source.uri}")
                bindings.play()
                nativeFramePipelinePlaybackRequested = true
                updateState {
                    copy(
                        playbackState = PlaybackStateUi.Playing,
                        isBuffering = false,
                        lastError = null,
                    )
                }
                startNativeFramePipelinePump("autoplay")
            }
            updateState {
                copy(
                    subtitle = it.subtitle ?: sourceSubtitle(source),
                    sourceLabel = source.label,
                    lastError = null,
                )
            }
            refreshFromNative()
        }
        .onFailure {
            handleInitializeFailureOnMain(source, it)
        }
        .getOrThrow()
}

private fun VesperNativePlayerBridge.handleInitializeFailureOnMain(
    source: VesperPlayerSource,
    error: Throwable,
) {
    // Dispose any partially-initialized native session to prevent a permanent
    // resource leak when initialization fails after the session handle has been
    // assigned (AGENTS.md rule). This also preserves the previous failure
    // behavior when background source-normalizer preparation fails before the
    // main-thread apply step starts.
    runCatching { bindings.dispose() }
    recordBenchmark(
        "initialize_failed",
        mapOf("error" to (error.message ?: error::class.java.simpleName)),
    )
    hasInitializedSource = false
    pendingAutoPlay = false
    clearTrackState()
    Log.e(NATIVE_PLAYER_BRIDGE_TAG, "failed to initialize source=${source.uri}", error)
    val message = error.message?.takeUnless(String::isBlank) ?: i18n.nativeBindingsUnavailable()
    val terminalError = error.toInitializePlayerErrorState(message)
    updateState {
        copy(
            subtitle = i18n.stubError(message),
            sourceLabel = source.label,
            playbackState = PlaybackStateUi.Paused,
            isBuffering = false,
            isInterrupted = false,
            lastError = terminalError,
        )
    }
}

internal fun VesperPlayerSource.androidDrmPhase0Failure(
    nativeFrameDecision: NativeFramePipelineRoute,
): VesperPlayerUnsupportedOperation? {
    drmConfiguration ?: return null
    val route =
        when (nativeFrameDecision) {
            NativeFramePipelineRoute.SdkManaged,
            is NativeFramePipelineRoute.Fail -> "nativeFrame"
            NativeFramePipelineRoute.SystemPlayer,
            is NativeFramePipelineRoute.Fallback -> "direct"
        }
    val reason =
        when {
            route == "nativeFrame" -> "drmUnsupportedRoute"
            !drmConfiguration.keySystem.equals("widevine", ignoreCase = true) -> "drmUnsupportedKeySystem"
            else -> return null
        }
    return VesperPlayerUnsupportedOperation(
        drmUnsupportedRouteMessage(route),
        drmUnsupportedRouteDetails(this, route = route, reason = reason),
    )
}

internal fun VesperNativePlayerBridge.disposeNativeBridge() {
    if (!isDisposed.compareAndSet(false, true)) {
        return
    }
    sourceLoadEpoch.incrementAndGet()
    sourceLoadRequestGeneration += 1L
    val completed = runOnMainSynchronously("dispose") {
        disposeNativeBridgeOnMain()
    }
    if (completed == MainThreadRunResult.Cancelled) {
        // The bridge is already atomically closed to new work. Keep cleanup
        // queued on the owner looper instead of touching Media3 or Views from
        // the caller thread after the bounded synchronous wait expires.
        mainHandler.post { disposeNativeBridgeOnMain() }
    }
}

private fun VesperNativePlayerBridge.disposeNativeBridgeOnMain() {
    if (!disposeCleanupStarted.compareAndSet(false, true)) return
    cancelPendingSubtitleSelectionForDispose()
    sourceLoadEpoch.incrementAndGet()
    advanceNativeUpdateEpoch(clearListener = true)
    hasInitializedSource = false
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    closeNativeFramePipelineOnRuntime()
    nativeFramePipelinePumpScheduler.close()
    nativeFramePipelinePumpScheduler.quitLooperSafely()
    clearTrackState()
    nativeFramePipelineOpenStatus = null
    nativeFramePipelineLastStatus = null
    clearPendingTimedNativeFrameFromRuntime()
    nativeFramePipelinePlaybackRequested = false
    resetNativeFramePipelineRuntimeMarkers()
    bindings.clearSystemPlayback()
    surfaceHost.setKeepScreenOn(false)
    surfaceHost.close()
    surfaceHost.detach()
    bindings.dispose()
    sourceLoadJob?.cancel()
    sourceLoadScope.cancel()
    sourceLoadDispatcher.close()
    recordBenchmark("dispose_command")
    benchmarkRecorder.dispose()
}

internal fun VesperNativePlayerBridge.refreshNativeBridge() {
    if (runOnMainSynchronously("refresh") {
            if (isDisposed.get()) {
                return@runOnMainSynchronously
            }
            Trace.beginSection("VesperRefresh#refreshSnapshot")
            try {
                bindings.refreshSnapshot()
            } finally {
                Trace.endSection()
            }
            Trace.beginSection("VesperRefresh#refreshFromNative")
            try {
                refreshFromNative()
            } finally {
                Trace.endSection()
            }
        }
        == MainThreadRunResult.Cancelled
    ) {
        throw mainThreadBridgeTimeout("refresh")
    }
}

internal fun VesperNativePlayerBridge.sampleTimelineNativeBridge(): TimelineUiState? {
    var sampledTimeline: TimelineUiState? = null
    if (runOnMainSynchronously("sampleTimeline") {
            if (isDisposed.get() ||
                isRequiredNativeFramePipelineFailureActive() ||
                !hasInitializedSource ||
                activeNativeItemEpoch != nativeUpdateEpoch
            ) {
                return@runOnMainSynchronously
            }
            Trace.beginSection("VesperRefresh#sampleTimeline")
            try {
                sampledTimeline = bindings.sampleTimeline()
            } finally {
                Trace.endSection()
            }
        }
        == MainThreadRunResult.Cancelled
    ) {
        throw mainThreadBridgeTimeout("sampleTimeline")
    }
    return sampledTimeline
}

internal fun VesperNativePlayerBridge.selectNativeSource(source: VesperPlayerSource) {
    if (isDisposed.get()) {
        return
    }
    val completed = runOnMainSynchronously("selectSource") {
        if (beginNativeSourceSelectionOnMain(source)) {
            launchSourceLoad { initializeNativeBridgeAsync() }
        }
    }
    if (completed == MainThreadRunResult.Cancelled) {
        throw mainThreadBridgeTimeout("selectSource")
    }
}

internal suspend fun VesperNativePlayerBridge.selectNativeSourceAsync(source: VesperPlayerSource) {
    if (isDisposed.get()) {
        return
    }
    val started = runOnMainForSourceLoad { beginNativeSourceSelectionOnMain(source) }
    if (!started || isDisposed.get()) return
    initializeNativeBridgeAsync()
}

private fun VesperNativePlayerBridge.beginNativeSourceSelectionOnMain(
    source: VesperPlayerSource,
): Boolean {
    if (isDisposed.get()) return false
    sourceLoadEpoch.incrementAndGet()
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
    // Fence the previous Media3 item and cancel its pending subtitle command
    // before either source API returns.
    bindings.invalidateSystemPlaybackCallbacks()
    clearTrackState()
    synchronized(runtimeWarnings) { runtimeWarnings.clear() }
    _subtitleState.value =
        VesperSubtitleState.loading(advertisedTrackCount = source.externalSubtitles.size)
    Log.i(
        NATIVE_PLAYER_BRIDGE_TAG,
        "selecting source=${source.uri} label=${source.label} kind=${source.kind} protocol=${source.protocol}",
    )
    updateState {
        copy(
            subtitle = i18n.openingSource(source.label),
            sourceLabel = source.label,
            playbackState = PlaybackStateUi.Ready,
            isBuffering = true,
            timeline = timeline.copy(positionMs = 0L),
            lastError = null,
        )
    }
    return true
}

internal fun VesperNativePlayerBridge.launchSourceLoad(block: suspend () -> Unit) {
    val requestGeneration = ++sourceLoadRequestGeneration
    sourceLoadJob?.cancel()
    sourceLoadJob =
        try {
            sourceLoadScope.launch {
                runCatching { block() }
                    .onFailure { error ->
                        if (error is CancellationException) {
                            return@onFailure
                        }
                        Log.e(NATIVE_PLAYER_BRIDGE_TAG, "source load failed", error)
                    }
            }
        } catch (error: RejectedExecutionException) {
            Log.w(NATIVE_PLAYER_BRIDGE_TAG, "source load queue rejected work", error)
            // A rejected latest load must not leave the source permanently in
            // the loading state. Surface a bounded, structured failure on the
            // owner thread so callers can retry or choose another source.
            val source = currentSource
            if (!isDisposed.get() && source != null) {
                mainHandler.post {
                    if (!isDisposed.get() &&
                        currentSource == source &&
                        sourceLoadRequestGeneration == requestGeneration
                    ) {
                        val failure =
                            VesperPlayerUnsupportedOperation(
                                "the Android source-load queue is full",
                                mapOf(
                                    "domain" to "source",
                                    "code" to "source_load_queue_full",
                                    "phase" to "source",
                                    "reason" to "sourceLoadQueueFull",
                                    "operation" to "initialize",
                                    "retriable" to true,
                                ),
                            )
                        if (hasInitializedSource) {
                            // A refresh of an already healthy item must not
                            // tear down playback merely because a superseded
                            // background preparation was rejected.
                            updateState {
                                copy(
                                    lastError = failure.toPlayerErrorState(),
                                    isBuffering = false,
                                )
                            }
                        } else {
                            handleInitializeFailureOnMain(source, failure)
                        }
                    }
                }
            }
            null
        }
}

/**
 * Returns `true` when the bridge is still alive and the epoch still matches the
 * load that captured it.
 *
 * `sourceLoadEpoch` is a wrapping `AtomicLong` (`incrementAndGet()`), so on a
 * theoretical 2^63-wrap it could revisit an old value. This helper only checks
 * the epoch, so callers that gate a continuation on a specific source MUST also
 * re-check `currentSource == source` (or `source != currentSource` to bail).
 * That source-identity clause is what makes the predicate behave as a
 * never-reuse token, because a new load always reassigns `currentSource` before
 * bumping the epoch. Do not rely on this helper alone for source-sensitive
 * decisions.
 */
internal fun VesperNativePlayerBridge.isCurrentSourceLoad(epoch: Long): Boolean =
    !isDisposed.get() && sourceLoadEpoch.get() == epoch

internal suspend fun <T> VesperNativePlayerBridge.runOnMainForSourceLoad(block: () -> T): T {
    if (Looper.myLooper() == Looper.getMainLooper()) {
        return block()
    }
    return suspendCancellableCoroutine { continuation ->
        val gate = Any()
        val runnable =
            Runnable {
                synchronized(gate) {
                    if (!continuation.isActive) {
                        return@Runnable
                    }
                    runCatching(block)
                        .onSuccess { value -> continuation.resume(value) }
                        .onFailure { error -> continuation.resumeWithException(error) }
                }
            }
        continuation.invokeOnCancellation {
            // Do not leave cancelled source-load continuations queued on the
            // main looper. Rapid source refreshes otherwise accumulate no-op
            // callbacks even though the source-load dispatcher is bounded.
            synchronized(gate) {
                mainHandler.removeCallbacks(runnable)
            }
        }
        if (!mainHandler.post(runnable)) {
            continuation.resumeWithException(
                IllegalStateException("the Android main looper rejected a player lifecycle command"),
            )
        }
    }
}

/**
 * Runs a short owner-thread mutation with a bounded wait for synchronous APIs.
 * A timeout leaves the mutation unapplied and reports failure to the caller;
 * it never falls back to touching Media3 or Android Views off-main.
 */
internal fun VesperNativePlayerBridge.runOnMainSynchronously(
    operation: String,
    block: () -> Unit,
): MainThreadRunResult {
    val mainLooper = Looper.getMainLooper()
    if (mainLooper == null || Looper.myLooper() == mainLooper) {
        block()
        return MainThreadRunResult.Completed
    }
    val completion = CountDownLatch(1)
    val failure = AtomicReference<Throwable?>()
    // The timeout and the main-looper runnable claim the operation through a
    // single CAS. This closes the removeCallbacks race where a runnable that
    // had already started could mutate player state after the caller reported
    // a timeout.
    val operationState = AtomicInteger(MAIN_THREAD_OPERATION_PENDING)
    val runnable = Runnable {
        if (!operationState.compareAndSet(
                MAIN_THREAD_OPERATION_PENDING,
                MAIN_THREAD_OPERATION_RUNNING,
            )
        ) {
            completion.countDown()
            return@Runnable
        }
        try {
            block()
        } catch (error: Throwable) {
            failure.set(error)
            Log.e(NATIVE_PLAYER_BRIDGE_TAG, "$operation failed on the main looper", error)
        } finally {
            operationState.set(MAIN_THREAD_OPERATION_COMPLETED)
            completion.countDown()
        }
    }
    if (!mainHandler.post(runnable)) {
        Log.e(NATIVE_PLAYER_BRIDGE_TAG, "$operation rejected by the main looper")
        return MainThreadRunResult.Cancelled
    }
    val completed = completion.await(MAIN_THREAD_BRIDGE_TIMEOUT_MS, TimeUnit.MILLISECONDS)
    if (!completed) {
        if (operationState.compareAndSet(
                MAIN_THREAD_OPERATION_PENDING,
                MAIN_THREAD_OPERATION_CANCELLED,
            )
        ) {
            mainHandler.removeCallbacks(runnable)
            Log.e(
                NATIVE_PLAYER_BRIDGE_TAG,
                "$operation did not reach the main looper within ${MAIN_THREAD_BRIDGE_TIMEOUT_MS}ms",
            )
            return MainThreadRunResult.Cancelled
        }
        // The owner thread claimed the operation before the timeout. Do not
        // report failure while its mutation is still in flight; doing so would
        // let callers start a replacement operation against partially updated
        // state. The owner thread owns completion and records any exception.
        Log.w(
            NATIVE_PLAYER_BRIDGE_TAG,
            "$operation was already running when the synchronous wait expired",
        )
        // Give an operation that has already claimed the owner thread one
        // additional bounded window to finish. This preserves exception
        // propagation for slow-but-finite lifecycle work without returning a
        // false timeout while the mutation is still in flight.
        if (completion.await(MAIN_THREAD_BRIDGE_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
            failure.get()?.let { throw it }
            return MainThreadRunResult.Completed
        }
        Log.e(
            NATIVE_PLAYER_BRIDGE_TAG,
            "$operation remains in flight after the bounded owner-thread grace window",
        )
        return MainThreadRunResult.Started
    }
    failure.get()?.let { throw it }
    return MainThreadRunResult.Completed
}

internal fun mainThreadBridgeTimeout(operation: String): IllegalStateException =
    IllegalStateException(
        "$operation did not reach the Android main thread within " +
            "${MAIN_THREAD_BRIDGE_TIMEOUT_MS}ms",
    )

private fun Throwable.toInitializePlayerErrorState(message: String): VesperPlayerErrorState =
    when (this) {
        is VesperPlayerUnsupportedOperation -> toPlayerErrorState()
        else ->
            VesperPlayerErrorState(
                message = message,
                code = VesperPlayerErrorCode.BackendFailure,
                category = VesperPlayerErrorCategory.Platform,
                retriable = false,
                details =
                    mapOf(
                        "reason" to "initializeFailed",
                        "errorClass" to this::class.java.name,
                        "errorMessage" to message,
                    ),
            )
    }
