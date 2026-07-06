package io.github.ikaros.vesper.player.android

import android.os.Looper
import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

private data class NativeSourceLoadPreparation(
    val pluginDiagnostics: List<Map<String, Any?>>,
    val sourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
)

internal fun VesperNativePlayerBridge.initializeNativeBridge() {
    if (isDisposed.get()) {
        return
    }
    sourceLoadScope.launchSourceLoad { initializeNativeBridgeAsync() }
}

internal suspend fun VesperNativePlayerBridge.initializeNativeBridgeAsync() {
    if (isDisposed.get()) {
        return
    }
    val epoch = sourceLoadEpoch.incrementAndGet()
    val source = runOnMainForSourceLoad { currentSource }
    if (source == null) {
        runOnMainForSourceLoad { handleInitializeWithoutSource() }
        return
    }
    val nativeFrameDecision =
        runOnMainForSourceLoad {
            prepareSourceLoadOnMain(source)
        } ?: return
    if (nativeFrameDecision is NativeFramePipelineRoute.Fail) {
        return
    }
    val preparation =
        try {
            withContext(sourceLoadDispatcher) {
                NativeSourceLoadPreparation(
                    pluginDiagnostics = probeMobilePluginsForSource(source),
                    sourceNormalizer =
                        bindings.prepareSourceNormalizerForPlayback(
                            source,
                            enabled = nativeFrameDecision != NativeFramePipelineRoute.SdkManaged,
                        ),
                )
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
            }
            throw error
        }
    if (!isCurrentSourceLoad(epoch)) {
        bindings.disposePreparedSourceNormalizerResource(preparation.sourceNormalizer)
        return
    }
    runOnMainForSourceLoad {
        if (!isCurrentSourceLoad(epoch)) {
            bindings.disposePreparedSourceNormalizerResource(preparation.sourceNormalizer)
            return@runOnMainForSourceLoad
        }
        applyPreparedSourceLoadOnMain(source, nativeFrameDecision, preparation)
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
    source: VesperPlayerSource,
): NativeFramePipelineRoute? {
    if (isDisposed.get()) {
        return null
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
    source: VesperPlayerSource,
    nativeFrameDecision: NativeFramePipelineRoute,
    preparation: NativeSourceLoadPreparation,
) {
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(preparation.pluginDiagnostics)
    advanceNativeUpdateEpoch()
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
    surfaceHost.detach()
    bindings.dispose()
    sourceLoadDispatcher.close()
    recordBenchmark("dispose_command")
    benchmarkRecorder.dispose()
}

internal fun VesperNativePlayerBridge.refreshNativeBridge() {
    if (isDisposed.get()) {
        return
    }
    bindings.refreshSnapshot()
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.selectNativeSource(source: VesperPlayerSource) {
    if (isDisposed.get()) {
        return
    }
    sourceLoadScope.launchSourceLoad { selectNativeSourceAsync(source) }
}

internal suspend fun VesperNativePlayerBridge.selectNativeSourceAsync(source: VesperPlayerSource) {
    if (isDisposed.get()) {
        return
    }
    runOnMainForSourceLoad {
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
        clearTrackState()
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
    }
    initializeNativeBridgeAsync()
}

internal fun CoroutineScope.launchSourceLoad(block: suspend () -> Unit) {
    launch {
        runCatching { block() }
            .onFailure { error ->
                Log.e(NATIVE_PLAYER_BRIDGE_TAG, "source load failed", error)
            }
    }
}

private fun VesperNativePlayerBridge.isCurrentSourceLoad(epoch: Long): Boolean =
    !isDisposed.get() && sourceLoadEpoch.get() == epoch

internal suspend fun <T> VesperNativePlayerBridge.runOnMainForSourceLoad(block: () -> T): T {
    if (Looper.myLooper() == Looper.getMainLooper()) {
        return block()
    }
    return suspendCancellableCoroutine { continuation ->
        val runnable =
            Runnable {
                if (!continuation.isActive) {
                    return@Runnable
                }
                runCatching(block)
                    .onSuccess { value -> continuation.resume(value) }
                    .onFailure { error -> continuation.resumeWithException(error) }
            }
        if (!mainHandler.post(runnable)) {
            runnable.run()
        }
    }
}

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
