package io.github.umbrella22.vesper.player.android

import org.json.JSONArray
import org.json.JSONObject

enum class VesperSourceNormalizerMode {
    Disabled,
    DiagnosticsOnly,
    PreflightOnly,
    PreferNormalized,
    RequireNormalized,
}

data class VesperSourceNormalizerConfiguration(
    val mode: VesperSourceNormalizerMode = VesperSourceNormalizerMode.Disabled,
    /**
     * Explicit plugin identities selected by the host. The registry resolves
     * these references to build-time or packaged artifacts.
     */
    val pluginReferences: List<VesperPluginReference> = emptyList(),
    val runtimeProfile: String? = null,
) {
    internal val isDisabled: Boolean
        get() = mode == VesperSourceNormalizerMode.Disabled

    internal val modeOrdinal: Int
        get() = when (mode) {
            VesperSourceNormalizerMode.Disabled -> 0
            VesperSourceNormalizerMode.DiagnosticsOnly -> 1
            VesperSourceNormalizerMode.PreflightOnly -> 2
            VesperSourceNormalizerMode.PreferNormalized -> 3
            VesperSourceNormalizerMode.RequireNormalized -> 4
        }
}

enum class VesperFrameProcessorMode {
    Disabled,
    DiagnosticsOnly,
}

enum class VesperNativeFramePipelineMode {
    Disabled,
    DiagnosticsOnly,
    PreferNativeFrame,
    RequireNativeFrame,
}

data class VesperFrameProcessorConfiguration(
    val mode: VesperFrameProcessorMode = VesperFrameProcessorMode.Disabled,
    /** Explicit plugin identities selected by the host. */
    val pluginReferences: List<VesperPluginReference> = emptyList(),
) {
    internal val isDisabled: Boolean
        get() = mode == VesperFrameProcessorMode.Disabled

    internal val modeOrdinal: Int
        get() = when (mode) {
            VesperFrameProcessorMode.Disabled -> 0
            VesperFrameProcessorMode.DiagnosticsOnly -> 1
        }
}

data class VesperNativeFramePipelineConfiguration(
    val mode: VesperNativeFramePipelineMode = VesperNativeFramePipelineMode.Disabled,
    /** Explicit decoder plugin identities selected by the host. */
    val decoderPluginReferences: List<VesperPluginReference> = emptyList(),
    /** Explicit frame-processor plugin identities selected by the host. */
    val frameProcessorPluginReferences: List<VesperPluginReference> = emptyList(),
    val maxInFlightFrames: Int? = null,
) {
    internal val isDisabled: Boolean
        get() = mode == VesperNativeFramePipelineMode.Disabled

    internal val modeOrdinal: Int
        get() = when (mode) {
            VesperNativeFramePipelineMode.Disabled -> 0
            VesperNativeFramePipelineMode.DiagnosticsOnly -> 1
            VesperNativeFramePipelineMode.PreferNativeFrame -> 2
            VesperNativeFramePipelineMode.RequireNativeFrame -> 3
        }

    internal val modeWireName: String
        get() = when (mode) {
            VesperNativeFramePipelineMode.Disabled -> "disabled"
            VesperNativeFramePipelineMode.DiagnosticsOnly -> "diagnosticsOnly"
            VesperNativeFramePipelineMode.PreferNativeFrame -> "preferNativeFrame"
            VesperNativeFramePipelineMode.RequireNativeFrame -> "requireNativeFrame"
    }
}

/**
 * Selects the native playback event hooks used by one Android host kit.
 *
 * Android playback only accepts explicitly selected native references. WASM
 * event hooks are desktop/tooling capabilities and are rejected by the native
 * Android registry during session creation.
 */
data class VesperPipelineEventHookConfiguration(
    val pluginReferences: List<VesperPluginReference> = emptyList(),
)

internal fun parsePluginDiagnosticsJson(json: String?): List<Map<String, Any?>> {
    if (json.isNullOrBlank()) {
        return emptyList()
    }
    return runCatching {
        val array = JSONArray(json)
        List(array.length()) { index ->
            jsonObjectToMap(array.getJSONObject(index))
        }
    }.getOrDefault(emptyList())
}

internal fun jsonObjectToMap(value: JSONObject): Map<String, Any?> {
    val result = linkedMapOf<String, Any?>()
    val keys = value.keys()
    while (keys.hasNext()) {
        val key = keys.next()
        result[key] = jsonValueToKotlin(value.opt(key))
    }
    return result
}

private fun jsonArrayToList(value: JSONArray): List<Any?> =
    List(value.length()) { index -> jsonValueToKotlin(value.opt(index)) }

private fun jsonValueToKotlin(value: Any?): Any? =
    when (value) {
        null, JSONObject.NULL -> null
        is JSONObject -> jsonObjectToMap(value)
        is JSONArray -> jsonArrayToList(value)
        else -> value
    }
