package io.github.ikaros.vesper.player.android

internal fun VesperNativePlayerBridge.releasePendingTimedNativeFrame(presented: Boolean) {
    releasePendingTimedNativeFrameOnRuntime(presented)
}

internal fun VesperNativePlayerBridge.releasePendingTimedNativeFrameOnRuntime(presented: Boolean) {
    nativeFramePipelinePumpScheduler.execute {
        val result = runCatching {
            releasePendingTimedNativeFrameFromRuntime(presented)
        }
        runOnMainThread {
            result.onFailure { handleNativeFramePipelineRuntimeFailure("release", it) }
        }
    }
}

internal fun VesperNativePlayerBridge.takePendingTimedNativeFrameForRuntime(): TimedNativeFrameRelease? =
    synchronized(nativeFramePipelineRuntimeLock) {
        pendingTimedNativeFrame?.also {
            pendingTimedNativeFrame = null
        }
    }

internal fun VesperNativePlayerBridge.storePendingTimedNativeFrameFromRuntime(timedFrame: TimedNativeFrameRelease) {
    synchronized(nativeFramePipelineRuntimeLock) {
        pendingTimedNativeFrame = timedFrame
    }
}

internal fun VesperNativePlayerBridge.clearPendingTimedNativeFrameFromRuntime(): TimedNativeFrameRelease? =
    synchronized(nativeFramePipelineRuntimeLock) {
        pendingTimedNativeFrame?.also {
            pendingTimedNativeFrame = null
        }
    }

internal fun VesperNativePlayerBridge.releasePendingTimedNativeFrameFromRuntime(presented: Boolean) {
    val pending = clearPendingTimedNativeFrameFromRuntime() ?: return
    if (presented) {
        bindings.releaseNativeFramePipelineFrame(pending.handle, presented = true)
    } else {
        releaseStaleNativeFramePipelineFrame(pending.handle)
    }
}

internal fun VesperNativePlayerBridge.releaseNativeFramePipelineFrame(frameHandle: Long, presented: Boolean) {
    postNativeFramePipelineRelease(frameHandle, presented)
}

internal fun VesperNativePlayerBridge.postNativeFramePipelineRelease(frameHandle: Long, presented: Boolean) {
    nativeFramePipelinePumpScheduler.execute {
        val result =
            runCatching {
                bindings.releaseNativeFramePipelineFrame(frameHandle, presented = presented)
            }
        runOnMainThread {
            result
                .onSuccess { status ->
                    nativeFramePipelineLastStatus = status ?: nativeFramePipelineLastStatus
                    publishNativeFramePipelinePumpStatus(nativeFramePipelineLastStatus)
                }
                .onFailure { handleNativeFramePipelineRuntimeFailure("release", it) }
        }
    }
}

internal fun VesperNativePlayerBridge.releaseStaleNativeFramePipelineFrame(frameHandle: Long) {
    bindings.releaseNativeFramePipelineFrame(frameHandle, presented = false)
}

internal fun VesperNativePlayerBridge.nativeFramePipelineCounters(): Map<String, Any?> {
    val counters =
        nativeFramePipelineLastStatus?.get("counters")
            ?: nativeFramePipelineOpenStatus?.get("counters")
            ?: return emptyMap()
    return (counters as? Map<*, *>)
        ?.mapNotNull { (key, value) ->
            key?.toString()?.let { it to value }
        }
        ?.toMap()
        ?: emptyMap()
}

internal fun VesperNativePlayerBridge.nativeFramePipelineStringValue(key: String): String? =
    if (nativeFramePipelineLastStatus != null) {
        nativeFramePipelineLastStatus?.get(key)?.toString()
    } else {
        nativeFramePipelineOpenStatus?.get(key)?.toString()
    }

internal fun VesperNativePlayerBridge.nativeFramePipelineBooleanValue(key: String): Boolean =
    nativeFramePipelineLastStatus?.get(key)?.toBooleanOrFalse()
        ?: nativeFramePipelineOpenStatus?.get(key).toBooleanOrFalse()

internal fun Any?.toBooleanOrFalse(): Boolean =
    when (this) {
        is Boolean -> this
        is String -> this.equals("true", ignoreCase = true)
        is Number -> this.toInt() != 0
        else -> false
    }

internal fun Map<String, Any?>.longValue(key: String): Long =
    when (val value = this[key]) {
        is Number -> value.toLong()
        is String -> value.toLongOrNull() ?: 0L
        else -> 0L
    }

internal fun Map<String, Any?>.nativeFramePipelineFrameHandle(): Long? {
    val value = this["handle"] ?: return null
    return when (value) {
        is Number -> value.toLong()
        is String -> value.toLongOrNull()
        else -> null
    }?.takeIf { it > 0L }
}

internal fun Map<String, Any?>.nativeFramePipelineTimedFrame(): TimedNativeFrameRelease? {
    if (this["status"] != "frame") {
        return null
    }
    val handle = nativeFramePipelineFrameHandle() ?: return null
    val presentationTimeUs =
        when (val value = this["presentationTimeUs"]) {
            is Number -> value.toLong()
            is String -> value.toLongOrNull()
            else -> null
        } ?: return null
    return TimedNativeFrameRelease(handle, presentationTimeUs)
}

internal fun VesperNativePlayerBridge.nativeFramePresenterProfileName(): String =
    when (surfaceKind) {
        NativeVideoSurfaceKind.SurfaceView -> "SurfaceView"
        NativeVideoSurfaceKind.TextureView -> "SurfaceTexture"
    }
