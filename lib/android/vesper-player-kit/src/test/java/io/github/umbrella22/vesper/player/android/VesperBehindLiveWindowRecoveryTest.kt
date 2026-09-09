package io.github.umbrella22.vesper.player.android

import androidx.media3.common.PlaybackException
import androidx.media3.exoplayer.source.BehindLiveWindowException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperBehindLiveWindowRecoveryTest {
    @Test
    fun detectsErrorCodeAndNestedBehindLiveWindowCause() {
        val direct =
            playbackException(
                errorCode = PlaybackException.ERROR_CODE_BEHIND_LIVE_WINDOW,
            )
        val nested =
            playbackException(
                errorCode = PlaybackException.ERROR_CODE_UNSPECIFIED,
                cause = IllegalStateException("wrapper", BehindLiveWindowException()),
            )

        assertTrue(direct.isBehindLiveWindowError())
        assertTrue(nested.isBehindLiveWindowError())
        val classified = classifyPlaybackException(nested)
        assertEquals(BACKEND_FAILURE_ORDINAL, classified.codeOrdinal)
        assertEquals(PLAYBACK_CATEGORY_ORDINAL, classified.categoryOrdinal)
        assertTrue(classified.retriable)
    }

    @Test
    fun boundsConsecutiveRecoveryAttempts() {
        val state = BehindLiveWindowRecoveryState(maxAttempts = 2)

        val first = state.onBehindLiveWindow(sourceEpoch = 7L)
        val second = state.onBehindLiveWindow(sourceEpoch = 7L)
        val exhausted = state.onBehindLiveWindow(sourceEpoch = 7L)

        assertTrue(first.shouldRecover)
        assertEquals(1, first.observedFailure)
        assertTrue(second.shouldRecover)
        assertEquals(2, second.observedFailure)
        assertFalse(exhausted.shouldRecover)
        assertEquals(3, exhausted.observedFailure)
        assertEquals(2, exhausted.maxAttempts)
    }

    @Test
    fun stableRecoveryAndSourceEpochResetAllowFutureAttemptOne() {
        val state = BehindLiveWindowRecoveryState(maxAttempts = 2)
        state.onBehindLiveWindow(sourceEpoch = 7L)
        state.onBehindLiveWindow(sourceEpoch = 7L)

        assertEquals(2, state.markStable(sourceEpoch = 7L))
        assertEquals(1, state.onBehindLiveWindow(sourceEpoch = 7L).observedFailure)
        state.onBehindLiveWindow(sourceEpoch = 7L)
        assertFalse(state.onBehindLiveWindow(sourceEpoch = 7L).shouldRecover)

        val nextSource = state.onBehindLiveWindow(sourceEpoch = 8L)
        assertTrue(nextSource.shouldRecover)
        assertEquals(1, nextSource.observedFailure)
        assertNull(state.markStable(sourceEpoch = 7L))
    }

    @Test
    fun nonBehindLiveWindowErrorDoesNotConsumeRecoveryAttempt() {
        val state = BehindLiveWindowRecoveryState(maxAttempts = 2)
        val networkError =
            playbackException(
                errorCode = PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
            )

        assertNull(behindLiveWindowRecoveryDecision(networkError, sourceEpoch = 11L, state = state))
        assertEquals(
            1,
            state.onBehindLiveWindow(sourceEpoch = 11L).observedFailure,
        )
    }

    @Test
    fun internalSeekSuppressionIsConsumedOnceAndExpires() {
        val suppression = BehindLiveWindowInternalSeekSuppression(timeoutMs = 2_000L)
        suppression.arm(sourceEpoch = 7L, nowElapsedRealtimeMs = 10_000L)

        assertTrue(suppression.consumeIfPending(sourceEpoch = 7L, nowElapsedRealtimeMs = 10_100L))
        assertFalse(suppression.consumeIfPending(sourceEpoch = 7L, nowElapsedRealtimeMs = 10_200L))

        suppression.arm(sourceEpoch = 7L, nowElapsedRealtimeMs = 20_000L)
        assertFalse(suppression.consumeIfPending(sourceEpoch = 7L, nowElapsedRealtimeMs = 22_001L))
    }

    @Test
    fun sourceResetDoesNotSuppressNextUserSeek() {
        val suppression = BehindLiveWindowInternalSeekSuppression(timeoutMs = 2_000L)
        suppression.arm(sourceEpoch = 7L, nowElapsedRealtimeMs = 10_000L)

        suppression.resetForSource(sourceEpoch = 8L)

        assertFalse(suppression.consumeIfPending(sourceEpoch = 8L, nowElapsedRealtimeMs = 10_100L))
        assertFalse(suppression.consumeIfPending(sourceEpoch = 7L, nowElapsedRealtimeMs = 10_100L))
    }

    private fun playbackException(
        errorCode: Int,
        cause: Throwable? = null,
    ): PlaybackException = PlaybackException("playback failed", cause, errorCode)
}
