package io.github.ikaros.vesper.player.android

import android.util.Log

internal fun VesperNativePlayerBridge.startNativeFramePipelinePump(reason: String) {
    if (
        isDisposed.get() ||
            nativeFramePipelineOpenStatus == null ||
            nativeFramePipelineFallbackReason != null
    ) {
        return
    }
    if (nativeFramePipelinePumpRunning) {
        markNativeFramePipelineDiagnosticsDirty()
        return
    }
    Log.d(NATIVE_PLAYER_BRIDGE_TAG, "starting native-frame pipeline pump reason=$reason")
    nativeFramePipelinePumpRunning = true
    nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
    scheduleNativeFramePipelinePump(delayMs = 0L)
    markNativeFramePipelineDiagnosticsDirty()
}

internal fun VesperNativePlayerBridge.stopNativeFramePipelinePump() {
    if (!nativeFramePipelinePumpRunning) {
        return
    }
    Log.d(NATIVE_PLAYER_BRIDGE_TAG, "stopping native-frame pipeline pump")
    nativeFramePipelinePumpRunning = false
    nativeFramePipelinePumpEpoch += 1
    nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
    nativeFramePipelinePumpScheduler.cancel()
    markNativeFramePipelineDiagnosticsDirty()
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
        if (!canContinueNativeFramePump(epoch)) {
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
    val timedFrame = status?.nativeFramePipelineTimedFrame()
    if (timedFrame != null) {
        val delayMs = nativeFramePipelineDelayUntilPresentation(timedFrame.presentationTimeUs)
        if (delayMs > 0L) {
            storePendingTimedNativeFrameFromRuntime(timedFrame)
            scheduleNativeFramePipelinePump(delayMs)
            return
        }
        val releaseResult =
            runCatching {
                bindings.releaseNativeFramePipelineFrame(timedFrame.handle, presented = true)
            }
        applyNativeFramePipelineReleaseResult(epoch, releaseResult)
        if (nativeFramePipelineOpenStatus == null || nativeFramePipelineFallbackReason != null) {
            return
        }
        publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
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
