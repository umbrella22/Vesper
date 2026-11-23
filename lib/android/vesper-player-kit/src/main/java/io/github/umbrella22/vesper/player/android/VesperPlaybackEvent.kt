package io.github.umbrella22.vesper.player.android

/** Observations from the player, expressed in the controller's timeline coordinates. */
sealed interface VesperPlaybackEvent {
    /** Media3 reached readiness after applying the target. */
    data class SeekCompleted(val targetPositionMs: Long, val positionMs: Long) : VesperPlaybackEvent
    /** Media3 redirected the seek to [positionMs]. [targetPositionMs] includes timeline clamping. */
    data class SeekAdjusted(val targetPositionMs: Long, val positionMs: Long) : VesperPlaybackEvent
    data class SeekFailed(val error: VesperPlayerErrorState) : VesperPlaybackEvent
    data class RetryScheduled(val attempt: Int, val delayMs: Long) : VesperPlaybackEvent
    data class LiveWindowRecoveryStarted(val attempt: Int, val maxAttempts: Int) : VesperPlaybackEvent
    /** The timeline position sampled when Media3 returned to ready after recovery. */
    data class LiveWindowRecovered(val attempt: Int, val positionMs: Long) : VesperPlaybackEvent
}
