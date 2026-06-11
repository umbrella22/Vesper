package io.github.ikaros.vesper.player.android

import android.os.Looper
import android.util.Log

internal fun VesperNativePlayerBridge.openNativeFramePipelineAfterSystemStartup(
    source: VesperPlayerSource,
    startupDiagnostics: List<Map<String, Any?>>,
): Boolean {
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)
    nativeFramePipelinePumpScheduler.execute {
        val result =
            runCatching {
                val openStatus =
                    bindings.openNativeFramePipeline(
                        source = source,
                        sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                        nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
                        surfaceKind = surfaceKind,
                    )
                check(openStatus != null) {
                    "Android native-frame pipeline open returned no session."
                }
                openStatus to advanceNativeFramePipelineOnce()
            }
        runOnMainThread {
            applyNativeFramePipelineOpenResult(source, startupDiagnostics, result)
        }
    }
    return !isRequiredNativeFramePipelineFailureActive()
}

internal fun VesperNativePlayerBridge.applyNativeFramePipelineOpenResult(
    source: VesperPlayerSource,
    startupDiagnostics: List<Map<String, Any?>>,
    result: Result<Pair<Map<String, Any?>, Map<String, Any?>?>>,
) {
    if (isDisposed.get() || source != currentSource) {
        result.getOrNull()?.second?.nativeFramePipelineFrameHandle()?.let { handle ->
            postNativeFramePipelineRelease(handle, presented = false)
        }
        return
    }
    result
        .onSuccess { opened ->
            nativeFramePipelineOpenStatus = opened.first
            Log.i(
                NATIVE_PLAYER_BRIDGE_TAG,
                "native-frame pipeline opened route=${nativeFramePipelineOpenStatus?.get("route")} " +
                    "presenter=${nativeFramePipelineOpenStatus?.get("presenterProfile")} " +
                    "surfaceAttached=${nativeFramePipelineOpenStatus?.get("surfaceAttached")}",
            )
            nativeFramePipelineLastStatus = opened.second
            Log.i(
                NATIVE_PLAYER_BRIDGE_TAG,
                "native-frame pipeline first advance status=${nativeFramePipelineLastStatus?.get("status")} " +
                    "message=${nativeFramePipelineLastStatus?.get("message")}",
            )
            publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
            currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)
            syncNativeFramePipelinePumpWithPlaybackState()
        }
        .onFailure { error ->
            handleNativeFramePipelineOpenFailure(source, startupDiagnostics, error)
        }
}

internal fun VesperNativePlayerBridge.handleNativeFramePipelineOpenFailure(
    source: VesperPlayerSource,
    startupDiagnostics: List<Map<String, Any?>>,
    error: Throwable,
) {
    val reason =
        error.message
            ?.takeUnless(String::isBlank)
            ?: "Android native-frame pipeline open failed."
    stopNativeFramePipelinePump()
    nativeFramePipelineOpenStatus = null
    nativeFramePipelineLastStatus = null
    resetNativeFramePipelineRuntimeMarkers()
    nativeFramePipelineFallbackReason = reason
    nativeFramePipelineRequiredFailure =
        nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
    closeNativeFramePipelineOnRuntime()
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)

    if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
        recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to reason))
        failRequiredNativeFramePipeline(reason, source)
        return
    }

    recordBenchmark("native_frame_pipeline_fallback", mapOf("reason" to reason))
    Log.i(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline open failed; continuing system playback: $reason")
}

internal fun VesperNativePlayerBridge.seekBindingsTo(positionMs: Long): Boolean {
    val shouldResumeNativeFramePump =
        nativeFramePipelinePumpRunning || _uiState.value.playbackState == PlaybackStateUi.Playing
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    if (isRequiredNativeFramePipelineFailureActive()) {
        return false
    }
    bindings.seekTo(positionMs)
    flushNativeFramePipeline()
    if (isRequiredNativeFramePipelineFailureActive()) {
        return false
    }
    if (nativeFramePipelineOpenStatus == null) {
        return true
    }
    seekNativeFramePipelineOnRuntime(positionMs)
    if (isRequiredNativeFramePipelineFailureActive()) {
        return false
    }
    if (shouldResumeNativeFramePump) {
        startNativeFramePipelinePump("seek")
    }
    return true
}

internal fun VesperNativePlayerBridge.flushNativeFramePipeline() {
    if (nativeFramePipelineOpenStatus == null) {
        return
    }
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    flushNativeFramePipelineOnRuntime()
}

internal fun VesperNativePlayerBridge.restartNativeFramePipelineFromBeginning() {
    if (nativeFramePipelineOpenStatus == null) {
        return
    }
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    seekNativeFramePipelineOnRuntime(0L)
}

internal fun VesperNativePlayerBridge.isRequiredNativeFramePipelineFailureActive(): Boolean =
    nativeFramePipelineRequiredFailure && nativeFramePipelineFallbackReason != null

