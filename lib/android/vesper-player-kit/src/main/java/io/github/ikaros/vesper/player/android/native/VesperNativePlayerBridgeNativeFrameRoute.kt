package io.github.ikaros.vesper.player.android

import android.util.Log
import java.io.File

internal fun VesperNativePlayerBridge.probePluginsForSource(
    source: VesperPlayerSource,
): List<Map<String, Any?>> {
    return pluginDiagnosticsWithNativeFramePipeline(probeMobilePluginsForSource(source))
}

internal fun VesperNativePlayerBridge.probeMobilePluginsForSource(
    source: VesperPlayerSource,
): List<Map<String, Any?>> {
    if (
        sourceNormalizerConfiguration.isDisabled &&
            frameProcessorConfiguration.isDisabled
    ) {
        return emptyList()
    }
    return runCatching {
        bindings.probeMobilePlugins(
            source = source,
            sourceNormalizerConfiguration = sourceNormalizerConfiguration,
            frameProcessorConfiguration = frameProcessorConfiguration,
        )
    }.onFailure { error ->
            Log.w(NATIVE_PLAYER_BRIDGE_TAG, "mobile plugin diagnostics failed for source=${source.uri}", error)
    }.getOrDefault(emptyList())
}

internal fun VesperNativePlayerBridge.pluginDiagnosticsWithNativeFramePipeline(
    pluginDiagnostics: List<Map<String, Any?>>,
): List<Map<String, Any?>> {
    val withoutNativeFrame =
        pluginDiagnostics.filter { diagnostic ->
            diagnostic["pluginKind"] != "native_frame_pipeline"
        }
    return withoutNativeFrame + nativeFramePipelineDiagnostics()
}

internal fun VesperNativePlayerBridge.nativeFramePipelineDiagnostics(): List<Map<String, Any?>> {
    if (nativeFramePipelineConfiguration.isDisabled) {
        return emptyList()
    }
    val participation =
        if (nativeFramePipelineFallbackReason != null) {
            if (nativeFramePipelineRequiredFailure) "selected" else "fallback"
        } else {
            when (nativeFramePipelineConfiguration.mode) {
                VesperNativeFramePipelineMode.PreferNativeFrame,
                VesperNativeFramePipelineMode.RequireNativeFrame -> "selected"
                VesperNativeFramePipelineMode.Disabled,
                VesperNativeFramePipelineMode.DiagnosticsOnly -> "available"
            }
        }
    val route =
        when (nativeFramePipelineConfiguration.mode) {
            VesperNativeFramePipelineMode.Disabled,
            VesperNativeFramePipelineMode.DiagnosticsOnly -> "systemPlayer"
            VesperNativeFramePipelineMode.PreferNativeFrame,
            VesperNativeFramePipelineMode.RequireNativeFrame ->
                if (
                    nativeFramePipelineFallbackReason == null ||
                        nativeFramePipelineRequiredFailure
                ) {
                    "sdkManagedNativeFrame"
                } else {
                    "systemPlayer"
                }
        }
    val status = if (nativeFramePipelineFallbackReason == null) "loaded" else "unsupported"
    val message =
        when (nativeFramePipelineConfiguration.mode) {
            VesperNativeFramePipelineMode.Disabled ->
                "Mobile native-frame pipeline is disabled; system player remains selected."
            VesperNativeFramePipelineMode.DiagnosticsOnly ->
                "Mobile native-frame pipeline diagnostics are enabled; playback still uses the system player."
            VesperNativeFramePipelineMode.PreferNativeFrame ->
                "Mobile native-frame pipeline is explicitly preferred; Android MediaCodec release-to-surface lane is selected when available."
            VesperNativeFramePipelineMode.RequireNativeFrame ->
                "Mobile native-frame pipeline is explicitly required; Android MediaCodec release-to-surface lane must be available."
        }
    val resolvedMessage =
        nativeFramePipelineFallbackReason?.let {
            val failureLabel =
                if (nativeFramePipelineRequiredFailure) "Failure reason" else "Fallback reason"
            "$message $failureLabel: $it"
        } ?: nativeFramePipelineLastStatus?.get("message")?.toString()?.takeIf(String::isNotBlank)?.let {
            "$message Native-frame lifecycle is open; advance currently reports: $it."
        } ?: nativeFramePipelineOpenStatus?.let {
            "$message Native-frame lifecycle is open; packet decode and release-to-surface presentation are active while playback is running."
        } ?: message
    val counters = nativeFramePipelineCounters()
    return listOf(
        mutableMapOf<String, Any?>(
            "path" to
                (
                    nativeFramePipelineConfiguration.decoderPluginLibraryPaths +
                        nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths
                ).joinToString(separator = File.pathSeparator),
            "pluginName" to "vesper-android-native-frame-pipeline",
            "pluginKind" to "native_frame_pipeline",
            "status" to status,
            "message" to
                "$resolvedMessage decoderPlugins=${nativeFramePipelineConfiguration.decoderPluginLibraryPaths.size}; " +
                    "frameProcessors=${nativeFramePipelineConfiguration.frameProcessorPluginLibraryPaths.size}; " +
                    "maxInFlightFrames=${nativeFramePipelineConfiguration.maxInFlightFrames ?: "default"}",
            "participation" to participation,
            "route" to route,
            "sourceInput" to "sourceNormalizerPacket",
            "decoderAdapter" to "MediaCodec",
            "presenterProfile" to
                (
                    nativeFramePipelineOpenStatus?.get("presenterProfile")?.toString()
                        ?: nativeFramePresenterProfileName()
                ),
            "presenterReady" to nativeFramePipelineBooleanValue("presenterReady"),
            "presenterConfigured" to nativeFramePipelineBooleanValue("presenterConfigured"),
            "presenterState" to nativeFramePipelineStringValue("presenterState"),
            "surfaceAttached" to nativeFramePipelineBooleanValue("surfaceAttached"),
            "surfaceProfile" to nativeFramePipelineStringValue("surfaceProfile"),
            "pipelineProfile" to
                (
                    nativeFramePipelineStringValue("pipelineProfile")
                        ?: "media_codec_surface_texture"
                ),
            "pumpRunning" to nativeFramePipelinePumpRunning,
            "decodedFrames" to counters.longValue("decodedFrames"),
            "processedFrames" to counters.longValue("processedFrames"),
            "presenterSubmitCount" to counters.longValue("presenterSubmitCount"),
            "presentedFrames" to counters.longValue("presentedFrames"),
            "deadlineMisses" to counters.longValue("deadlineMisses"),
            "backpressureCount" to counters.longValue("backpressureCount"),
            "lateDropped" to counters.longValue("lateDropped"),
            "lifecycle" to
                when {
                    nativeFramePipelineRequiredFailure -> "failed"
                    nativeFramePipelineFallbackReason != null -> "fallback"
                    nativeFramePipelineOpenStatus != null -> "open"
                    else -> "notOpened"
                },
            "lastAdvanceStatus" to nativeFramePipelineLastStatus?.get("status"),
            "fallbackTargetRoute" to
                if (
                    nativeFramePipelineFallbackReason == null ||
                        nativeFramePipelineRequiredFailure
                ) {
                    null
                } else {
                    "systemPlayer"
                },
            "fallbackReason" to nativeFramePipelineFallbackReason,
        )
    )
}

