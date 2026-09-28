package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VesperNativeJniBindingsPlayerPolicyTest {
    @Test
    fun bufferingMinimumIncludesBothPlaybackThresholds() {
        val policy = VesperBufferingPolicy(
            minBufferMs = 1_000,
            maxBufferMs = 50_000,
            bufferForPlaybackMs = 1_500,
            bufferForPlaybackAfterRebufferMs = 3_000,
        )

        assertEquals(
            ResolvedBufferingPolicy(3_000, 50_000, 1_500, 3_000),
            resolveBufferingPolicy(policy.toNativePayload()),
        )
        // Exercise Media3's actual constructor preconditions as well.
        buildLoadControl(policy.toNativePayload())
    }

    @Test
    fun bufferingMaximumIncludesTheResolvedMinimum() {
        val policy = VesperBufferingPolicy(
            minBufferMs = 1_000,
            maxBufferMs = 2_000,
            bufferForPlaybackMs = 5_000,
            bufferForPlaybackAfterRebufferMs = 3_000,
        )

        assertEquals(
            ResolvedBufferingPolicy(5_000, 5_000, 5_000, 3_000),
            resolveBufferingPolicy(policy.toNativePayload()),
        )
    }

    @Test
    fun incompleteBufferingPolicyUsesMedia3Defaults() {
        for (missing in 0..3) {
            val policy = VesperBufferingPolicy(
                minBufferMs = 4_000.takeUnless { missing == 0 },
                maxBufferMs = 12_000.takeUnless { missing == 1 },
                bufferForPlaybackMs = 500.takeUnless { missing == 2 },
                bufferForPlaybackAfterRebufferMs = 1_000.takeUnless { missing == 3 },
            )
            assertNull(resolveBufferingPolicy(policy.toNativePayload()))
        }
    }

    @Test
    fun zeroExponentialDelayRemainsZeroForUnlimitedRetries() {
        assertEquals(0L, retryDelay(0, 8_000, VesperRetryBackoff.Exponential, 1_025))
        assertEquals(0L, retryDelay(0, Long.MAX_VALUE, VesperRetryBackoff.Exponential, Int.MAX_VALUE))
    }

    @Test
    fun retryDelaySaturatesAtTheConfiguredMaximum() {
        assertEquals(8_000L, retryDelay(1_000, 8_000, VesperRetryBackoff.Exponential, 70))
        assertEquals(5_000L, retryDelay(Long.MAX_VALUE, 5_000, VesperRetryBackoff.Fixed, 1))
        assertEquals(Long.MAX_VALUE, retryDelay(Long.MAX_VALUE / 2 + 1, Long.MAX_VALUE, VesperRetryBackoff.Linear, 2))
        assertEquals(0L, retryDelay(1_000, 0, VesperRetryBackoff.Exponential, Int.MAX_VALUE))
    }

    @Test
    fun retryDelayPreservesLargeRepresentableValues() {
        assertEquals(Long.MAX_VALUE - 1, retryDelay(Long.MAX_VALUE - 1, Long.MAX_VALUE, VesperRetryBackoff.Fixed, 1))
        assertEquals(1L shl 62, retryDelay(1, Long.MAX_VALUE, VesperRetryBackoff.Exponential, 63))
        assertEquals(Long.MAX_VALUE, retryDelay(1, Long.MAX_VALUE, VesperRetryBackoff.Exponential, 64))
    }

    @Test
    fun retryDelayKeepsEachBackoffShapeAndDefaults() {
        assertEquals(1_000L, retryDelay(1_000, 8_000, VesperRetryBackoff.Fixed, 3))
        assertEquals(3_000L, retryDelay(1_000, 8_000, VesperRetryBackoff.Linear, 3))
        assertEquals(4_000L, retryDelay(1_000, 8_000, VesperRetryBackoff.Exponential, 3))
        assertEquals(3_000L, resolveRetryDelayMs(VesperRetryPolicy().toNativePayload(), 3))
    }

    private fun retryDelay(base: Long, maximum: Long, backoff: VesperRetryBackoff, attempt: Int): Long =
        resolveRetryDelayMs(
            VesperRetryPolicy(
                maxAttempts = null,
                baseDelayMs = base,
                maxDelayMs = maximum,
                backoff = backoff,
            ).toNativePayload(),
            attempt,
        )
}
