package io.github.ikaros.vesper.player.android

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
    val pluginLibraryPaths: List<String> = emptyList(),
    val runtimeProfile: String? = null,
) {
    internal val isDisabled: Boolean
        get() = mode == VesperSourceNormalizerMode.Disabled && pluginLibraryPaths.isEmpty()

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

data class VesperFrameProcessorConfiguration(
    val mode: VesperFrameProcessorMode = VesperFrameProcessorMode.Disabled,
    val pluginLibraryPaths: List<String> = emptyList(),
) {
    internal val isDisabled: Boolean
        get() = mode == VesperFrameProcessorMode.Disabled && pluginLibraryPaths.isEmpty()

    internal val modeOrdinal: Int
        get() = when (mode) {
            VesperFrameProcessorMode.Disabled -> 0
            VesperFrameProcessorMode.DiagnosticsOnly -> 1
        }
}

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
