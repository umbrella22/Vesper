package io.github.umbrella22.vesper.player.android

import android.os.Looper
import android.util.Log
import kotlinx.coroutines.CompletableDeferred

internal fun VesperNativePlayerBridge.openNativeFramePipelineAfterSystemStartup(
    epoch: Long,
    source: VesperPlayerSource,
    startupDiagnostics: List<Map<String, Any?>>,
    requiredStartup: CompletableDeferred<Unit>?,
): Boolean {
    if (!isCurrentSourceLoad(epoch)) {
        requiredStartup?.cancel()
        return false
    }
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)
    nativeFramePipelineOpeningEpoch = epoch
    pendingNativeFrameStartupSeekMs = null
    fun publishOpenResult(result: Result<Pair<Map<String, Any?>, Map<String, Any?>?>>) {
        runOnMainThread {
            if (!isCurrentSourceLoad(epoch) || source != currentSource) {
                requiredStartup?.cancel()
                return@runOnMainThread
            }
            applyNativeFramePipelineOpenResult(source, startupDiagnostics, result)
            if (requiredStartup != null) {
                val error = _uiState.value.lastError
                if (isRequiredNativeFramePipelineFailureActive() && error != null) {
                    requiredStartup.completeExceptionally(VesperPlayerCommandException(error))
                } else {
                    requiredStartup.complete(Unit)
                }
            }
        }
    }
    nativeFramePipelinePumpScheduler.executeCommand(
        NativeFramePipelineRuntimeCommand(
            operation = "open",
            action = openCommand@{
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
                        if (!isCurrentSourceLoad(epoch)) {
                            return@runCatching openStatus to null
                        }
                        val firstAdvance = advanceNativeFramePipelineOnce()
                        registerAdvancedNativeFrameFromRuntime(
                            nativeFramePipelinePumpEpoch,
                            firstAdvance,
                        )
                        openStatus to firstAdvance
                    }
                if (!isCurrentSourceLoad(epoch)) {
                    discardStaleNativeFramePipelineOpenResult(result)
                    requiredStartup?.cancel()
                    return@openCommand
                }
                publishOpenResult(result)
            },
            onRejected = {
                publishOpenResult(Result.failure(IllegalStateException("Android native-frame runtime rejected open.")))
            },
        )
    )
    return !isRequiredNativeFramePipelineFailureActive()
}

