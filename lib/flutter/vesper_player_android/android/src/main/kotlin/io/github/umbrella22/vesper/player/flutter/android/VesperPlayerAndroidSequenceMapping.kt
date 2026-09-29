package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperSourceHandle
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceConfiguration
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceContentIdentity
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceItem
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceMediaKind
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceMode
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequencePreloadProfile
import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceSourceRequest

internal fun Map<String, Any?>.toPlaybackSequenceConfiguration():
    VesperPlaybackSequenceConfiguration =
    VesperPlaybackSequenceConfiguration(
        sequenceId = this["sequenceId"] as? String
            ?: throw IllegalArgumentException("Missing sequenceId."),
        mode = when (this["mode"] as? String) {
            "finite" -> VesperPlaybackSequenceMode.Finite
            "replenishable" -> VesperPlaybackSequenceMode.Replenishable
            else -> throw IllegalArgumentException("Unknown sequence mode.")
        },
        historyLimit = int("historyLimit", 16),
        forwardWindow = int("forwardWindow", 1),
        refillThreshold = int("refillThreshold", 1),
        maxItems = int("maxItems", 512),
        maxPendingRequests = int("maxPendingRequests", 32),
        maxEvents = int("maxEvents", 512),
        requestTimeoutMs = sourceLong("requestTimeoutMs", 15_000),
        sourceExpiryLeadMs = sourceLong("sourceExpiryLeadMs", 15_000),
        maxSourceRegistryEntries =
            int("maxSourceRegistryEntries", 1_024),
    )

internal fun Map<String, Any?>.toPlaybackSequenceItem(resolve: (Map<String, Any?>) -> VesperSourceHandle): VesperPlaybackSequenceItem {
    val source = (this["source"] as? Map<*, *>)?.stringMap()?.let(resolve)
    return VesperPlaybackSequenceItem(
        itemId = this["itemId"] as? String
            ?: throw IllegalArgumentException("Missing sequence itemId."),
        contentIdentity = VesperPlaybackSequenceContentIdentity(
            providerNamespace = this["providerNamespace"] as? String ?: "",
            value = this["contentIdentity"] as? String ?: "",
        ),
        mediaKind = when (this["mediaKind"] as? String) {
            "vod", null -> VesperPlaybackSequenceMediaKind.Vod
            "live" -> VesperPlaybackSequenceMediaKind.Live
            "liveDvr" -> VesperPlaybackSequenceMediaKind.LiveDvr
            else -> throw IllegalArgumentException("Unknown sequence media kind.")
        },
        source = source,
        providerMetadataRef = this["providerMetadataRef"] as? String,
        preloadProfile = (this["preloadProfile"] as? Map<*, *>)
            ?.stringMap()?.toPlaybackSequencePreloadProfile()
            ?: VesperPlaybackSequencePreloadProfile(),
    )
}

private fun Map<String, Any?>.toPlaybackSequencePreloadProfile() =
    VesperPlaybackSequencePreloadProfile(
        expectedMemoryBytes = sourceLong("expectedMemoryBytes", 0),
        expectedDiskBytes = sourceLong("expectedDiskBytes", 0),
        ttlMs = this["ttlMs"]?.let { sourceLong("ttlMs", 0) },
        warmupWindowMs = this["warmupWindowMs"]?.let { sourceLong("warmupWindowMs", 0) },
    )

internal fun Map<String, Any?>.toPlaybackSequenceResolvedSource(resolve: (Map<String, Any?>) -> VesperSourceHandle):
    Pair<VesperPlaybackSequenceSourceRequest, VesperSourceHandle> =
    VesperPlaybackSequenceSourceRequest.fromWireMap(this) to resolve(requireNestedMap(this, "source"))
