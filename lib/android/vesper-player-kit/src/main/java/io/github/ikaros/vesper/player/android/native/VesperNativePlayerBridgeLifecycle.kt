package io.github.ikaros.vesper.player.android

import android.util.Log

internal fun VesperNativePlayerBridge.initializeNativeBridge() {
    if (isDisposed.get()) {
        return
    }
    recordBenchmark("initialize_start")
    val source = currentSource ?: run {
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
        return
    }
    currentPluginDiagnostics = probePluginsForSource(source)
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
            return
        }
        NativeFramePipelineRoute.SdkManaged -> {
            recordBenchmark("native_frame_pipeline_selected")
        }
    }
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
            recordBenchmark(
                "initialize_failed",
                mapOf("error" to (it.message ?: it::class.java.simpleName)),
            )
            hasInitializedSource = false
            pendingAutoPlay = false
            clearTrackState()
            Log.e(NATIVE_PLAYER_BRIDGE_TAG, "failed to initialize source=${source.uri}", it)
            val message = it.message?.takeUnless(String::isBlank) ?: i18n.nativeBindingsUnavailable()
            val terminalError = it.toInitializePlayerErrorState(message)
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
    advanceNativeUpdateEpoch(clearListener = true)
    hasInitializedSource = false
    stopNativeFramePipelinePump()
    releasePendingTimedNativeFrameOnRuntime(presented = false)
    closeNativeFramePipelineOnRuntime()
    nativeFramePipelinePumpScheduler.close()
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
    initialize()
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
