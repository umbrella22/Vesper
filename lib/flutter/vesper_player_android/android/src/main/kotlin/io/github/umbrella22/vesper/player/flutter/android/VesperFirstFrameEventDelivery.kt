package io.github.umbrella22.vesper.player.flutter.android

import io.flutter.plugin.common.EventChannel
import io.github.umbrella22.vesper.player.android.VesperFirstFrameObservation

/** One retained observation may be delivered after the channel resumes listening. */
internal class VesperFirstFrameEventDelivery {
    private var lastDeliveredEpoch: Long? = null

    fun deliver(playerId: String, observation: VesperFirstFrameObservation?, sink: EventChannel.EventSink?) {
        if (sink == null || observation == null || lastDeliveredEpoch == observation.playbackEpoch) return
        lastDeliveredEpoch = observation.playbackEpoch
        sink.success(mapOf(
            "playerId" to playerId, "type" to "firstFrame",
            "observation" to observation.toMap(),
        ))
    }
}
