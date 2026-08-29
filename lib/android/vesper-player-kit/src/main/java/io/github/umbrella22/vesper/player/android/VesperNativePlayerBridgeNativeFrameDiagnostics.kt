package io.github.umbrella22.vesper.player.android

import android.os.SystemClock
import android.util.Log

internal fun VesperNativePlayerBridge.publishNativeFramePipelinePumpStatus(status: Map<String, Any?>?) {
    if (status == null) {
        return
    }
    if (nativeFramePipelineCountersFromStatus(status).longValue("presentedFrames") > 0L) {
        nativeFramePipelineParticipated = true
    }
    val key = nativeFramePipelinePumpSummaryKey(status)
    if (key != nativeFramePipelineLastLoggedPumpKey) {
        nativeFramePipelineLastLoggedPumpKey = key
        Log.d(NATIVE_PLAYER_BRIDGE_TAG, "native-frame pump ${nativeFramePipelinePumpSummary(status)}")
    }
    if (key != nativeFramePipelineLastPublishedDiagnosticsKey) {
        nativeFramePipelineLastPublishedDiagnosticsKey = key
        markNativeFramePipelineDiagnosticsDirty()
    }
}

internal fun VesperNativePlayerBridge.markNativeFramePipelineDiagnosticsDirty() {
    nativeFramePipelineDiagnosticsDirty = true
}

internal fun VesperNativePlayerBridge.refreshNativeFramePipelineDiagnosticsIfDirty() {
    if (!nativeFramePipelineDiagnosticsDirty) {
        return
    }
    nativeFramePipelineDiagnosticsDirty = false
    currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
}

internal fun VesperNativePlayerBridge.nativeFramePipelinePumpSummary(status: Map<String, Any?>): String {
    val counters = nativeFramePipelineCounters()
    val message =
        status["message"]
            ?.toString()
            ?.replace('\n', ' ')
            ?.take(120)
            .orEmpty()
    return "status=${status["status"]} " +
        "surfaceAttached=${nativeFramePipelineBooleanValue("surfaceAttached")} " +
        "presenterReady=${nativeFramePipelineBooleanValue("presenterReady")} " +
        "presenterState=${nativeFramePipelineStringValue("presenterState")} " +
        "decodedFrames=${counters.longValue("decodedFrames")} " +
        "processedFrames=${counters.longValue("processedFrames")} " +
        "presenterSubmits=${counters.longValue("presenterSubmitCount")} " +
        "presentedFrames=${counters.longValue("presentedFrames")} " +
        "sourcePackets=${counters.longValue("sourcePacketsRead")} " +
        "decoderPackets=${counters.longValue("decoderPacketsSent")} " +
        "decoderBackpressure=${counters.longValue("decoderBackpressureCount")} " +
        "deadlineMisses=${counters.longValue("deadlineMisses")} " +
        "backpressure=${counters.longValue("backpressureCount")} " +
        "message=$message"
}

internal fun VesperNativePlayerBridge.nativeFramePipelinePumpSummaryKey(status: Map<String, Any?>): String {
    val counters = nativeFramePipelineCounters()
    val message =
        status["message"]
            ?.toString()
            ?.replace('\n', ' ')
            ?.take(80)
            .orEmpty()
    return "status=${status["status"]};" +
        "surface=${nativeFramePipelineBooleanValue("surfaceAttached")};" +
        "ready=${nativeFramePipelineBooleanValue("presenterReady")};" +
        "state=${nativeFramePipelineStringValue("presenterState")};" +
        "decoded=${nativeFramePipelineCounterLogBucket(counters.longValue("decodedFrames"))};" +
        "processed=${nativeFramePipelineCounterLogBucket(counters.longValue("processedFrames"))};" +
        "submits=${nativeFramePipelineCounterLogBucket(counters.longValue("presenterSubmitCount"))};" +
        "presented=${nativeFramePipelineCounterLogBucket(counters.longValue("presentedFrames"))};" +
        "sourcePackets=${nativeFramePipelineCounterLogBucket(counters.longValue("sourcePacketsRead"))};" +
        "decoderPackets=${nativeFramePipelineCounterLogBucket(counters.longValue("decoderPacketsSent"))};" +
        "decoderBackpressure=${nativeFramePipelineCounterLogBucket(counters.longValue("decoderBackpressureCount"))};" +
        "deadline=${nativeFramePipelineCounterLogBucket(counters.longValue("deadlineMisses"))};" +
        "backpressure=${nativeFramePipelineCounterLogBucket(counters.longValue("backpressureCount"))};" +
        "message=$message"
}

internal fun VesperNativePlayerBridge.nativeFramePipelineCounterLogBucket(value: Long): Long =
    when {
        value <= 0L -> 0L
        value < NATIVE_FRAME_PIPELINE_LOG_COUNTER_BUCKET_SIZE -> 1L
        else -> value / NATIVE_FRAME_PIPELINE_LOG_COUNTER_BUCKET_SIZE
    }

