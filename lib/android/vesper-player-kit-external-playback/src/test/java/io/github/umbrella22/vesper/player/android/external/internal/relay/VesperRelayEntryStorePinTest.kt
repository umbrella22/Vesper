package io.github.umbrella22.vesper.player.android.external.internal.relay

import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import java.io.Closeable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Test

class VesperRelayEntryStorePinTest {
    @Test
    fun activeTokenSurvivesPausePastTtlAndPruneButUnclaimedTokenExpires() {
        var now = 0L
        val invalidated = mutableListOf<String>()
        val store = VesperRelayEntryStore(30 * 60 * 1000L, { now }, invalidated::add)
        val source = VesperPlayerSource.remote("https://fixture.test/movie.mp4", "Movie")
        store.put("active", source, null)
        store.put("unclaimed", source, null)
        assertTrue(store.pin("active"))
        now = 31 * 60 * 1000L
        store.pruneExpiredEntries()
        assertNotNull(store.entryForToken("active"))
        assertNull(store.entryForToken("unclaimed"))
        assertEquals(listOf("unclaimed"), invalidated)
        store.invalidate("active")
        assertFalse(store.pin("active"))
        assertNull(store.entryForToken("active"))
        assertEquals(listOf("unclaimed", "active"), invalidated)
    }

    @Test
    fun pinDoesNotReviveExpiredOrRemovedRegistration() {
        var now = 0L
        val invalidated = mutableListOf<String>()
        val store = VesperRelayEntryStore(100L, { now }, invalidated::add)
        val source = VesperPlayerSource.remote("https://fixture.test/movie.mp4", "Movie")
        store.put("expired", source, null)
        now = 101L
        assertFalse(store.pin("expired"))
        assertNull(store.entryForToken("expired"))
        assertFalse(store.pin("missing"))
        assertEquals(listOf("expired"), invalidated)
    }

    @Test
    fun invalidateAllRetiresEveryLifetimeBeforeTheFirstCloseCanBlock() {
        val invalidated = mutableListOf<String>()
        val store = VesperRelayEntryStore(null, { 0L }, invalidated::add)
        val source = VesperPlayerSource.remote("https://fixture.test/movie.mp4", "Movie")
        store.put("first", source, null)
        store.put("second", source, null)
        val first = store.entryForToken("first")!!
        val second = store.entryForToken("second")!!
        val firstCloseEntered = CountDownLatch(1)
        val releaseFirstClose = CountDownLatch(1)
        first.lifetime.attach(object : Closeable {
            override fun close() {
                firstCloseEntered.countDown()
                check(releaseFirstClose.await(5, TimeUnit.SECONDS))
            }
        })
        second.lifetime.attach(object : Closeable { override fun close() = Unit })

        val executor = Executors.newSingleThreadExecutor()
        try {
            val future = executor.submit { store.invalidateAll() }
            assertTrue(firstCloseEntered.await(5, TimeUnit.SECONDS))

            assertFalse("every detached lifetime must be terminal before cleanup blocks", second.lifetime.isActive)
            val late = object : Closeable {
                var closed = false
                override fun close() { closed = true }
            }
            assertFalse(second.lifetime.attach(late))
            assertTrue("late resources must be closed immediately", late.closed)

            releaseFirstClose.countDown()
            future.get(5, TimeUnit.SECONDS)
            assertEquals(listOf("first", "second"), invalidated)
        } finally {
            releaseFirstClose.countDown()
            executor.shutdownNow()
        }
    }
}
