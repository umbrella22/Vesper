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
            updateState {
                copy(
                    subtitle = i18n.stubError(nativeFrameDecision.reason),
                    sourceLabel = source.label,
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
                updateState { copy(playbackState = PlaybackStateUi.Playing, isBuffering = false) }
                startNativeFramePipelinePump("autoplay")
            }
            updateState {
                copy(
                    subtitle = it.subtitle ?: sourceSubtitle(source),
                    sourceLabel = source.label,
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
            updateState {
                copy(
                    subtitle = i18n.stubError(message),
                    sourceLabel = source.label,
                )
            }
        }
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
        )
    }
    initialize()
}
