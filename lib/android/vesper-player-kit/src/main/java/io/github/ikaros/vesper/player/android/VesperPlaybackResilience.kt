package io.github.ikaros.vesper.player.android

enum class VesperBufferingPreset {
    Default,
    Balanced,
    Streaming,
    Resilient,
    LowLatency,
}

data class VesperBufferingPolicy(
    val preset: VesperBufferingPreset = VesperBufferingPreset.Default,
    val minBufferMs: Int? = null,
    val maxBufferMs: Int? = null,
    val bufferForPlaybackMs: Int? = null,
    val bufferForPlaybackAfterRebufferMs: Int? = null,
) {
    companion object {
        fun balanced(): VesperBufferingPolicy =
            VesperBufferingPolicy(
                preset = VesperBufferingPreset.Balanced,
                minBufferMs = 10_000,
                maxBufferMs = 30_000,
                bufferForPlaybackMs = 1_000,
                bufferForPlaybackAfterRebufferMs = 2_000,
            )

        fun streaming(): VesperBufferingPolicy =
            VesperBufferingPolicy(
                preset = VesperBufferingPreset.Streaming,
                minBufferMs = 12_000,
                maxBufferMs = 36_000,
                bufferForPlaybackMs = 1_200,
                bufferForPlaybackAfterRebufferMs = 2_500,
            )

        fun resilient(): VesperBufferingPolicy =
            VesperBufferingPolicy(
                preset = VesperBufferingPreset.Resilient,
                minBufferMs = 20_000,
                maxBufferMs = 50_000,
                bufferForPlaybackMs = 1_500,
                bufferForPlaybackAfterRebufferMs = 3_000,
            )

        fun lowLatency(): VesperBufferingPolicy =
            VesperBufferingPolicy(
                preset = VesperBufferingPreset.LowLatency,
                minBufferMs = 4_000,
                maxBufferMs = 12_000,
                bufferForPlaybackMs = 500,
                bufferForPlaybackAfterRebufferMs = 1_000,
            )
    }
}

enum class VesperRetryBackoff {
    Fixed,
    Linear,
    Exponential,
}

enum class VesperCachePreset {
    Default,
    Disabled,
    Streaming,
    Resilient,
}

class VesperRetryPolicy(
    val maxAttempts: Int? = 3,
    baseDelayMs: Long? = null,
    maxDelayMs: Long? = null,
    backoff: VesperRetryBackoff? = null,
) {
    private val rawBaseDelayMs: Long? = baseDelayMs
    private val rawMaxDelayMs: Long? = maxDelayMs
    private val rawBackoff: VesperRetryBackoff? = backoff

    val baseDelayMs: Long
        get() = rawBaseDelayMs ?: 1_000L

    val maxDelayMs: Long
        get() = rawMaxDelayMs ?: 5_000L

    val backoff: VesperRetryBackoff
        get() = rawBackoff ?: VesperRetryBackoff.Linear

    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is VesperRetryPolicy) {
            return false
        }

        return maxAttempts == other.maxAttempts &&
            rawBaseDelayMs == other.rawBaseDelayMs &&
            rawMaxDelayMs == other.rawMaxDelayMs &&
            rawBackoff == other.rawBackoff
    }

    override fun hashCode(): Int {
        var result = maxAttempts ?: 0
        result = 31 * result + (rawBaseDelayMs?.hashCode() ?: 0)
        result = 31 * result + (rawMaxDelayMs?.hashCode() ?: 0)
        result = 31 * result + (rawBackoff?.hashCode() ?: 0)
        return result
    }

    override fun toString(): String =
        "VesperRetryPolicy(maxAttempts=$maxAttempts, baseDelayMs=$rawBaseDelayMs, " +
            "maxDelayMs=$rawMaxDelayMs, backoff=$rawBackoff)"

    companion object {
        fun aggressive(): VesperRetryPolicy =
            VesperRetryPolicy(
                maxAttempts = 2,
                baseDelayMs = 500L,
                maxDelayMs = 2_000L,
                backoff = VesperRetryBackoff.Fixed,
            )

        fun resilient(): VesperRetryPolicy =
            VesperRetryPolicy(
                maxAttempts = 6,
                baseDelayMs = 1_000L,
                maxDelayMs = 8_000L,
                backoff = VesperRetryBackoff.Exponential,
            )
    }
}

data class VesperCachePolicy(
    val preset: VesperCachePreset = VesperCachePreset.Default,
    val maxMemoryBytes: Long? = null,
    val maxDiskBytes: Long? = null,
) {
    companion object {
        fun disabled(): VesperCachePolicy =
            VesperCachePolicy(
                preset = VesperCachePreset.Disabled,
                maxMemoryBytes = 0L,
                maxDiskBytes = 0L,
            )

        fun streaming(): VesperCachePolicy =
            VesperCachePolicy(
                preset = VesperCachePreset.Streaming,
                maxMemoryBytes = 8L * 1024L * 1024L,
                maxDiskBytes = 128L * 1024L * 1024L,
            )

        fun resilient(): VesperCachePolicy =
            VesperCachePolicy(
                preset = VesperCachePreset.Resilient,
                maxMemoryBytes = 16L * 1024L * 1024L,
                maxDiskBytes = 384L * 1024L * 1024L,
            )
    }
}

data class VesperPlaybackResiliencePolicy(
    val buffering: VesperBufferingPolicy = VesperBufferingPolicy(),
    val retry: VesperRetryPolicy = VesperRetryPolicy(),
    val cache: VesperCachePolicy = VesperCachePolicy(),
) {
    companion object {
        fun balanced(): VesperPlaybackResiliencePolicy =
            VesperPlaybackResiliencePolicy(
                buffering = VesperBufferingPolicy.balanced(),
                retry = VesperRetryPolicy(),
                cache = VesperCachePolicy.streaming(),
            )

        fun streaming(): VesperPlaybackResiliencePolicy =
            VesperPlaybackResiliencePolicy(
                buffering = VesperBufferingPolicy.streaming(),
                retry = VesperRetryPolicy(),
                cache = VesperCachePolicy.streaming(),
            )

        fun resilient(): VesperPlaybackResiliencePolicy =
            VesperPlaybackResiliencePolicy(
                buffering = VesperBufferingPolicy.resilient(),
                retry = VesperRetryPolicy.resilient(),
                cache = VesperCachePolicy.resilient(),
            )

        fun lowLatency(): VesperPlaybackResiliencePolicy =
            VesperPlaybackResiliencePolicy(
                buffering = VesperBufferingPolicy.lowLatency(),
                retry = VesperRetryPolicy.aggressive(),
                cache = VesperCachePolicy.disabled(),
            )
    }
}
