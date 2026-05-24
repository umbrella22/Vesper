package io.github.ikaros.vesper.player.flutter.android

import io.flutter.plugin.common.MethodCall

internal fun MethodCall.argumentMap(): Map<String, Any?> =
    (arguments as? Map<*, *>)?.stringMap() ?: emptyMap()

internal fun Map<*, *>.stringMap(): Map<String, Any?> =
    entries.associate { (key, value) -> key.toString() to value }

internal fun Any?.stringStringMap(): Map<String, String> {
    val raw = this as? Map<*, *> ?: return emptyMap()
    return raw.entries.mapNotNull { (key, value) ->
        val stringKey = key as? String ?: return@mapNotNull null
        val stringValue = value as? String ?: return@mapNotNull null
        stringKey to stringValue
    }.toMap()
}

internal fun requireNestedMap(
    arguments: Map<String, Any?>,
    key: String,
): Map<String, Any?> {
    val raw = arguments[key] as? Map<*, *>
    return raw?.stringMap() ?: throw IllegalArgumentException("Missing $key.")
}

