package io.github.umbrella22.vesper.player.android

/** Reports at most one suspected playback stall per native load attempt. */
data class VesperPlaybackStallPolicy(
    val enabled: Boolean = true,
    val positionThresholdMs: Long = 5_000,
    val bufferingThresholdMs: Long = 15_000,
) {
    init {
        require(positionThresholdMs > 0) { "positionThresholdMs must be positive" }
        require(bufferingThresholdMs > 0) { "bufferingThresholdMs must be positive" }
    }
}

enum class VesperPlaybackStallKind(val wireName: String) {
    PositionNotAdvancing("positionNotAdvancing"), BufferingTimeout("bufferingTimeout"),
}

/** Media-clock evidence only; this does not identify an audio decoder or sink failure. */
data class VesperPlaybackStallObservation(
    val playbackEpoch: Long,
    val kind: VesperPlaybackStallKind,
    val stalledForMs: Long,
    val elapsedSinceLoadStartMs: Long,
    val mediaPositionMs: Long,
    val audio: VesperAudioPlaybackDiagnostics,
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "playbackEpoch" to playbackEpoch, "kind" to kind.wireName,
        "stalledForMs" to stalledForMs, "elapsedSinceLoadStartMs" to elapsedSinceLoadStartMs,
        "mediaPositionMs" to mediaPositionMs, "audio" to audio.toMap(),
    )
}

internal data class PlaybackStallEvidence(val kind: VesperPlaybackStallKind, val durationMs: Long)

/** Constant-space observer driven by an independent monotonic timer. */
internal class VesperPlaybackStallDetector {
    var policy = VesperPlaybackStallPolicy()
        set(value) { field = value; resetWindow() }
    private var lastSampleMs: Long? = null
    private var lastPositionMs: Long? = null
    private var windowStartMs: Long? = null
    private var windowKind: VesperPlaybackStallKind? = null
    private var hasProgressed = false
    private var reported = false

    fun resetAttempt() { reported = false; resetWindow() }

    fun resetWindow() {
        lastSampleMs = null
        lastPositionMs = null
        windowStartMs = null
        windowKind = null
        hasProgressed = false
    }

    fun sample(nowMs: Long, positionMs: Long?, eligible: Boolean, buffering: Boolean): PlaybackStallEvidence? {
        if (!policy.enabled || !eligible || positionMs == null || positionMs < 0) {
            resetWindow()
            return null
        }
        val previousSample = lastSampleMs
        // An unobserved interval (suspension or a blocked main thread) cannot
        // establish continuous stalling. Re-arm from actual progress afterward.
        if (previousSample != null && (nowMs < previousSample || nowMs - previousSample > MAX_SAMPLE_GAP_MS)) {
            resetWindow()
        }
        lastSampleMs = nowMs
        val previousPosition = lastPositionMs
        lastPositionMs = positionMs
        val kind = if (buffering) VesperPlaybackStallKind.BufferingTimeout else VesperPlaybackStallKind.PositionNotAdvancing
        if (previousPosition == null || positionMs < previousPosition) {
            hasProgressed = false
            windowStartMs = nowMs
            windowKind = kind
            return null
        }
        if (positionMs > previousPosition) {
            hasProgressed = true
            windowStartMs = nowMs
            windowKind = kind
            return null
        }
        if (windowKind != kind) {
            windowKind = kind
            windowStartMs = nowMs
        }
        if (!hasProgressed || reported) return null
        val duration = nowMs - (windowStartMs ?: nowMs)
        val threshold = if (buffering) policy.bufferingThresholdMs else policy.positionThresholdMs
        if (duration < threshold) return null
        reported = true
        return PlaybackStallEvidence(kind, duration)
    }

    companion object {
        const val SAMPLE_INTERVAL_MS = 1_000L
        const val MAX_SAMPLE_GAP_MS = 3 * SAMPLE_INTERVAL_MS
    }
}
