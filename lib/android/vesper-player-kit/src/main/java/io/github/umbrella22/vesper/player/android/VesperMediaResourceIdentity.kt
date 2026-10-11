package io.github.umbrella22.vesper.player.android

import androidx.media3.datasource.DataSpec
import java.security.MessageDigest
import java.util.Locale

/** Exact locators and transport properties define cache identity; log descriptions never participate. */
internal fun mediaResourceCacheKey(
    namespace: String,
    uri: String,
    headers: Map<String, String>,
    httpMethod: Int = DataSpec.HTTP_METHOD_GET,
    httpBody: ByteArray? = null,
    requestHeaders: Map<String, String> = emptyMap(),
): String {
    val digest = MessageDigest.getInstance("SHA-256")
    fun field(bytes: ByteArray) {
        digest.update(bytes.size.toString().toByteArray(Charsets.US_ASCII))
        digest.update(0.toByte())
        digest.update(bytes)
    }
    fun text(value: String) = field(value.toByteArray(Charsets.UTF_8))
    fun headerFields(properties: Map<String, String>) {
        text(properties.size.toString())
        properties.forEach { (name, value) ->
            text(name)
            text(value)
        }
    }
    text("vesper-media-resource-v2")
    text(namespace)
    text(uri)
    text(httpMethod.toString())
    // Media3 replaces exact-spelling properties, not case-insensitive HTTP field names.
    val properties = headers + requestHeaders
    headerFields(properties.toSortedMap())
    val hasCaseAliases = hasConflictingResourceHeaderNames(properties)
    text(if (hasCaseAliases) "case-alias-inputs" else "distinct-header-names")
    if (hasCaseAliases) {
        // Conflicting spellings can make transport output depend on input order and origin.
        headerFields(headers)
        headerFields(requestHeaders)
    }
    text(if (httpBody == null) "no-body" else "body")
    if (httpBody != null) field(httpBody)
    return "v2." + digest.digest().joinToString("") { "%02x".format(it) }
}

internal fun hasConflictingResourceHeaderNames(properties: Map<String, String>): Boolean =
    properties.keys.distinctBy { it.lowercase(Locale.ROOT) }.size != properties.size