internal fun VesperNativePlayerBridge.syncNativeFramePipelineSurfaceDiagnostics() {
    if (nativeFramePipelineOpenStatus == null) {
        return
    }
    currentNativeFramePipelineStatusOnRuntime()
}

internal fun VesperNativePlayerBridge.advanceNativeFramePipelineOnce(): Map<String, Any?>? {
    val status = bindings.advanceNativeFramePipeline() ?: return null
    val frameHandle = status.nativeFramePipelineFrameHandle()
    if (status["status"] == "frame" && frameHandle != null) {
        if (status["requiresHostRelease"].toBooleanOrFalse()) {
            return status
        }
        // Presented release-to-surface frames are released by the Rust pipeline and report
        // `presented`. Any raw frame handle reaching Kotlin is not presented by this bridge.
        return bindings.releaseNativeFramePipelineFrame(frameHandle, presented = false)
            ?: status
    }
    return status
}

internal fun VesperNativePlayerBridge.flushNativeFramePipelineOnRuntime() {
    postNativeFramePipelineCommand("flush") {
        releasePendingTimedNativeFrameFromRuntime(presented = false)
        bindings.flushNativeFramePipeline()
    }
}

internal fun VesperNativePlayerBridge.postNativeFramePipelineCommand(
    operation: String,
    command: () -> Map<String, Any?>?,
) {
    nativeFramePipelinePumpScheduler.execute {
        val result = runCatching(command)
        runOnMainThread {
            applyNativeFramePipelineCommandResult(operation, result)
        }
    }
}

internal fun VesperNativePlayerBridge.seekNativeFramePipelineOnRuntime(positionMs: Long) {
    postNativeFramePipelineCommand("seek") {
        releasePendingTimedNativeFrameFromRuntime(presented = false)
        bindings.seekNativeFramePipeline(positionMs)
    }
}

internal fun VesperNativePlayerBridge.currentNativeFramePipelineStatusOnRuntime() {
    postNativeFramePipelineCommand("status") {
        bindings.currentNativeFramePipelineStatus()
    }
}

internal fun VesperNativePlayerBridge.closeNativeFramePipelineOnRuntime() {
    nativeFramePipelinePumpScheduler.execute {
        runCatching {
            releasePendingTimedNativeFrameFromRuntime(presented = false)
            bindings.closeNativeFramePipeline()
        }.onFailure { error ->
            runOnMainThread {
                Log.w(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline close failed", error)
            }
        }
    }
}

internal fun VesperNativePlayerBridge.applyNativeFramePipelineCommandResult(
    operation: String,
    result: Result<Map<String, Any?>?>,
) {
    if (isDisposed.get()) {
        return
    }
    result
        .onSuccess { status ->
            nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
            markNativeFramePipelineDiagnosticsDirty()
        }
        .onFailure { handleNativeFramePipelineRuntimeFailure(operation, it) }
}

internal fun VesperNativePlayerBridge.handleNativeFramePipelineRuntimeFailure(operation: String, error: Throwable) {
    val reason =
        error.message
            ?.takeUnless(String::isBlank)
            ?: "Android native-frame pipeline $operation failed."
    if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
        Log.w(NATIVE_PLAYER_BRIDGE_TAG, "required native-frame pipeline $operation failed; stopping playback", error)
    } else {
        Log.w(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline $operation failed; falling back to system playback", error)
    }
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    nativeFramePipelineFallbackReason = reason
    nativeFramePipelineRequiredFailure =
        nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
    nativeFramePipelineOpenStatus = null
    nativeFramePipelineLastStatus = null
    resetNativeFramePipelineRuntimeMarkers()
    markNativeFramePipelineDiagnosticsDirty()
    val shouldFailRequiredNativeFrame =
        nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
    runOnMainThread {
        if (isDisposed.get()) {
            return@runOnMainThread
        }
        closeNativeFramePipelineOnRuntime()
        if (shouldFailRequiredNativeFrame) {
            recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to reason))
            failRequiredNativeFramePipeline(reason, currentSource)
        }
    }
}

internal fun VesperNativePlayerBridge.runOnMainThread(action: () -> Unit) {
    if (
        nativeFramePipelinePumpScheduler.inlineCallbacksForTests ||
            Looper.myLooper() == Looper.getMainLooper()
    ) {
        action()
    } else {
        mainHandler.post(action)
    }
}

internal fun VesperNativePlayerBridge.failRequiredNativeFramePipeline(reason: String, source: VesperPlayerSource?) {
    hasInitializedSource = false
    pendingAutoPlay = false
    nativeFramePipelinePlaybackRequested = false
    runCatching { bindings.clearSystemPlayback() }
    clearTrackState()
    surfaceHost.reattachIfAvailable()
    updateState {
        copy(
            subtitle = i18n.stubError(reason),
            sourceLabel = source?.label ?: sourceLabel,
            playbackState = PlaybackStateUi.Ready,
            isBuffering = false,
        )
    }
}
