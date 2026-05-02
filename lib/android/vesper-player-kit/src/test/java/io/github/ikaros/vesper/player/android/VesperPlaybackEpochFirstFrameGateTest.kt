package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlaybackEpochFirstFrameGateTest {
    @Test
    fun marksFirstFrameOnlyOncePerPlaybackEpoch() {
        val gate = VesperPlaybackEpochFirstFrameGate()

        gate.advanceEpoch()
        assertTrue(gate.markFirstFrameRendered().isFirstForEpoch)
        assertFalse(gate.markFirstFrameRendered().isFirstForEpoch)

        gate.advanceEpoch()
        assertTrue(gate.markFirstFrameRendered().isFirstForEpoch)
        assertFalse(gate.markFirstFrameRendered().isFirstForEpoch)
    }
}