internal fun VesperNativePlayerBridge.enforceNativeFramePipelineFirstFrameWatchdog(): Boolean {
    if (
        nativeFramePipelineOpenStatus == null ||
            nativeFramePipelineFallbackReason != null ||
            !nativeFramePipelinePumpRunning
    ) {
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        return false
    }
    val counters = nativeFramePipelineCounters()
    if (counters.longValue("presentedFrames") > 0L) {
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        return false
    }
    val surfaceAttached = nativeFramePipelineBooleanValue("surfaceAttached")
    val presenterConfigured = nativeFramePipelineBooleanValue("presenterConfigured")
    if (!surfaceAttached || !presenterConfigured) {
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
        return false
    }
    val now = SystemClock.elapsedRealtime()
    val startedAt =
        nativeFramePipelineFirstFrameWatchdogStartedAtMs ?: now.also {
            nativeFramePipelineFirstFrameWatchdogStartedAtMs = it
        }
    if (now - startedAt < NATIVE_FRAME_PIPELINE_FIRST_FRAME_TIMEOUT_MS) {
        return false
    }

    val reason =
        "Android native-frame pipeline did not present a frame within " +
            "${NATIVE_FRAME_PIPELINE_FIRST_FRAME_TIMEOUT_MS}ms after surface attachment " +
            "(presenterState=${nativeFramePipelineStringValue("presenterState")}, " +
            "decodedFrames=${counters.longValue("decodedFrames")}, " +
            "presenterSubmits=${counters.longValue("presenterSubmitCount")}, " +
            "presentedFrames=${counters.longValue("presentedFrames")})."
    handleNativeFramePipelineRuntimeFailure(
        "first-frame-timeout",
        IllegalStateException(reason),
    )
    return true
}

internal fun VesperNativePlayerBridge.resetNativeFramePipelineFirstFrameWatchdogIfDetached() {
    if (
        !nativeFramePipelineBooleanValue("surfaceAttached") ||
            nativeFramePipelineCounters().longValue("presentedFrames") > 0L
    ) {
        nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
    }
}

internal fun VesperNativePlayerBridge.resetNativeFramePipelineRuntimeMarkers() {
    nativeFramePipelineParticipated = false
    nativeFramePipelineFirstFrameWatchdogStartedAtMs = null
    nativeFramePipelineLastLoggedPumpKey = null
    nativeFramePipelineLastPublishedDiagnosticsKey = null
}

internal fun VesperNativePlayerBridge.nativeFramePipelinePumpDelayMs(status: Map<String, Any?>?): Long? =
    when (status?.get("status")?.toString()) {
        "endOfStream" -> null
        "presented", "released", "frame" -> NATIVE_FRAME_PIPELINE_ACTIVE_PUMP_DELAY_MS
        "pending" -> {
            val message = status["message"]?.toString().orEmpty()
            if (message.contains("backpressure", ignoreCase = true)) {
                NATIVE_FRAME_PIPELINE_BACKPRESSURE_PUMP_DELAY_MS
            } else {
                NATIVE_FRAME_PIPELINE_IDLE_PUMP_DELAY_MS
            }
        }
        else -> NATIVE_FRAME_PIPELINE_IDLE_PUMP_DELAY_MS
    }

internal fun VesperNativePlayerBridge.syncNativeFramePipelinePumpWithPlaybackState() {
    if (nativeFramePipelineOpenStatus != null && nativeFramePipelineFallbackReason == null) {
        if (nativeFramePipelinePlaybackRequested) {
            startNativeFramePipelinePump("playback-request")
        } else {
            stopNativeFramePipelinePump()
        }
        return
    }
    when (_uiState.value.playbackState) {
        PlaybackStateUi.Playing -> startNativeFramePipelinePump("playback-state")
        PlaybackStateUi.Ready,
        PlaybackStateUi.Paused,
        PlaybackStateUi.Finished,
        -> stopNativeFramePipelinePump()
    }
}

internal fun VesperNativePlayerBridge.nativeFramePipelineDelayUntilPresentation(presentationTimeUs: Long): Long {
    val timeline = _uiState.value.timeline
    val framePositionMs = (presentationTimeUs / 1_000L).coerceAtLeast(0L)
    val deltaMs = framePositionMs - timeline.positionMs
    if (deltaMs <= 0L) {
        return 0L
    }
    val playbackRate = _uiState.value.playbackRate.takeIf { it.isFinite() && it > 0f } ?: 1f
    return (deltaMs / playbackRate).toLong().coerceIn(1L, NATIVE_FRAME_PIPELINE_MAX_FRAME_DELAY_MS)
}

internal fun VesperNativePlayerBridge.reschedulePendingTimedNativeFrameForCurrentRate() {
    val pending = synchronized(nativeFramePipelineRuntimeLock) {
        pendingTimedNativeFrame
    } ?: return
    if (
        !nativeFramePipelinePumpRunning ||
            nativeFramePipelineOpenStatus == null ||
            nativeFramePipelineFallbackReason != null
    ) {
        return
    }
    scheduleNativeFramePipelinePump(
        nativeFramePipelineDelayUntilPresentation(pending.presentationTimeUs)
    )
}
