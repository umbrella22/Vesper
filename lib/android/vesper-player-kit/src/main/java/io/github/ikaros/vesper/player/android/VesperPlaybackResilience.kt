package io.github.ikaros.vesper.player.android

enum class VesperBufferingPreset {
    Default,
    Balanced,
    Streaming,
    Resilient,
    LowLatency,
}

private data class VesperBufferingPresetDefaults(
    val minBufferMs: Int,
    val maxBufferMs: Int,
    val bufferForPlaybackMs: Int,
    val bufferForPlaybackAfterRebufferMs: Int,
)

class VesperBufferingPolicy(
    val preset: VesperBufferingPreset = VesperBufferingPreset.Default,
    minBufferMs: Int? = null,
    maxBufferMs: Int? = null,
    bufferForPlaybackMs: Int? = null,
    bufferForPlaybackAfterRebufferMs: Int? = null,
) {
    private val rawMinBufferMs: Int? = minBufferMs
    private val rawMaxBufferMs: Int? = maxBufferMs
    private val rawBufferForPlaybackMs: Int? = bufferForPlaybackMs
    private val rawBufferForPlaybackAfterRebufferMs: Int? = bufferForPlaybackAfterRebufferMs

    val minBufferMs: Int?
        get() = rawMinBufferMs ?: presetDefaults(preset)?.minBufferMs

    val maxBufferMs: Int?
        get() = rawMaxBufferMs ?: presetDefaults(preset)?.maxBufferMs

    val bufferForPlaybackMs: Int?
        get() = rawBufferForPlaybackMs ?: presetDefaults(preset)?.bufferForPlaybackMs

    val bufferForPlaybackAfterRebufferMs: Int?
        get() =
            rawBufferForPlaybackAfterRebufferMs
                ?: presetDefaults(preset)?.bufferForPlaybackAfterRebufferMs

    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is VesperBufferingPolicy) {
            return false
        }

        return preset == other.preset &&
            minBufferMs == other.minBufferMs &&
            maxBufferMs == other.maxBufferMs &&
            bufferForPlaybackMs == other.bufferForPlaybackMs &&
            bufferForPlaybackAfterRebufferMs == other.bufferForPlaybackAfterRebufferMs
    }

    override fun hashCode(): Int {
        var result = preset.hashCode()
        result = 31 * result + (minBufferMs ?: 0)
        result = 31 * result + (maxBufferMs ?: 0)
        result = 31 * result + (bufferForPlaybackMs ?: 0)
        result = 31 * result + (bufferForPlaybackAfterRebufferMs ?: 0)
        return result
    }

    override fun toString(): String =
        "VesperBufferingPolicy(preset=$preset, minBufferMs=$minBufferMs, " +
            "maxBufferMs=$maxBufferMs, bufferForPlaybackMs=$bufferForPlaybackMs, " +
            "bufferForPlaybackAfterRebufferMs=$bufferForPlaybackAfterRebufferMs)"

    internal fun toNativePayload(): NativeBufferingPolicy =
        NativeBufferingPolicy(
            presetOrdinal = preset.ordinal,
            hasMinBufferMs = rawMinBufferMs != null,
            minBufferMs = rawMinBufferMs ?: 0,
            hasMaxBufferMs = rawMaxBufferMs != null,
            maxBufferMs = rawMaxBufferMs ?: 0,
            hasBufferForPlaybackMs = rawBufferForPlaybackMs != null,
            bufferForPlaybackMs = rawBufferForPlaybackMs ?: 0,
            hasBufferForPlaybackAfterRebufferMs = rawBufferForPlaybackAfterRebufferMs != null,
            bufferForPlaybackAfterRebufferMs = rawBufferForPlaybackAfterRebufferMs ?: 0,
        )

    companion object {
        private fun presetDefaults(
            preset: VesperBufferingPreset,
        ): VesperBufferingPresetDefaults? =
            when (preset) {
                VesperBufferingPreset.Default -> null
                VesperBufferingPreset.Balanced ->
                    VesperBufferingPresetDefaults(
                        minBufferMs = 10_000,
                        maxBufferMs = 30_000,
                        bufferForPlaybackMs = 1_000,
                        bufferForPlaybackAfterRebufferMs = 2_000,
                    )
                VesperBufferingPreset.Streaming ->
                    VesperBufferingPresetDefaults(
                        minBufferMs = 12_000,
                        maxBufferMs = 36_000,
                        bufferForPlaybackMs = 1_200,
                        bufferForPlaybackAfterRebufferMs = 2_500,
                    )
                VesperBufferingPreset.Resilient ->
                    VesperBufferingPresetDefaults(
                        minBufferMs = 20_000,
                        maxBufferMs = 50_000,
                        bufferForPlaybackMs = 1_500,
                        bufferForPlaybackAfterRebufferMs = 3_000,
                    )
                VesperBufferingPreset.LowLatency ->
                    VesperBufferingPresetDefaults(
                        minBufferMs = 4_000,
                        maxBufferMs = 12_000,
                        bufferForPlaybackMs = 500,
                        bufferForPlaybackAfterRebufferMs = 1_000,
                    )
            }

        fun balanced(): VesperBufferingPolicy =
            VesperBufferingPolicy(preset = VesperBufferingPreset.Balanced)

        fun streaming(): VesperBufferingPolicy =
            VesperBufferingPolicy(preset = VesperBufferingPreset.Streaming)

        fun resilient(): VesperBufferingPolicy =
            VesperBufferingPolicy(preset = VesperBufferingPreset.Resilient)

        fun lowLatency(): VesperBufferingPolicy =
            VesperBufferingPolicy(preset = VesperBufferingPreset.LowLatency)
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
    maxAttempts: Int? = 3,
    baseDelayMs: Long? = null,
    maxDelayMs: Long? = null,
    backoff: VesperRetryBackoff? = null,
) {
    private val hasMaxAttemptsOverride: Boolean = maxAttempts != 3
    private val rawMaxAttempts: Int? = if (hasMaxAttemptsOverride) maxAttempts else null
    private val rawBaseDelayMs: Long? = baseDelayMs
    private val rawMaxDelayMs: Long? = maxDelayMs
    private val rawBackoff: VesperRetryBackoff? = backoff

    val maxAttempts: Int?
        get() = if (hasMaxAttemptsOverride) rawMaxAttempts else 3

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
            hasMaxAttemptsOverride == other.hasMaxAttemptsOverride &&
            rawMaxAttempts == other.rawMaxAttempts &&
            rawBaseDelayMs == other.rawBaseDelayMs &&
            rawMaxDelayMs == other.rawMaxDelayMs &&
            rawBackoff == other.rawBackoff
    }

    override fun hashCode(): Int {
        var result = maxAttempts ?: 0
        result = 31 * result + hasMaxAttemptsOverride.hashCode()
        result = 31 * result + (rawMaxAttempts ?: 0)
        result = 31 * result + (rawBaseDelayMs?.hashCode() ?: 0)
        result = 31 * result + (rawMaxDelayMs?.hashCode() ?: 0)
        result = 31 * result + (rawBackoff?.hashCode() ?: 0)
        return result
    }

    override fun toString(): String =
        "VesperRetryPolicy(maxAttempts=$maxAttempts, baseDelayMs=$rawBaseDelayMs, " +
            "maxDelayMs=$rawMaxDelayMs, backoff=$rawBackoff)"

    internal fun toNativePayload(): NativeRetryPolicy =
        NativeRetryPolicy(
            usesDefaultMaxAttempts = !hasMaxAttemptsOverride,
            hasMaxAttempts = rawMaxAttempts != null,
            maxAttempts = rawMaxAttempts ?: 0,
            hasBaseDelayMs = rawBaseDelayMs != null,
            baseDelayMs = rawBaseDelayMs ?: 0L,
            hasMaxDelayMs = rawMaxDelayMs != null,
            maxDelayMs = rawMaxDelayMs ?: 0L,
            hasBackoff = rawBackoff != null,
            backoffOrdinal = rawBackoff?.ordinal ?: VesperRetryBackoff.Linear.ordinal,
        )

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

