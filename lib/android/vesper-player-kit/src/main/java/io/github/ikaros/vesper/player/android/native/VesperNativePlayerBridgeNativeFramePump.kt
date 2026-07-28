package io.github.ikaros.vesper.player.android

import android.util.Log

internal fun VesperNativePlayerBridge.startNativeFramePipelinePump(reason: String) {
    var alreadyRunning = false
    val started =
        synchronized(nativeFramePipelineRuntimeLock) {
            if (
                isDisposed.get() ||
                    nativeFramePipelineOpenStatus == null ||
                    nativeFramePipelineFallbackReason != null
            ) {
                return@synchronized false
            }
            if (nativeFramePipelinePumpRunning) {
                alreadyRunning = true
                return@synchronized false
            }
            nativeFramePipelinePumpRunning = true
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
            scheduleNativeFramePipelinePump(delayMs = 0L)
            true
        }
    if (!started) {
        if (alreadyRunning) {
            markNativeFramePipelineDiagnosticsDirty()
        }
        return
    }
    Log.d(NATIVE_PLAYER_BRIDGE_TAG, "starting native-frame pipeline pump reason=$reason")
    markNativeFramePipelineDiagnosticsDirty()
}

internal fun VesperNativePlayerBridge.stopNativeFramePipelinePump() {
    var stopped = false
    var hadPendingFrame = false
    synchronized(nativeFramePipelineRuntimeLock) {
        hadPendingFrame = pendingTimedNativeFrame != null
        if (nativeFramePipelinePumpRunning) {
            nativeFramePipelinePumpRunning = false
            nativeFramePipelinePumpEpoch += 1
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
            nativeFramePipelinePumpScheduler.cancel()
            stopped = true
        }
    }
    if (!stopped && !hadPendingFrame) {
        return
    }
    if (stopped) {
        Log.d(NATIVE_PLAYER_BRIDGE_TAG, "stopping native-frame pipeline pump")
        markNativeFramePipelineDiagnosticsDirty()
    }
    if (hadPendingFrame) {
        releasePendingTimedNativeFrameOnRuntime(presented = false)
    }
}

internal fun VesperNativePlayerBridge.scheduleNativeFramePipelinePump(delayMs: Long) {
    val epoch = nativeFramePipelinePumpEpoch
    nativeFramePipelinePumpScheduler.schedule(delayMs) {
        runNativeFramePipelinePumpTickWorker(epoch)
    }
}

internal fun VesperNativePlayerBridge.runNativeFramePipelinePumpTickWorker(epoch: Long) {
    runCatching {
        runNativeFramePipelinePumpTickWorkerUnchecked(epoch)
    }.onFailure { error ->
        runOnMainThread {
            if (canApplyNativeFramePumpResult(epoch)) {
                handleNativeFramePipelineRuntimeFailure("pump", error)
            }
        }
    }
}

internal fun VesperNativePlayerBridge.runNativeFramePipelinePumpTickWorkerUnchecked(epoch: Long) {
    if (!canContinueNativeFramePump(epoch)) {
        releasePendingTimedNativeFrameFromRuntime(presented = false)
        return
    }
    val pendingRelease = takePendingTimedNativeFrameForRuntime()
    if (pendingRelease != null) {
        if (pendingRelease.pumpEpoch != epoch || !canContinueNativeFramePump(epoch)) {
            releaseStaleNativeFramePipelineFrame(pendingRelease.handle)
            return
        }
        val releaseResult =
            runCatching {
                bindings.releaseNativeFramePipelineFrame(pendingRelease.handle, presented = true)
            }
        runOnMainThread {
            applyNativeFramePipelineReleaseResult(epoch, releaseResult)
        }
        if (!canContinueNativeFramePump(epoch)) {
            return
        }
    } else if (!canContinueNativeFramePump(epoch)) {
        return
    }

    val advanceResult = runCatching { advanceNativeFramePipelineOnce() }
    val advanceStatus = advanceResult.getOrNull()
    if (
        advanceStatus != null &&
            !registerAdvancedNativeFrameForCurrentPumpFromRuntime(epoch, advanceStatus)
    ) {
        return
    }
    if (!canContinueNativeFramePump(epoch)) {
        return
    }
    runOnMainThread {
        applyNativeFramePipelineAdvanceResult(epoch, advanceResult)
    }
}

internal fun VesperNativePlayerBridge.applyNativeFramePipelineReleaseResult(
    epoch: Long,
    result: Result<Map<String, Any?>?>,
) {
    if (!canApplyNativeFramePumpResult(epoch)) {
        return
    }
    result
        .onSuccess { status ->
            nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
            publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
        }
        .onFailure { handleNativeFramePipelineRuntimeFailure("release", it) }
}

internal fun VesperNativePlayerBridge.applyNativeFramePipelineAdvanceResult(
    epoch: Long,
    result: Result<Map<String, Any?>?>,
) {
    if (!canApplyNativeFramePumpResult(epoch)) {
        return
    }
    val status =
        result
            .onFailure { handleNativeFramePipelineRuntimeFailure("advance", it) }
            .getOrElse { return }
    runNativeFramePipelinePumpTick(epoch, status)
}

internal fun VesperNativePlayerBridge.runNativeFramePipelinePumpTick(
    epoch: Long,
    status: Map<String, Any?>?,
) {
    if (
        epoch != nativeFramePipelinePumpEpoch ||
            !nativeFramePipelinePumpRunning ||
            isDisposed.get()
    ) {
        return
    }
    if (nativeFramePipelineOpenStatus == null || nativeFramePipelineFallbackReason != null) {
        stopNativeFramePipelinePump()
        return
    }

    nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
    publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
    val timedFrame = status?.nativeFramePipelineTimedFrame(epoch)
    if (timedFrame != null) {
        if (!registerAdvancedNativeFrameForCurrentPumpFromRuntime(epoch, status)) {
            return
        }
        val delayMs = nativeFramePipelineDelayUntilPresentation(timedFrame.presentationTimeUs)
        scheduleNativeFramePipelinePump(delayMs)
        return
    }
    if (enforceNativeFramePipelineFirstFrameWatchdog()) {
        return
    }
    val nextDelayMs = nativeFramePipelinePumpDelayMs(status)
    if (nextDelayMs == null) {
        stopNativeFramePipelinePump()
        return
    }
    scheduleNativeFramePipelinePump(nextDelayMs)
}

internal fun VesperNativePlayerBridge.canContinueNativeFramePump(epoch: Long): Boolean =
    canApplyNativeFramePumpResult(epoch) &&
        nativeFramePipelineOpenStatus != null &&
        nativeFramePipelineFallbackReason == null

internal fun VesperNativePlayerBridge.canApplyNativeFramePumpResult(epoch: Long): Boolean =
    epoch == nativeFramePipelinePumpEpoch &&
        nativeFramePipelinePumpRunning &&
        !isDisposed.get()