private fun VesperNativePlayerBridge.applyNativeFramePipelineOpenResult(
    source: VesperPlayerSource,
    startupDiagnostics: List<Map<String, Any?>>,
    result: Result<Pair<Map<String, Any?>, Map<String, Any?>?>>,
) {
    val startupSeekMs = pendingNativeFrameStartupSeekMs
    clearNativeFrameStartupSeek()
    result
        .onSuccess { opened ->
            nativeFramePipelineOpenStatus = opened.first
            if (startupSeekMs != null) {
                flushNativeFramePipeline()
                if (nativeFramePipelineOpenStatus == null) return
                seekNativeFramePipelineOnRuntime(startupSeekMs)
                if (nativeFramePipelineOpenStatus == null) return
            }
            Log.i(
                NATIVE_PLAYER_BRIDGE_TAG,
                "native-frame pipeline opened route=${nativeFramePipelineOpenStatus?.get("route")} " +
                    "presenter=${nativeFramePipelineOpenStatus?.get("presenterProfile")} " +
                    "surfaceAttached=${nativeFramePipelineOpenStatus?.get("surfaceAttached")}",
            )
            nativeFramePipelineLastStatus = if (startupSeekMs == null) opened.second else null
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

internal fun VesperNativePlayerBridge.discardStaleNativeFramePipelineOpenResult(
    _result: Result<Pair<Map<String, Any?>, Map<String, Any?>?>>,
) {
    runCatching {
        releasePendingTimedNativeFrameFromRuntime(presented = false)
        bindings.closeNativeFramePipeline()
    }.onFailure { error ->
        Log.w(NATIVE_PLAYER_BRIDGE_TAG, "stale native-frame pipeline close failed failure=${diagnosticPlaybackFailure(error)}")
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
    closeNativeFramePipelineOnRuntime(
        restoreSystemVideoForEpoch = sourceLoadEpoch.get().takeUnless { nativeFramePipelineRequiredFailure },
    )
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(startupDiagnostics)

    if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
        recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to reason))
        failRequiredNativeFramePipeline(
            reason, source, operation = "open",
            unsupported = error is UnsupportedOperationException,
        )
        return
    }

    recordBenchmark("native_frame_pipeline_fallback", mapOf("reason" to reason))
    Log.i(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline open failed; continuing system playback: $reason")
}

internal fun VesperNativePlayerBridge.seekBindingsTo(
    positionMs: Long,
    commandId: Long = seekCommandGeneration.incrementAndGet(),
    sourceEpoch: Long = sourceCommandGeneration.get(),
): Boolean {
    val shouldResumeNativeFramePump =
        nativeFramePipelinePumpRunning || _uiState.value.playbackState == PlaybackStateUi.Playing
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    if (isRequiredNativeFramePipelineFailureActive()) {
        return false
    }
    bindings.seekTo(positionMs, commandId, sourceEpoch, seekCommandTimeoutMs)
    flushNativeFramePipeline()
    if (isRequiredNativeFramePipelineFailureActive()) {
        return false
    }
    if (nativeFramePipelineOpenStatus == null) {
        if (isNativeFramePipelineOpening() && seekCommandGeneration.get() == commandId &&
            sourceCommandGeneration.get() == sourceEpoch
        ) {
            pendingNativeFrameStartupSeekMs = positionMs
        }
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

internal fun VesperNativePlayerBridge.isNativeFramePipelineOpening(): Boolean =
    nativeFramePipelineOpeningEpoch == sourceLoadEpoch.get()

internal fun VesperNativePlayerBridge.clearNativeFrameStartupSeek() {
    nativeFramePipelineOpeningEpoch = null
    pendingNativeFrameStartupSeekMs = null
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
    postNativeFramePipelineCommand(
        operation = "flush",
        coalescingKey = "flush",
    ) {
        releasePendingTimedNativeFrameFromRuntime(presented = false)
        bindings.flushNativeFramePipeline()
    }
}

internal fun VesperNativePlayerBridge.postNativeFramePipelineCommand(
    operation: String,
    coalescingKey: String? = null,
    runsDuringClose: Boolean = false,
    replacesPendingCommands: Boolean = false,
    command: () -> Map<String, Any?>?,
) {
    val epoch = sourceLoadEpoch.get()
    nativeFramePipelinePumpScheduler.executeCommand(
        NativeFramePipelineRuntimeCommand(
            operation = operation,
            coalescingKey = coalescingKey,
            runsDuringClose = runsDuringClose,
            replacesPendingCommands = replacesPendingCommands,
            action = runtimeCommand@{
                if (!isCurrentNativeFramePipelineRuntimeCommand(epoch, runsDuringClose)) {
                    return@runtimeCommand
                }
                val result = runCatching(command)
                runOnMainThread {
                    if (!isCurrentNativeFramePipelineRuntimeCommand(epoch, runsDuringClose)) {
                        return@runOnMainThread
                    }
                    applyNativeFramePipelineCommandResult(operation, result)
                }
            },
            onRejected = {
                runOnMainThread {
                    handleNativeFramePipelineRuntimeFailure(
                        operation,
                        IllegalStateException(
                            "Android native-frame runtime command queue rejected $operation.",
                        ),
                    )
                }
            },
        )
    )
}

internal fun VesperNativePlayerBridge.seekNativeFramePipelineOnRuntime(positionMs: Long) {
    postNativeFramePipelineCommand(
        operation = "seek",
        coalescingKey = "seek",
    ) {
        releasePendingTimedNativeFrameFromRuntime(presented = false)
        bindings.seekNativeFramePipeline(positionMs)
    }
}

internal fun VesperNativePlayerBridge.currentNativeFramePipelineStatusOnRuntime() {
    postNativeFramePipelineCommand(
        operation = "status",
        coalescingKey = "status",
    ) {
        bindings.currentNativeFramePipelineStatus()
    }
}

internal fun VesperNativePlayerBridge.closeNativeFramePipelineOnRuntime(
    restoreSystemVideoForEpoch: Long? = null,
) {
    nativeFramePipelinePumpScheduler.executeCommand(
        NativeFramePipelineRuntimeCommand(
            operation = "close",
            runsDuringClose = true,
            replacesPendingCommands = true,
            action = closeCommand@{
                // Closing owns the native session captured by the command. A source
                // generation change fences active work, but must not revoke cleanup.
                runCatching {
                    releasePendingTimedNativeFrameFromRuntime(presented = false)
                    bindings.closeNativeFramePipeline()
                }.onSuccess {
                    if (restoreSystemVideoForEpoch != null) {
                        runOnMainThread {
                            if (isCurrentSourceLoad(restoreSystemVideoForEpoch)) {
                                bindings.restoreSystemPlaybackVideo()
                            }
                        }
                    }
                }.onFailure { error ->
                    runOnMainThread {
                        Log.w(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline close failed failure=${diagnosticPlaybackFailure(error)}")
                    }
                }
            },
            onRejected = {
                runOnMainThread {
                    Log.w(
                        NATIVE_PLAYER_BRIDGE_TAG,
                        "native-frame runtime command queue rejected close",
                    )
                }
            },
        )
    )
}

internal fun VesperNativePlayerBridge.isCurrentNativeFramePipelineRuntimeCommand(
    epoch: Long,
    allowDisposedBridge: Boolean,
): Boolean =
    sourceLoadEpoch.get() == epoch && (allowDisposedBridge || !isDisposed.get())

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
        Log.w(NATIVE_PLAYER_BRIDGE_TAG, "required native-frame pipeline $operation failed; stopping playback failure=${diagnosticPlaybackFailure(error)}")
    } else {
        Log.w(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pipeline $operation failed; falling back to system playback failure=${diagnosticPlaybackFailure(error)}")
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
        closeNativeFramePipelineOnRuntime(
            restoreSystemVideoForEpoch = sourceLoadEpoch.get().takeUnless { shouldFailRequiredNativeFrame },
        )
        if (shouldFailRequiredNativeFrame) {
            recordBenchmark("native_frame_pipeline_failed", mapOf("reason" to reason))
            failRequiredNativeFramePipeline(reason, currentSource, operation)
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

private fun VesperNativePlayerBridge.failRequiredNativeFramePipeline(
    reason: String,
    source: VesperPlayerSource?,
    operation: String,
    unsupported: Boolean = false,
) {
    hasInitializedSource = false
    pendingAutoPlay = false
    nativeFramePipelinePlaybackRequested = false
    runCatching { bindings.clearSystemPlayback() }
    clearTrackState()
    surfaceHost.reattachIfAvailable()
    val terminalError = VesperPlayerErrorState(
        message = reason,
        code = if (unsupported) VesperPlayerErrorCode.Unsupported else VesperPlayerErrorCode.BackendFailure,
        category = if (unsupported) VesperPlayerErrorCategory.Capability else VesperPlayerErrorCategory.Platform,
        retriable = false,
        details = mapOf(
            "reason" to if (operation == "open") "nativeFramePipelineOpenFailed" else "nativeFramePipelineRuntimeFailed",
            "route" to "nativeFrame",
            "operation" to operation,
        ),
    )
    updateState {
        copy(
            subtitle = i18n.stubError(reason),
            sourceLabel = source?.label ?: sourceLabel,
            playbackState = PlaybackStateUi.Ready,
            isBuffering = false,
            isInterrupted = false,
            lastError = terminalError,
        )
    }
}
