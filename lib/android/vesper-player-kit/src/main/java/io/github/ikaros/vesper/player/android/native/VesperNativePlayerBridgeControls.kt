package io.github.ikaros.vesper.player.android

import android.util.Log
import android.view.ViewGroup

internal fun VesperNativePlayerBridge.attachNativeSurfaceHost(host: ViewGroup) {
    recordBenchmark("attach_surface_host")
    surfaceHost.updateVideoLayout(bindings.currentVideoLayoutInfo())
    surfaceHost.attach(host)
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    syncNativeFramePipelineSurfaceDiagnostics()
    resetNativeFramePipelineFirstFrameWatchdogIfDetached()
    syncNativeFramePipelinePumpWithPlaybackState()
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.detachNativeSurfaceHost(host: ViewGroup?) {
    recordBenchmark("detach_surface_host")
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    if (isRequiredNativeFramePipelineFailureActive()) {
        surfaceHost.detachWithoutNativeNotification(host)
        return
    }
    surfaceHost.detach(host)
    syncNativeFramePipelineSurfaceDiagnostics()
    resetNativeFramePipelineFirstFrameWatchdogIfDetached()
}

internal fun VesperNativePlayerBridge.playNativeBridge() {
    recordBenchmark("play_command")
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    if (_uiState.value.playbackState == PlaybackStateUi.Finished) {
        restartNativeFramePipelineFromBeginning()
        if (isRequiredNativeFramePipelineFailureActive()) {
            return
        }
    }
    if (!hasInitializedSource) {
        if (currentSource != null) {
            pendingAutoPlay = true
        }
        return
    }
    bindings.play()
    nativeFramePipelinePlaybackRequested = true
    updateState {
        copy(
            playbackState = PlaybackStateUi.Playing,
            isBuffering = false,
            lastError = null,
        )
    }
    startNativeFramePipelinePump("play")
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.pauseNativeBridge() {
    recordBenchmark("pause_command")
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.pause()
    nativeFramePipelinePlaybackRequested = false
    updateState { copy(playbackState = PlaybackStateUi.Paused, isBuffering = false) }
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.toggleNativePause() {
    when (_uiState.value.playbackState) {
        PlaybackStateUi.Playing -> pause()
        PlaybackStateUi.Ready,
        PlaybackStateUi.Paused,
        PlaybackStateUi.Finished,
        -> play()
    }
}

internal fun VesperNativePlayerBridge.stopNativeBridge() {
    recordBenchmark("stop_command")
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    stopNativeFramePipelinePump()
    nativeFramePipelinePlaybackRequested = false
    bindings.stop()
    flushNativeFramePipeline()
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    updateState {
        copy(
            playbackState = PlaybackStateUi.Ready,
            timeline = timeline.copy(positionMs = 0L),
            isBuffering = false,
            lastError = null,
        )
    }
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.seekNativeBridgeBy(deltaMs: Long) {
    val current = _uiState.value.timeline
    val target = current.clampedPosition(current.positionMs + deltaMs)
    recordBenchmark("seek_start", mapOf("positionMs" to target.toString()))
    if (!seekBindingsTo(target)) {
        return
    }
    updateState { copy(timeline = timeline.copy(positionMs = target)) }
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.seekNativeBridgeToRatio(ratio: Float) {
    val timeline = _uiState.value.timeline
    val position = timeline.positionForRatio(ratio)
    recordBenchmark("seek_start", mapOf("positionMs" to position.toString()))
    if (!seekBindingsTo(position)) {
        return
    }
    updateState { copy(timeline = timeline.copy(positionMs = position)) }
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.seekNativeBridgeToLiveEdge() {
    val timeline = _uiState.value.timeline
    val liveEdge = timeline.goLivePositionMs ?: return
    recordBenchmark("seek_start", mapOf("positionMs" to liveEdge.toString()))
    if (!seekBindingsTo(liveEdge)) {
        return
    }
    updateState { copy(timeline = timeline.copy(positionMs = liveEdge)) }
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.setNativePlaybackRate(rate: Float) {
    recordBenchmark("set_playback_rate_command", mapOf("rate" to rate.toString()))
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.setPlaybackRate(rate)
    updateState { copy(playbackRate = rate) }
    reschedulePendingTimedNativeFrameForCurrentRate()
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.setNativeVideoTrackSelection(selection: VesperTrackSelection) {
    recordBenchmark("set_video_track_selection_command", mapOf("mode" to selection.mode.name))
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.setVideoTrackSelection(selection)
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.setNativeAudioTrackSelection(selection: VesperTrackSelection) {
    recordBenchmark("set_audio_track_selection_command", mapOf("mode" to selection.mode.name))
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.setAudioTrackSelection(selection)
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.setNativeSubtitleTrackSelection(selection: VesperTrackSelection) {
    recordBenchmark("set_subtitle_track_selection_command", mapOf("mode" to selection.mode.name))
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.setSubtitleTrackSelection(selection)
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.setNativeAbrPolicy(policy: VesperAbrPolicy) {
    recordBenchmark("set_abr_policy_command", mapOf("mode" to policy.mode.name))
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.setAbrPolicy(policy)
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.setNativeResiliencePolicy(policy: VesperPlaybackResiliencePolicy) {
    if (currentResiliencePolicy == policy) {
        return
    }

    currentResiliencePolicy = policy
    _resiliencePolicy.value = policy
    recordBenchmark("set_resilience_policy_command")
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    val source = currentSource ?: return
    if (!hasInitializedSource) {
        return
    }

    val preservedState = PreservedPlaybackState.capture(
        uiState = _uiState.value,
        trackSelection = _trackSelection.value,
    )

    Log.i(
        NATIVE_PLAYER_BRIDGE_TAG,
        "apply resilience policy buffering=${policy.buffering.preset} retry=${policy.retry.backoff} cache=${policy.cache.preset}",
    )
    updateState { copy(isBuffering = true) }
    launchSourceLoad {
        initializeNativeBridgeAsync()
        runOnMainForSourceLoad {
            restorePlaybackState(source, preservedState)
        }
    }
}

internal fun VesperNativePlayerBridge.setNativeKeepScreenOnDuringPlayback(enabled: Boolean) {
    keepScreenOnDuringPlayback = enabled
    syncKeepScreenOn()
}

internal fun VesperNativePlayerBridge.configureNativeSystemPlayback(
    configuration: VesperSystemPlaybackConfiguration,
) {
    if (isDisposed.get() || isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.configureSystemPlayback(configuration)
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.updateNativeSystemPlaybackMetadata(
    metadata: VesperSystemPlaybackMetadata,
) {
    if (isDisposed.get() || isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.updateSystemPlaybackMetadata(metadata)
    refreshFromNative()
}

internal fun VesperNativePlayerBridge.clearNativeSystemPlayback() {
    if (isRequiredNativeFramePipelineFailureActive()) {
        return
    }
    bindings.clearSystemPlayback()
}
