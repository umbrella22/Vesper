package io.github.umbrella22.vesper.player.android

import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlayerLooperDispatcherTest {
    @Test
    fun ownerThreadRunsInlineWithoutPosting() {
        var posts = 0
        var mutations = 0
        val dispatcher =
            VesperPlayerLooperDispatcher(
                isOwnerThread = { true },
                post = {
                    posts += 1
                    true
                },
                remove = {},
                timeoutMs = 10,
            )

        assertTrue(dispatcher.dispatch { mutations += 1 })
        assertEquals(1, mutations)
        assertEquals(0, posts)
    }

    @Test
    fun workerDispatchCompletesOnOwnerExecutor() {
        val executor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "fake-player-application-looper")
        }
        val ownerMutations = AtomicInteger(0)
        val dispatcher =
            VesperPlayerLooperDispatcher(
                isOwnerThread = { false },
                post = {
                    executor.execute(it)
                    true
                },
                remove = {},
                timeoutMs = 1_000,
            )
        try {
            assertTrue(
                dispatcher.dispatch {
                    if (Thread.currentThread().name == "fake-player-application-looper") {
                        ownerMutations.incrementAndGet()
                    }
                }
            )
            assertEquals(1, ownerMutations.get())
        } finally {
            executor.shutdownNow()
            assertTrue(executor.awaitTermination(1, TimeUnit.SECONDS))
        }
    }

    @Test
    fun queuedTimeoutCancelsLateMutation() {
        var queued: Runnable? = null
        var removed = false
        var mutations = 0
        val dispatcher =
            VesperPlayerLooperDispatcher(
                isOwnerThread = { false },
                post = {
                    queued = it
                    true
                },
                remove = {
                    if (queued === it) {
                        removed = true
                    }
                },
                timeoutMs = 5,
            )

        val startedAt = System.nanoTime()
        assertFalse(dispatcher.dispatch { mutations += 1 })
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAt)
        assertTrue(elapsedMs < 200)
        assertTrue(removed)

        queued?.run()
        assertEquals(0, mutations)
    }
}
