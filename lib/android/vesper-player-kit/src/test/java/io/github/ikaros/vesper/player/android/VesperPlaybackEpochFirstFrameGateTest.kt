package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

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

    @Test
    fun concurrentFirstFrameMarksOnlyWinOnce() {
        val gate = VesperPlaybackEpochFirstFrameGate()
        gate.advanceEpoch()
        val workers = 16
        val executor = Executors.newFixedThreadPool(workers)
        val ready = CountDownLatch(workers)
        val start = CountDownLatch(1)
        val done = CountDownLatch(workers)
        val winners = java.util.concurrent.atomic.AtomicInteger(0)

        repeat(workers) {
            executor.execute {
                ready.countDown()
                start.await()
                if (gate.markFirstFrameRendered().isFirstForEpoch) {
                    winners.incrementAndGet()
                }
                done.countDown()
            }
        }

        assertTrue(ready.await(1, TimeUnit.SECONDS))
        start.countDown()
        assertTrue(done.await(1, TimeUnit.SECONDS))
        executor.shutdownNow()
        assertEquals(1, winners.get())
    }
}