private data class VesperCachePresetDefaults(
    val maxMemoryBytes: Long,
    val maxDiskBytes: Long,
)

class VesperCachePolicy(
    val preset: VesperCachePreset = VesperCachePreset.Default,
    maxMemoryBytes: Long? = null,
    maxDiskBytes: Long? = null,
) {
    private val rawMaxMemoryBytes: Long? = maxMemoryBytes
    private val rawMaxDiskBytes: Long? = maxDiskBytes

    val maxMemoryBytes: Long?
        get() = rawMaxMemoryBytes ?: presetDefaults(preset)?.maxMemoryBytes

    val maxDiskBytes: Long?
        get() = rawMaxDiskBytes ?: presetDefaults(preset)?.maxDiskBytes

    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is VesperCachePolicy) {
            return false
        }

        return preset == other.preset &&
            maxMemoryBytes == other.maxMemoryBytes &&
            maxDiskBytes == other.maxDiskBytes
    }

    override fun hashCode(): Int {
        var result = preset.hashCode()
        result = 31 * result + (maxMemoryBytes?.hashCode() ?: 0)
        result = 31 * result + (maxDiskBytes?.hashCode() ?: 0)
        return result
    }

    override fun toString(): String =
        "VesperCachePolicy(preset=$preset, maxMemoryBytes=$maxMemoryBytes, " +
            "maxDiskBytes=$maxDiskBytes)"

    internal fun toNativePayload(): NativeCachePolicy =
        NativeCachePolicy(
            presetOrdinal = preset.ordinal,
            hasMaxMemoryBytes = rawMaxMemoryBytes != null,
            maxMemoryBytes = rawMaxMemoryBytes ?: 0L,
            hasMaxDiskBytes = rawMaxDiskBytes != null,
            maxDiskBytes = rawMaxDiskBytes ?: 0L,
        )

    companion object {
        private fun presetDefaults(preset: VesperCachePreset): VesperCachePresetDefaults? =
            when (preset) {
                VesperCachePreset.Default -> null
                VesperCachePreset.Disabled ->
                    VesperCachePresetDefaults(
                        maxMemoryBytes = 0L,
                        maxDiskBytes = 0L,
                    )
                VesperCachePreset.Streaming ->
                    VesperCachePresetDefaults(
                        maxMemoryBytes = 8L * 1024L * 1024L,
                        maxDiskBytes = 128L * 1024L * 1024L,
                    )
                VesperCachePreset.Resilient ->
                    VesperCachePresetDefaults(
                        maxMemoryBytes = 16L * 1024L * 1024L,
                        maxDiskBytes = 384L * 1024L * 1024L,
                    )
            }

        fun disabled(): VesperCachePolicy =
            VesperCachePolicy(preset = VesperCachePreset.Disabled)

        fun streaming(): VesperCachePolicy =
            VesperCachePolicy(preset = VesperCachePreset.Streaming)

        fun resilient(): VesperCachePolicy =
            VesperCachePolicy(preset = VesperCachePreset.Resilient)
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

data class VesperPreloadBudgetPolicy(
    val maxConcurrentTasks: Int? = null,
    val maxMemoryBytes: Long? = null,
    val maxDiskBytes: Long? = null,
    val warmupWindowMs: Long? = null,
)
