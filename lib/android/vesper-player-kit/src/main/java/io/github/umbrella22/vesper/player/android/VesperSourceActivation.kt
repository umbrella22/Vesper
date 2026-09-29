package io.github.umbrella22.vesper.player.android

/** Initial playback intent belongs to this activation, not to a previous source. */
data class VesperSourceActivationOptions(
    val playWhenReady: Boolean = true,
    val startPositionMs: Long = 0,
    val playbackRate: Float = 1f,
    val timeoutMs: Long = 30_000,
) {
    init {
        require(startPositionMs >= 0)
        require(playbackRate.isFinite() && playbackRate > 0)
        require(timeoutMs in 1..60_000)
    }
}

data class VesperSourceActivationResult(
    val activationId: String,
    val sessionId: String,
    val sourceId: String,
    val playbackEpoch: Long,
) {
    fun toWireMap(): Map<String, Any?> = mapOf("activationId" to activationId, "sessionId" to sessionId,
        "sourceId" to sourceId, "playbackEpoch" to playbackEpoch)
}
