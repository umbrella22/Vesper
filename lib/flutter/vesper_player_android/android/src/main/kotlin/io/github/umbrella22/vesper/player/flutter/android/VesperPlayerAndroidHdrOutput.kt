package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperHdrOutputSnapshot

internal fun VesperHdrOutputSnapshot?.toFlutterMap(playerId: String): Map<String, Any?> {
    if (this == null) return mapOf(
        "state" to "unknown", "playerId" to playerId, "reason" to "outputObservationUnavailable",
    )
    return mapOf(
        "state" to state.wireName,
        "format" to format.wireName,
        "playerId" to playerId,
        "sourceRevision" to sourceRevision,
        "outputGeneration" to outputGeneration,
        "effectiveVideoTrackId" to effectiveVideoTrackId,
        "catalogRevision" to catalogRevision,
        "displayId" to displayId,
        "evidence" to evidence,
        "reason" to reason,
    )
}
