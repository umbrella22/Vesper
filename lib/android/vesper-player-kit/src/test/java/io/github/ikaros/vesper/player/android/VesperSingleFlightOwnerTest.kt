package io.github.ikaros.vesper.player.android

import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperSingleFlightOwnerTest {
    @Test
    fun concurrentCallersShareOneConstruction() {
        val constructions = AtomicInteger(0)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val owner = VesperSingleFlightOwner<String, Any>(waitTimeoutMillis = 1_000L) { }
        val executor = Executors.newFixedThreadPool(2)
        try {
            val first = executor.submit<Any> {
                owner.get("cache") {
                    constructions.incrementAndGet()
                    entered.countDown()
                    release.await()
                    Any()
                }
            }
            entered.await(1, TimeUnit.SECONDS)
            val second = executor.submit<Any> {
                owner.get("cache") {
                    constructions.incrementAndGet()
                    Any()
                }
            }
            release.countDown()

            assertSame(first.get(1, TimeUnit.SECONDS), second.get(1, TimeUnit.SECONDS))
            assertEquals(1, constructions.get())
        } finally {
            release.countDown()
            executor.shutdownNow()
            owner.close()
        }
    }

    @Test
    fun waiterTimesOutWithoutStartingDuplicateConstruction() {
        val constructions = AtomicInteger(0)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val owner = VesperSingleFlightOwner<String, Any>(waitTimeoutMillis = 25L) { }
        val executor = Executors.newSingleThreadExecutor()
        try {
            val first = executor.submit<Any> {
                owner.get("cache") {
                    constructions.incrementAndGet()
                    entered.countDown()
                    release.await()
                    Any()
                }
            }
            entered.await(1, TimeUnit.SECONDS)

            assertThrows(TimeoutException::class.java) {
                owner.get("cache") {
                    constructions.incrementAndGet()
                    Any()
                }
            }
            assertEquals(1, constructions.get())
            val firstError = assertThrows(ExecutionException::class.java) {
                first.get(1, TimeUnit.SECONDS)
            }
            assertTrue(firstError.cause is TimeoutException)
            release.countDown()
        } finally {
            release.countDown()
            executor.shutdownNow()
            owner.close()
        }
    }

    @Test
    fun firstCallerTimesOutWithoutStartingDuplicateConstruction() {
        val constructions = AtomicInteger(0)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val resource = Any()
        val owner = VesperSingleFlightOwner<String, Any>(waitTimeoutMillis = 25L) { }
        val executor = Executors.newSingleThreadExecutor()
        try {
            val first = executor.submit<Any> {
                owner.get("cache") {
                    constructions.incrementAndGet()
                    entered.countDown()
                    release.await()
                    resource
                }
            }
            assertTrue(entered.await(1, TimeUnit.SECONDS))

            val error = assertThrows(ExecutionException::class.java) {
                first.get(1, TimeUnit.SECONDS)
            }
            assertTrue(error.cause is TimeoutException)
            assertEquals(1, constructions.get())

            release.countDown()
            assertSame(
                resource,
                owner.get("cache") {
                    constructions.incrementAndGet()
                    Any()
                },
            )
            assertEquals(1, constructions.get())
        } finally {
            release.countDown()
            executor.shutdownNow()
            owner.close()
        }
    }

    @Test
    fun distinctKeysAreRejectedAtTheConfiguredEntryLimit() {
        val constructions = AtomicInteger(0)
        val closes = AtomicInteger(0)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val resource = Any()
        val owner =
            VesperSingleFlightOwner<String, Any>(
                waitTimeoutMillis = 1_000L,
                maxEntries = 1,
            ) {
                closes.incrementAndGet()
            }
        val executor = Executors.newSingleThreadExecutor()
        try {
            val first = executor.submit<Any> {
                owner.get("blocked") {
                    constructions.incrementAndGet()
                    entered.countDown()
                    release.await()
                    resource
                }
            }
            assertTrue(entered.await(1, TimeUnit.SECONDS))

            val error = assertThrows(RejectedExecutionException::class.java) {
                owner.get("overflow") {
                    constructions.incrementAndGet()
                    Any()
                }
            }
            assertTrue(error.message.orEmpty().contains("entry limit of 1"))
            assertEquals(1, constructions.get())

            release.countDown()
            assertSame(resource, first.get(1, TimeUnit.SECONDS))
            owner.close()
            assertEquals(1, closes.get())
        } finally {
            release.countDown()
            executor.shutdownNow()
            owner.close()
        }
    }

    @Test
    fun timedOutQueuedConstructionIsDiscardedBeforeItCanPublish() {
        val queuedConstructions = AtomicInteger(0)
        val closes = AtomicInteger(0)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val blockedFinished = CountDownLatch(1)
        val firstFinished = CountDownLatch(1)
        val blockedResource = Any()
        val barrierResource = Any()
        val owner =
            VesperSingleFlightOwner<String, Any>(
                waitTimeoutMillis = 100L,
                maxEntries = 2,
            ) {
                closes.incrementAndGet()
            }
        val executor = Executors.newSingleThreadExecutor()
        try {
            val first = executor.submit<Any> {
                try {
                    owner.get("blocked") {
                        entered.countDown()
                        release.await()
                        blockedFinished.countDown()
                        blockedResource
                    }
                } finally {
                    firstFinished.countDown()
                }
            }
            assertTrue(entered.await(1, TimeUnit.SECONDS))
            assertTrue(firstFinished.await(1, TimeUnit.SECONDS))
            val firstError = assertThrows(ExecutionException::class.java) {
                first.get(1, TimeUnit.SECONDS)
            }
            assertTrue(firstError.cause is TimeoutException)

            assertThrows(TimeoutException::class.java) {
                owner.get("queued") {
                    queuedConstructions.incrementAndGet()
                    Any()
                }
            }

            release.countDown()
            assertTrue(blockedFinished.await(1, TimeUnit.SECONDS))
            assertSame(
                blockedResource,
                owner.get("blocked") { Any() },
            )
            assertSame(
                barrierResource,
                owner.get("barrier") { barrierResource },
            )
            assertEquals(0, queuedConstructions.get())
            owner.close()
            assertEquals(2, closes.get())
        } finally {
            release.countDown()
            executor.shutdownNow()
            owner.close()
        }
    }

    @Test
    fun closeReleasesPublishedResourceExactlyOnce() {
        val closes = AtomicInteger(0)
        val owner = VesperSingleFlightOwner<String, Any>(waitTimeoutMillis = 1_000L) {
            closes.incrementAndGet()
        }
        owner.get("cache") { Any() }

        owner.close()
        owner.close()

        assertEquals(1, closes.get())
    }
}
