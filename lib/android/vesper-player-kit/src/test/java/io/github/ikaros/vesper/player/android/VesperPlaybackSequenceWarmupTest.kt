package io.github.ikaros.vesper.player.android

import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class VesperPlaybackSequenceWarmupTest {
    private fun intentJson(revision: Long = 1L): JSONObject =
        JSONObject(
            """
            {
              "sessionGeneration": 7,
              "itemId": "item-a",
              "sourceReference": "sequence-source-1",
              "sourceRevision": $revision,
              "warmupTaskId": ${100 + revision},
              "warmupGoal": "progressiveRange",
              "priority": "next",
              "cacheIdentity": {
                "canonicalKey": "vesper-sequence-cache:v1:17:example.provider:9:content-a:4:1080:5:media:6:public:$revision"
              },
              "profile": {
                "expectedMemoryBytes": 99999999,
                "warmupWindowMs": 1000
              }
            }
            """.trimIndent(),
        )

    @Test
    fun parserBoundsProgressiveWarmupAndKeepsRevisionInKey() {
        val first = VesperSequenceWarmupIntent.fromJson(intentJson())
        val second = VesperSequenceWarmupIntent.fromJson(intentJson(revision = 2L))

        assertNotNull(first)
        assertNotNull(second)
        assertEquals(64L * 1024L, first!!.targetBytes)
        assertNotEquals(first.key, second!!.key)
        assertEquals("progressiveRange", first.goal)
    }

    @Test
    fun parserRejectsUrlLikeOrUnknownIdentity() {
        val urlLike = intentJson().put("cacheIdentity", JSONObject().put("canonicalKey", "https://token"))
        val unknownPriority = intentJson().put("priority", "background")
        val unknownGoal = intentJson().put("warmupGoal", "decoderReady")

        assertNull(VesperSequenceWarmupIntent.fromJson(urlLike))
        assertNull(VesperSequenceWarmupIntent.fromJson(unknownPriority))
        assertNull(VesperSequenceWarmupIntent.fromJson(unknownGoal))
    }

    @Test
    fun warmupSnapshotStartsBoundedAndEmpty() {
        val snapshot = VesperPlaybackSequenceWarmupSnapshot()
        assertEquals(0, snapshot.activeJobs)
        assertEquals(0L, snapshot.actualBytes)
        assertTrue(snapshot.cacheEntries <= 4_096)
    }

    @Test
    fun saturatingCounterDoesNotWrap() {
        assertEquals(Long.MAX_VALUE, vesperSequenceSaturatingAdd(Long.MAX_VALUE, 1L))
        assertEquals(Long.MAX_VALUE, vesperSequenceSaturatingAdd(Long.MAX_VALUE - 1L, 1L))
        assertEquals(3L, vesperSequenceSaturatingAdd(1L, 2L))
    }

    @Test
    fun progressiveTransportStripsCallerRangeAndReadsExactly64KiB() = runTest {
        val requests = mutableListOf<VesperSequenceWarmupReadRequest>()
        val stream = ByteArrayWarmupStream(totalBytes = 128 * 1024, cacheHit = false)
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport { request ->
                    requests += request
                    stream
                },
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson()))
        val source =
            progressiveSource(
                revision = 1L,
                headers =
                    mapOf(
                        "Authorization" to "Bearer private-token",
                        "Referer" to "https://example.invalid/watch",
                        "rAnGe" to "bytes=99-100",
                    ),
            )

        executor.reconcile(listOf(intent)) { _, _, _ -> source }
        advanceUntilIdle()

        assertEquals(1, requests.size)
        assertEquals(0L, requests.single().position)
        assertEquals(64L * 1024L, requests.single().length)
        assertEquals(1_000L, requests.single().timeoutMillis)
        assertEquals("Bearer private-token", requests.single().headers["Authorization"])
        assertEquals("https://example.invalid/watch", requests.single().headers["Referer"])
        assertFalse(requests.single().headers.keys.any { it.equals("Range", ignoreCase = true) })
        assertEquals(64L * 1024L, stream.bytesRead)
        assertTrue(stream.closed.get())
        assertEquals("completed", reports.last().status)
        assertEquals(64L * 1024L, reports.last().actualBytes)
        assertEquals(false, reports.last().cacheHit)
        assertEquals(0, reports.last().cacheEntries)
        assertEquals(0L, reports.last().cacheBytes)
        assertEquals(0L, reports.last().evictedEntries)
        assertEquals(1L, executor.snapshot.value.completedJobs)
        executor.close()
    }

    @Test
    fun completedReportUsesSameNonZeroInventoryAsSnapshot() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val order = mutableListOf<String>()
        var inventoryCall = 0
        val inventoryObserver =
            VesperSequenceCacheInventoryObserver {
                inventoryCall += 1
                order += "inventory-$inventoryCall"
                if (inventoryCall == 1) {
                    VesperSequenceCacheInventoryObservation(
                        keys = setOf("evicted-key", "retained-key"),
                        bytes = 128L * 1024L,
                    )
                } else {
                    VesperSequenceCacheInventoryObservation(
                        keys = setOf("retained-key"),
                        bytes = 64L * 1024L,
                    )
                }
            }
        val executor =
            testExecutor(
                onReport = {
                    reports += it
                    if (it.status == "completed") order += "completed-report"
                },
                transport = VesperSequenceWarmupTransport {
                    ByteArrayWarmupStream(totalBytes = 64 * 1024, cacheHit = true)
                },
                inventoryObserver = inventoryObserver,
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson()))

        executor.reconcile(listOf(intent)) { _, _, revision -> progressiveSource(revision) }
        advanceUntilIdle()

        val completed = reports.single { it.status == "completed" }
        assertEquals(1, completed.cacheEntries)
        assertEquals(64L * 1024L, completed.cacheBytes)
        assertEquals(1L, completed.evictedEntries)
        assertEquals(1, executor.snapshot.value.cacheEntries)
        assertEquals(64L * 1024L, executor.snapshot.value.cacheBytes)
        assertEquals(1L, executor.snapshot.value.evictedEntries)
        assertEquals(listOf("inventory-2", "completed-report"), order.takeLast(2))
        executor.close()
    }

    @Test
    fun inventoryFailureReportsTypedFailureWithoutInventingEviction() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        var inventoryCall = 0
        val inventoryObserver =
            VesperSequenceCacheInventoryObserver {
                inventoryCall += 1
                if (inventoryCall == 1) {
                    VesperSequenceCacheInventoryObservation(
                        keys = setOf("first-key", "second-key"),
                        bytes = 128L * 1024L,
                    )
                } else {
                    error("inventory unavailable")
                }
            }
        val executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport {
                    ByteArrayWarmupStream(totalBytes = 64 * 1024, cacheHit = false)
                },
                inventoryObserver = inventoryObserver,
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson()))

        executor.reconcile(listOf(intent)) { _, _, revision -> progressiveSource(revision) }
        advanceUntilIdle()

        assertTrue(reports.none { it.status == "completed" })
        assertEquals("cache_inventory_failed", reports.last().reasonCode)
        assertEquals(1L, executor.snapshot.value.failedJobs)
        assertEquals(0L, executor.snapshot.value.evictedEntries)
        assertEquals(2, executor.snapshot.value.cacheEntries)
        assertEquals(128L * 1024L, executor.snapshot.value.cacheBytes)
        executor.close()
    }

    @Test
    fun revisionReplacementCancelsOldReadExactlyOnce() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val staleStream = SuspendedWarmupStream()
        val currentStream = ByteArrayWarmupStream(totalBytes = 64 * 1024, cacheHit = false)
        val executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport { request ->
                    if (request.uri.contains("revision=1")) staleStream else currentStream
                },
            )
        val first = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))
        val second = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 2L)))

        executor.reconcile(listOf(first)) { _, _, revision -> progressiveSource(revision) }
        runCurrent()
        executor.reconcile(listOf(second)) { _, _, revision -> progressiveSource(revision) }
        advanceUntilIdle()

        assertEquals(listOf(2L), reports.filter { it.status == "completed" }.map { it.sourceRevision })
        assertEquals(listOf(1L), reports.filter { it.status == "cancelled" }.map { it.sourceRevision })
        assertTrue(staleStream.closed.get())
        assertTrue(currentStream.closed.get())
        assertEquals(1L, executor.snapshot.value.completedJobs)
        assertEquals(1L, executor.snapshot.value.cancelledJobs)
        executor.close()
    }

    @Test
    fun revisionReplacementDuringInventoryCannotCompleteOldRevision() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val first = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))
        val second = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 2L)))
        var inventoryCall = 0
        lateinit var executor: VesperPlaybackSequenceWarmupExecutor
        val inventoryObserver =
            VesperSequenceCacheInventoryObserver {
                inventoryCall += 1
                if (inventoryCall == 2) {
                    executor.reconcile(listOf(second)) { _, _, revision -> progressiveSource(revision) }
                }
                VesperSequenceCacheInventoryObservation(
                    keys = setOf("retained-key"),
                    bytes = 64L * 1024L,
                )
            }
        executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport {
                    ByteArrayWarmupStream(totalBytes = 64 * 1024, cacheHit = false)
                },
                inventoryObserver = inventoryObserver,
            )

        executor.reconcile(listOf(first)) { _, _, revision -> progressiveSource(revision) }
        advanceUntilIdle()

        assertEquals(listOf(1L), reports.filter { it.status == "cancelled" }.map { it.sourceRevision })
        assertEquals(listOf(2L), reports.filter { it.status == "completed" }.map { it.sourceRevision })
        assertTrue(reports.none { it.status == "completed" && it.sourceRevision == 1L })
        assertEquals(1L, executor.snapshot.value.cancelledJobs)
        assertEquals(1L, executor.snapshot.value.completedJobs)
        executor.close()
    }

    @Test
    fun cancellationMarkedBeforeJobCancelStillReportsCancelled() {
        val readEntered = CountDownLatch(1)
        val releaseRead = CountDownLatch(1)
        val worker = Executors.newSingleThreadExecutor()
        val dispatcher = worker.asCoroutineDispatcher()
        val reports = CopyOnWriteArrayList<VesperSequenceWarmupReport>()
        val oldCancelled = CountDownLatch(1)
        val currentCompleted = CountDownLatch(1)
        var executor: VesperPlaybackSequenceWarmupExecutor? = null
        try {
            executor =
                VesperPlaybackSequenceWarmupExecutor(
                    context = null,
                    maxDiskBytes = 0L,
                    onSourceExpired = { _, _ -> },
                    onReport = {
                        reports += it
                        if (it.status == "cancelled" && it.sourceRevision == 1L) oldCancelled.countDown()
                        if (it.status == "completed" && it.sourceRevision == 2L) currentCompleted.countDown()
                    },
                    dispatcher = dispatcher,
                    transport = VesperSequenceWarmupTransport {
                        BlockingCompletionWarmupStream(readEntered, releaseRead)
                    },
                    cancellationMarkedCallback = { jobs ->
                        releaseRead.countDown()
                        runBlocking { jobs.forEach { it.join() } }
                    },
                )
            val first = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))
            val second = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 2L)))

            val activeExecutor = requireNotNull(executor)
            activeExecutor.reconcile(listOf(first)) { _, _, revision -> progressiveSource(revision) }
            assertTrue(readEntered.await(5, TimeUnit.SECONDS))
            activeExecutor.reconcile(listOf(second)) { _, _, revision -> progressiveSource(revision) }
            assertTrue(oldCancelled.await(5, TimeUnit.SECONDS))
            assertTrue(currentCompleted.await(5, TimeUnit.SECONDS))

            assertEquals(listOf(1L), reports.filter { it.status == "cancelled" }.map { it.sourceRevision })
            assertEquals(listOf(2L), reports.filter { it.status == "completed" }.map { it.sourceRevision })
            assertTrue(reports.none { it.status == "completed" && it.sourceRevision == 1L })
            assertEquals(1L, activeExecutor.snapshot.value.cancelledJobs)
            assertEquals(1L, activeExecutor.snapshot.value.completedJobs)
        } finally {
            executor?.close()
            dispatcher.close()
        }
    }

    @Test
    fun sameKeyCannotReenterBetweenJobRemovalAndTerminalCommit() {
        val readEntered = CountDownLatch(1)
        val releaseRead = CountDownLatch(1)
        val worker = Executors.newSingleThreadExecutor()
        val dispatcher = worker.asCoroutineDispatcher()
        val reports = CopyOnWriteArrayList<VesperSequenceWarmupReport>()
        val reentrySourceLookups = AtomicInteger(0)
        val reentryAttempted = AtomicBoolean(false)
        val oldCancelled = CountDownLatch(1)
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))
        var executor: VesperPlaybackSequenceWarmupExecutor? = null
        try {
            executor =
                VesperPlaybackSequenceWarmupExecutor(
                    context = null,
                    maxDiskBytes = 0L,
                    onSourceExpired = { _, _ -> },
                    onReport = {
                        reports += it
                        if (it.status == "cancelled" && it.sourceRevision == 1L) oldCancelled.countDown()
                    },
                    dispatcher = dispatcher,
                    transport = VesperSequenceWarmupTransport {
                        BlockingCompletionWarmupStream(readEntered, releaseRead)
                    },
                    cancellationMarkedCallback = { jobs ->
                        releaseRead.countDown()
                        runBlocking { jobs.forEach { it.join() } }
                    },
                    completionTransitionCallback = {
                        if (reentryAttempted.compareAndSet(false, true)) {
                            requireNotNull(executor).reconcile(listOf(intent)) { _, _, revision ->
                                reentrySourceLookups.incrementAndGet()
                                progressiveSource(revision)
                            }
                        }
                    },
                )
            val activeExecutor = requireNotNull(executor)

            activeExecutor.reconcile(listOf(intent)) { _, _, revision -> progressiveSource(revision) }
            assertTrue(readEntered.await(5, TimeUnit.SECONDS))
            activeExecutor.reconcile(emptyList()) { _, _, revision -> progressiveSource(revision) }

            assertTrue(reentryAttempted.get())
            assertEquals(0, reentrySourceLookups.get())
            assertTrue(oldCancelled.await(5, TimeUnit.SECONDS))
            assertEquals(listOf(1L), reports.filter { it.status == "cancelled" }.map { it.sourceRevision })
            assertTrue(reports.none { it.status == "completed" && it.sourceRevision == 1L })
        } finally {
            executor?.close()
            dispatcher.close()
        }
    }

    @Test
    fun terminalCommitDuringSourceLookupPreventsSameKeyInsertion() {
        val lookupEntered = CountDownLatch(1)
        val releaseLookup = CountDownLatch(1)
        val worker = Executors.newSingleThreadExecutor()
        val lookupCaller = Executors.newSingleThreadExecutor()
        val dispatcher = worker.asCoroutineDispatcher()
        val openCount = AtomicInteger(0)
        val sourceLookups = AtomicInteger(0)
        val reports = CopyOnWriteArrayList<VesperSequenceWarmupReport>()
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))
        val executor =
            VesperPlaybackSequenceWarmupExecutor(
                context = null,
                maxDiskBytes = 0L,
                onSourceExpired = { _, _ -> },
                onReport = reports::add,
                dispatcher = dispatcher,
                transport = VesperSequenceWarmupTransport {
                    openCount.incrementAndGet()
                    SuspendedWarmupStream()
                },
            )
        try {
            val firstLookup = lookupCaller.submit {
                executor.reconcile(listOf(intent)) { _, _, revision ->
                    sourceLookups.incrementAndGet()
                    lookupEntered.countDown()
                    check(releaseLookup.await(5, TimeUnit.SECONDS))
                    progressiveSource(revision)
                }
            }
            assertTrue(lookupEntered.await(5, TimeUnit.SECONDS))

            executor.reconcile(listOf(intent)) { _, _, _ ->
                sourceLookups.incrementAndGet()
                null
            }
            releaseLookup.countDown()
            firstLookup.get(5, TimeUnit.SECONDS)

            assertEquals(2, sourceLookups.get())
            assertEquals(0, openCount.get())
            assertEquals(0, executor.snapshot.value.activeJobs)
            assertEquals(1L, executor.snapshot.value.failedJobs)
            assertEquals(listOf("source_reference_missing"), reports.map { it.reasonCode })
        } finally {
            releaseLookup.countDown()
            executor.close()
            dispatcher.close()
            lookupCaller.shutdownNow()
        }
    }

    @Test
    fun activeJobDoesNotResolveOrFailAgain() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val sourceLookups = AtomicInteger(0)
        val stream = SuspendedWarmupStream()
        val executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport { stream },
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))

        executor.reconcile(listOf(intent)) { _, _, revision ->
            sourceLookups.incrementAndGet()
            progressiveSource(revision)
        }
        runCurrent()
        executor.reconcile(listOf(intent)) { _, _, _ ->
            sourceLookups.incrementAndGet()
            null
        }

        assertEquals(1, sourceLookups.get())
        assertTrue(reports.none { it.status == "failed" })
        assertEquals(0L, executor.snapshot.value.failedJobs)
        executor.close()
        advanceUntilIdle()
    }

    @Test
    fun closeDuringSourceLookupCannotRecordMissingSourceFailure() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport {
                    ByteArrayWarmupStream(totalBytes = 64 * 1024, cacheHit = false)
                },
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))

        executor.reconcile(listOf(intent)) { _, _, _ ->
            executor.close()
            null
        }

        assertTrue(reports.isEmpty())
        assertEquals(0L, executor.snapshot.value.failedJobs)
        assertEquals(0, executor.snapshot.value.activeJobs)
    }

    @Test
    fun terminalKeySurvivesWindowExitAndPriorityChange() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val sourceLookups = AtomicInteger(0)
        val executor =
            testExecutor(
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport {
                    ByteArrayWarmupStream(totalBytes = 64 * 1024, cacheHit = false)
                },
            )
        val nextIntent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = 1L)))
        val currentIntent = nextIntent.copy(priority = VesperSequenceWarmupPriority.Current)

        executor.reconcile(listOf(nextIntent)) { _, _, revision ->
            sourceLookups.incrementAndGet()
            progressiveSource(revision)
        }
        advanceUntilIdle()
        executor.reconcile(emptyList()) { _, _, revision -> progressiveSource(revision) }
        executor.reconcile(listOf(currentIntent)) { _, _, revision ->
            sourceLookups.incrementAndGet()
            progressiveSource(revision)
        }
        advanceUntilIdle()

        assertEquals(1, sourceLookups.get())
        assertEquals(1, reports.count { it.status == "completed" })
        assertEquals(1L, executor.snapshot.value.completedJobs)
        executor.close()
    }

    @Test
    fun closeFencesLateExpiryAndTerminalCallbacks() = runTest {
        val release = CompletableDeferred<Unit>()
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val expiries = mutableListOf<Pair<String, Long>>()
        val stream = LateExpiredWarmupStream(release)
        val executor =
            testExecutor(
                onSourceExpired = { itemId, revision -> expiries += itemId to revision },
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport { stream },
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson()))

        executor.reconcile(listOf(intent)) { _, _, revision -> progressiveSource(revision) }
        runCurrent()
        executor.close()
        release.complete(Unit)
        advanceUntilIdle()

        assertTrue(expiries.isEmpty())
        assertTrue(reports.none { it.status != "started" })
        assertTrue(stream.closed.get())
        assertEquals(0, executor.snapshot.value.activeJobs)
    }

    @Test
    fun closeInsideExpiryCallbackSuppressesTerminalReport() = runTest {
        val reports = mutableListOf<VesperSequenceWarmupReport>()
        val expiries = mutableListOf<Pair<String, Long>>()
        lateinit var executor: VesperPlaybackSequenceWarmupExecutor
        executor =
            testExecutor(
                onSourceExpired = { itemId, revision ->
                    expiries += itemId to revision
                    executor.close()
                },
                onReport = reports::add,
                transport = VesperSequenceWarmupTransport {
                    throw VesperSequenceWarmupHttpStatusException(403)
                },
            )
        val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson()))

        executor.reconcile(listOf(intent)) { _, _, revision -> progressiveSource(revision) }
        advanceUntilIdle()

        assertEquals(listOf("item-a" to 1L), expiries)
        assertTrue(reports.none { it.status == "failed" && it.reasonCode == "source_expired" })
        assertEquals(0, executor.snapshot.value.activeJobs)
    }

    @Test
    fun expiredHttpStatusesNotifyCurrentRevisionOnly() = runTest {
        for (status in listOf(401, 403, 410, 404)) {
            val reports = mutableListOf<VesperSequenceWarmupReport>()
            val expiries = mutableListOf<Pair<String, Long>>()
            val executor =
                testExecutor(
                    onSourceExpired = { itemId, revision -> expiries += itemId to revision },
                    onReport = reports::add,
                    transport = VesperSequenceWarmupTransport {
                        throw VesperSequenceWarmupHttpStatusException(status)
                    },
                )
            val intent = requireNotNull(VesperSequenceWarmupIntent.fromJson(intentJson(revision = status.toLong())))

            executor.reconcile(listOf(intent)) { _, _, revision -> progressiveSource(revision) }
            advanceUntilIdle()

            if (status == 404) {
                assertTrue(expiries.isEmpty())
                assertEquals("warmup_failed", reports.last().reasonCode)
            } else {
                assertEquals(listOf("item-a" to status.toLong()), expiries)
                assertEquals("source_expired", reports.last().reasonCode)
            }
            executor.close()
        }
    }

    private fun kotlinx.coroutines.test.TestScope.testExecutor(
        onSourceExpired: (String, Long) -> Unit = { _, _ -> },
        onReport: (VesperSequenceWarmupReport) -> Unit,
        transport: VesperSequenceWarmupTransport,
        inventoryObserver: VesperSequenceCacheInventoryObserver? = null,
    ): VesperPlaybackSequenceWarmupExecutor =
        VesperPlaybackSequenceWarmupExecutor(
            context = null,
            maxDiskBytes = 0L,
            onSourceExpired = onSourceExpired,
            onReport = onReport,
            dispatcher = StandardTestDispatcher(testScheduler),
            transport = transport,
            inventoryObserver = inventoryObserver,
        )

    private fun progressiveSource(
        revision: Long,
        headers: Map<String, String> = emptyMap(),
    ): VesperPlayerSource =
        VesperPlayerSource.remote(
            uri = "https://example.invalid/video.mp4?revision=$revision",
            label = "fixture",
            protocol = VesperPlayerSourceProtocol.Progressive,
            headers = headers,
        )
}

