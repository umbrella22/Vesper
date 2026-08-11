package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceCacheIdentity
import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceConfiguration
import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceContentIdentity
import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceItem
import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceMediaKind
import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceMode
import io.github.ikaros.vesper.player.android.VesperPlaybackSequencePreloadProfile
import io.github.ikaros.vesper.player.android.VesperPlaybackSequenceResolvedSource

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
        historyLimit = (this["historyLimit"] as? Number)?.toInt() ?: 16,
        forwardWindow = (this["forwardWindow"] as? Number)?.toInt() ?: 1,
        refillThreshold = (this["refillThreshold"] as? Number)?.toInt() ?: 1,
        maxItems = (this["maxItems"] as? Number)?.toInt() ?: 512,
        maxPendingRequests = (this["maxPendingRequests"] as? Number)?.toInt() ?: 32,
        maxEvents = (this["maxEvents"] as? Number)?.toInt() ?: 512,
        requestTimeoutMs = (this["requestTimeoutMs"] as? Number)?.toLong() ?: 15_000,
        sourceExpiryLeadMs = (this["sourceExpiryLeadMs"] as? Number)?.toLong() ?: 15_000,
        maxSourceRegistryEntries =
            (this["maxSourceRegistryEntries"] as? Number)?.toInt() ?: 1_024,
    )

internal fun Map<String, Any?>.toPlaybackSequenceItem(): VesperPlaybackSequenceItem {
    val cacheMap = (this["cacheIdentity"] as? Map<*, *>)?.stringMap()
    val sourceMap = (this["source"] as? Map<*, *>)?.stringMap()
    val cache = cacheMap?.toPlaybackSequenceCacheIdentity()
    val source = sourceMap?.toVesperPlayerSource()
    require((cache == null) == (source == null)) {
        "sequence source and cacheIdentity must be supplied together"
    }
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
        cacheIdentity = cache,
        sourceRevision = (this["sourceRevision"] as? Number)?.toLong()
            ?: cache?.sourceRevision ?: 0,
        expiresAtEpochMs = (this["expiresAtEpochMs"] as? Number)?.toLong(),
        providerMetadataRef = this["providerMetadataRef"] as? String,
        preloadProfile = (this["preloadProfile"] as? Map<*, *>)
            ?.stringMap()?.toPlaybackSequencePreloadProfile()
            ?: VesperPlaybackSequencePreloadProfile(),
    )
}

private fun Map<String, Any?>.toPlaybackSequenceCacheIdentity() =
    VesperPlaybackSequenceCacheIdentity(
        providerNamespace = this["providerNamespace"] as? String ?: "",
        contentIdentity = this["contentIdentity"] as? String ?: "",
        renditionIdentity = this["renditionIdentity"] as? String ?: "",
        resourceIdentity = this["resourceIdentity"] as? String ?: "",
        accessPartition = this["accessPartition"] as? String ?: "",
        sourceRevision = (this["sourceRevision"] as? Number)?.toLong() ?: 0,
    )

private fun Map<String, Any?>.toPlaybackSequencePreloadProfile() =
    VesperPlaybackSequencePreloadProfile(
        expectedMemoryBytes = (this["expectedMemoryBytes"] as? Number)?.toLong() ?: 0,
        expectedDiskBytes = (this["expectedDiskBytes"] as? Number)?.toLong() ?: 0,
        ttlMs = (this["ttlMs"] as? Number)?.toLong(),
        warmupWindowMs = (this["warmupWindowMs"] as? Number)?.toLong(),
    )

internal fun Map<String, Any?>.toPlaybackSequenceResolvedSource():
    VesperPlaybackSequenceResolvedSource {
    val cache = requireNestedMap(this, "cacheIdentity").toPlaybackSequenceCacheIdentity()
    return VesperPlaybackSequenceResolvedSource(
        sessionGeneration = (this["sessionGeneration"] as? Number)?.toLong() ?: 0,
        requestId = (this["requestId"] as? Number)?.toLong() ?: 0,
        resolutionAttemptId = (this["resolutionAttemptId"] as? Number)?.toLong() ?: 0,
        itemId = this["itemId"] as? String ?: "",
        expectedSourceRevision =
            (this["expectedSourceRevision"] as? Number)?.toLong() ?: 0,
        sourceRevision = (this["sourceRevision"] as? Number)?.toLong() ?: 0,
        source = requireNestedMap(this, "source").toVesperPlayerSource(),
        cacheIdentity = cache,
        expiresAtEpochMs = (this["expiresAtEpochMs"] as? Number)?.toLong(),
    )
}