internal fun VesperNativePlayerBridge.evaluateNativeFramePipelineRoute(): NativeFramePipelineRoute {
    return when (nativeFramePipelineConfiguration.mode) {
        VesperNativeFramePipelineMode.Disabled,
        VesperNativeFramePipelineMode.DiagnosticsOnly -> {
            nativeFramePipelineFallbackReason = null
            nativeFramePipelineRequiredFailure = false
            NativeFramePipelineRoute.SystemPlayer
        }
        VesperNativeFramePipelineMode.PreferNativeFrame,
        VesperNativeFramePipelineMode.RequireNativeFrame -> {
            val reason = nativeFramePipelineUnavailableReason()
            if (reason == null) {
                nativeFramePipelineFallbackReason = null
                nativeFramePipelineRequiredFailure = false
                currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
                NativeFramePipelineRoute.SdkManaged
            } else {
                nativeFramePipelineFallbackReason = reason
                nativeFramePipelineRequiredFailure =
                    nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame
                currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(currentPluginDiagnostics)
                if (nativeFramePipelineConfiguration.mode == VesperNativeFramePipelineMode.RequireNativeFrame) {
                    NativeFramePipelineRoute.Fail(reason)
                } else {
                    NativeFramePipelineRoute.Fallback(reason)
                }
            }
        }
    }
}

internal fun VesperNativePlayerBridge.nativeFramePipelineUnavailableReason(): String? {
    if (currentSource?.drmConfiguration != null) {
        return drmUnsupportedRouteMessage("nativeFrame")
    }
    if (nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty()) {
        return "Android native-frame pipeline requires a MediaCodec decoder plugin path."
    }
    if (sourceNormalizerConfiguration.pluginLibraryPaths.isEmpty()) {
        return "Android native-frame pipeline requires a SourceNormalizer packet-stream plugin path."
    }
    if (surfaceKind != NativeVideoSurfaceKind.SurfaceView) {
        return "Android native-frame pipeline currently supports SurfaceView only; TextureView falls back to system playback."
    }
    return null
}

internal sealed interface NativeFramePipelineRoute {
    data object SystemPlayer : NativeFramePipelineRoute
    data class Fallback(val reason: String) : NativeFramePipelineRoute
    data class Fail(val reason: String) : NativeFramePipelineRoute
    data object SdkManaged : NativeFramePipelineRoute
}

internal fun nativeFrameRouteLogLabel(route: NativeFramePipelineRoute): String =
    when (route) {
        NativeFramePipelineRoute.SystemPlayer -> "systemPlayer"
        is NativeFramePipelineRoute.Fallback -> "fallback:${route.reason}"
        is NativeFramePipelineRoute.Fail -> "fail:${route.reason}"
        NativeFramePipelineRoute.SdkManaged -> "sdkManagedNativeFrame"
    }