private class ByteArrayWarmupStream(
    private val totalBytes: Int,
    override val cacheHit: Boolean,
) : VesperSequenceWarmupReadStream {
    val closed = AtomicBoolean(false)
    var bytesRead: Long = 0L
        private set

    override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        val remaining = totalBytes - bytesRead.toInt()
        if (remaining <= 0) return -1
        val count = minOf(remaining, length)
        repeat(count) { buffer[offset + it] = 0x41 }
        bytesRead += count
        return count
    }

    override fun close() {
        closed.set(true)
    }
}

private class SuspendedWarmupStream : VesperSequenceWarmupReadStream {
    override val cacheHit: Boolean = false
    val closed = AtomicBoolean(false)

    override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int = awaitCancellation()

    override fun close() {
        closed.set(true)
    }
}

private class BlockingCompletionWarmupStream(
    private val readEntered: CountDownLatch,
    private val releaseRead: CountDownLatch,
) : VesperSequenceWarmupReadStream {
    override val cacheHit: Boolean = false

    override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        readEntered.countDown()
        check(releaseRead.await(5, TimeUnit.SECONDS))
        return -1
    }

    override fun close() = Unit
}

private class LateExpiredWarmupStream(
    private val release: CompletableDeferred<Unit>,
) : VesperSequenceWarmupReadStream {
    override val cacheHit: Boolean = false
    val closed = AtomicBoolean(false)

    override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        withContext(NonCancellable) { release.await() }
        throw VesperSequenceWarmupHttpStatusException(401)
    }

    override fun close() {
        closed.set(true)
    }
}
