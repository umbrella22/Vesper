package io.github.umbrella22.vesper.example.androidcomposehost

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ExamplePictureInPicturePresentationStateTest {
    @Test
    fun `manual request enters presentation until mode callback exits`() {
        val requested =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureRequestStarted()

        assertTrue(requested.presentation)
        assertFalse(requested.active)

        val active = requested.onPictureInPictureModeChanged(true)
        assertTrue(active.presentation)
        assertTrue(active.active)

        val inactive = active.onPictureInPictureModeChanged(false)
        assertFalse(inactive.presentation)
        assertFalse(inactive.active)
    }

    @Test
    fun `user leave hint only enters presentation when pip is enabled`() {
        val disabled =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureUserLeaveHint(enabled = false)

        assertFalse(disabled.presentation)

        val enabled =
            disabled.onPictureInPictureUserLeaveHint(enabled = true)

        assertTrue(enabled.presentation)
        assertTrue(enabled.pendingAutoEnter)
    }

    @Test
    fun `auto enter timeout restores presentation when pip never starts`() {
        val pending =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureUserLeaveHint(enabled = true)

        val timedOut = pending.onPictureInPictureAutoEnterTimeout()

        assertFalse(timedOut.presentation)
        assertFalse(timedOut.pendingAutoEnter)
    }

    @Test
    fun `auto enter timeout keeps active pip presentation`() {
        val active =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureUserLeaveHint(enabled = true)
                .onPictureInPictureModeChanged(true)

        val timedOut = active.onPictureInPictureAutoEnterTimeout()

        assertTrue(timedOut.presentation)
        assertTrue(timedOut.active)
    }
}
