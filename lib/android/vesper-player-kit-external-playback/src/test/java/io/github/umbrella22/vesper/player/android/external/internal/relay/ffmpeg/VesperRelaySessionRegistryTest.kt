package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayLifetime
import java.io.Closeable
import java.io.InterruptedIOException
import java.util.concurrent.Callable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.*
import org.junit.Test

class VesperRelaySessionRegistryTest {
    private class Session : Closeable {
        val closes = AtomicInteger()
        override fun close() { closes.incrementAndGet() }
    }

    @Test
    fun concurrentFirstAcquirePreparesOneSharedSessionUntilInvalidation() {
        val registry = VesperRelaySessionRegistry<Session>()
        val owner = VesperRelayLifetime()
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val prepares = AtomicInteger()
        val executor = Executors.newFixedThreadPool(4)
        val session = Session()
        try {
            val results = (0 until 4).map {
                executor.submit(Callable {
                    registry.acquire("token", owner) {
                        prepares.incrementAndGet()
                        entered.countDown()
                        check(release.await(5, TimeUnit.SECONDS))
                        session
                    }
                })
            }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            release.countDown()
            results.forEach { assertSame(session, it.get(5, TimeUnit.SECONDS)) }
            assertEquals(1, prepares.get())
            assertEquals(0, session.closes.get())
            registry.invalidate("token")
            assertEquals(1, session.closes.get())
            owner.close()
            assertEquals("retired slot must be detached from its owner", 1, session.closes.get())
        } finally {
            release.countDown()
            owner.close()
            executor.shutdownNow()
        }
    }

    @Test
    fun cancellationDuringPreparationClosesLateResultAndRejectsFreshAcquire() {
        val registry = VesperRelaySessionRegistry<Session>()
        val owner = VesperRelayLifetime()
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val late = Session()
        val executor = Executors.newFixedThreadPool(2)
        try {
            val winner = executor.submit(Callable<Throwable?> {
                try {
                    registry.acquire("token", owner) {
                        entered.countDown()
                        // Model an operation whose underlying provider completes
                        // even when its caller has been interrupted.
                        while (release.count != 0L) {
                            try { release.await(5, TimeUnit.SECONDS) } catch (_: InterruptedException) { }
                        }
                        late
                    }
                    null
                } catch (error: Throwable) { error }
            })
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            owner.close()
            release.countDown()
            assertTrue(winner.get(5, TimeUnit.SECONDS) is InterruptedIOException)
            assertEquals(1, late.closes.get())
            assertThrows(InterruptedIOException::class.java) {
                registry.acquire("token", owner) { fail("retired owner cannot recreate preparation"); Session() }
            }
        } finally {
            release.countDown()
            owner.close()
            executor.shutdownNow()
        }
    }

    @Test
    fun failedPreparationClosesRegisteredResourcesAndAllowsOneFreshRetry() {
        val registry = VesperRelaySessionRegistry<Session>()
        val owner = VesperRelayLifetime()
        val partial = Session()
        assertThrows(IllegalStateException::class.java) {
            registry.acquire("token", owner) { lifetime ->
                lifetime.attach(partial)
                throw IllegalStateException("fixture failure")
            }
        }
        assertEquals(1, partial.closes.get())
        val replacement = registry.acquire("token", owner) { Session() }
        registry.invalidate("token")
        owner.close()
        assertEquals(1, partial.closes.get())
        assertEquals(1, replacement.closes.get())
    }
}
