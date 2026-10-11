package io.github.umbrella22.vesper.player.flutter.android

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
    return arguments.optionalInputMap(key) ?: throw IllegalArgumentException("Missing $key.")
}

internal fun Map<String, Any?>.optionalInputMap(key: String): Map<String, Any?>? {
    val value = this[key] ?: return null
    require(value is Map<*, *>) { "$key must be a map." }
    require(value.keys.all { it is String }) { "$key keys must be strings." }
    return value.stringMap()
}

internal fun Map<String, Any?>.decodeInputMapList(key: String): List<Map<String, Any?>> {
    val value = this[key] ?: return emptyList()
    require(value is List<*>) { "$key must be a list." }
    return value.mapIndexed { index, entry ->
        require(entry is Map<*, *>) { "$key[$index] must be a map." }
        require(entry.keys.all { it is String }) { "$key[$index] keys must be strings." }
        entry.stringMap()
    }
}

internal fun Map<String, Any?>.decodeInputStringList(key: String): List<String> {
    val value = this[key] ?: return emptyList()
    require(value is List<*>) { "$key must be a list." }
    return value.mapIndexed { index, entry ->
        require(entry is String) { "$key[$index] must be a string." }
        entry
    }
}
