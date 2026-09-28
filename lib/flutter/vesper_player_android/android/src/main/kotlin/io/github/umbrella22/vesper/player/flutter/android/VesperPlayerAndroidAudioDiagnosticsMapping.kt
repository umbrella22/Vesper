package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperAudioDecoderCapabilityRequest
import io.github.umbrella22.vesper.player.android.VesperPlaybackStallPolicy

private fun Map<String, Any?>.positiveDiagnosticLong(key: String): Long? {
    val value = this[key] ?: return null
    require(value is Int || value is Long) { "$key must be an integer" }
    return (value as Number).toLong().also { require(it > 0) { "$key must be positive" } }
}

private fun Map<String, Any?>.positiveDiagnosticInt(key: String): Int? =
    positiveDiagnosticLong(key)?.also { require(it <= Int.MAX_VALUE) { "$key exceeds the platform integer range" } }?.toInt()

private fun Map<String, Any?>.diagnosticString(key: String): String? {
    val value = this[key] ?: return null
    require(value is String) { "$key must be a string" }
    return value
}

internal fun Map<String, Any?>.toAudioDecoderCapabilityRequest() = VesperAudioDecoderCapabilityRequest(
    sampleMimeType = diagnosticString("sampleMimeType"), codec = diagnosticString("codec"),
    channels = positiveDiagnosticInt("channels"), sampleRate = positiveDiagnosticInt("sampleRate"),
)

internal fun Map<String, Any?>.toPlaybackStallPolicy(): VesperPlaybackStallPolicy {
    val enabled = this["enabled"] ?: true
    require(enabled is Boolean) { "enabled must be a boolean" }
    return VesperPlaybackStallPolicy(enabled, positiveDiagnosticLong("positionThresholdMs") ?: 5_000,
        positiveDiagnosticLong("bufferingThresholdMs") ?: 15_000)
}
