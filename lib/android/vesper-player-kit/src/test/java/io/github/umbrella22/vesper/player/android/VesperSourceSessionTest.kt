package io.github.umbrella22.vesper.player.android

import java.nio.ByteBuffer
import java.nio.file.Files
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test

class VesperSourceSessionTest {
    private fun session(
        configuration: VesperSourceSessionConfiguration = VesperSourceSessionConfiguration(),
        cache: VesperDashStartupCache = VesperDashStartupCache(),
        clock: () -> Long = { 1_000 },
        dash: DashStartupTransport = DashStartupTransport { _, _, _, _ -> error("Unexpected DASH network") },
        transport: VesperSequenceWarmupTransport = VesperSequenceWarmupTransport { request ->
            object : VesperSequenceWarmupReadStream {
                override val cacheHit = false
                var remaining = request.length.toInt()
                override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int {
                    val count = minOf(remaining, length)
                    if (count == 0) return -1
                    buffer.fill(42, offset, offset + count); remaining -= count; return count
                }
                override fun close() {}
            }
        },
    ) = VesperSourceSession(configuration, transport, dash, cache, Dispatchers.IO, clock, clock)
    private fun source(headers: Map<String, String> = emptyMap()) = VesperPlayerSource.remote("https://media.test/a.mp4", "test", headers = headers)

    @Test fun independentPreloadAndLeasesPreserveIdentityAndDefensivelyCopyCredentials() = runBlocking {
        val headers = mutableMapOf("Authorization" to "one")
        val license = mutableMapOf("Cookie" to "one")
        val subtitles = mutableListOf(VesperExternalSubtitleSource("s", "https://test/s", headers = headers))
        val session = session()
        val handle = session.register(source(headers).copy(externalSubtitles = subtitles, drmConfiguration = VesperPlayerDrmConfiguration("x", "https://test", license)))
        headers["Authorization"] = "two"; license["Cookie"] = "two"; subtitles.clear()
        val first = handle.acquire(); val second = handle.acquire()
        assertSame(first.source.dashStartupScope, second.source.dashStartupScope)
        assertEquals("one", first.source.headers["Authorization"])
        assertEquals("one", first.source.drmConfiguration!!.licenseHeaders["Cookie"])
        assertEquals("one", first.source.externalSubtitles.single().headers["Authorization"])
        handle.close()
        first.checkValid()
        try { handle.acquire(); fail() } catch (_: IllegalStateException) {}
        session.close(); second.checkValid()
        session.invalidate()
        try { first.checkValid(); fail() } catch (_: IllegalStateException) {}
        first.close(); second.close()
    }

    @Test fun progressivePreloadIsBoundedScopedAndDoesNotClaimWholeFileReuse() = runBlocking {
        val cache = VesperDashStartupCache()
        val session = session(cache = cache)
        try {
            val one = session.register(source(mapOf("Authorization" to "one")))
            val two = session.register(source(mapOf("Authorization" to "two")))
            val options = VesperPreloadOptions(maximumBytes = 32)
            val result = one.preload(options).await()
            assertEquals(VesperPreloadState.Completed, result.state)
            assertEquals(32L, result.actualBytes)
            assertEquals(VesperPreloadReuse.DownloadOnly, result.reuse)
            val lease = one.acquire(); val other = two.acquire()
            assertNull(cache.read(lease.source.dashStartupScope!!, DashStartupResource(lease.source.uri), lease.source.headers))
            assertNotNull(cache.read(lease.source.dashStartupScope!!, DashStartupResource(lease.source.uri, 0, 32), lease.source.headers))
            assertNull(cache.read(other.source.dashStartupScope!!, DashStartupResource(other.source.uri, 0, 32), other.source.headers))
            lease.close(); other.close()
        } finally { session.close() }
    }

    @Test fun localDashRegistrationPreloadsWithoutControllerAndAcquisitionUsesSameCache() = runBlocking {
        val file = Files.createTempFile("session", ".mpd").toFile()
        val headers = mapOf("Referer" to "https://app.test")
        val cache = VesperDashStartupCache()
        val sidx = ByteBuffer.allocate(44).putInt(44).putInt(0x73696478).putInt(0).putInt(1).putInt(1000).putInt(0).putInt(0).putShort(0).putShort(1).putInt(20).putInt(1000).putInt(0).array()
        val calls = AtomicInteger()
        val session = session(cache = cache, dash = DashStartupTransport { resource, actual, _, _ ->
            assertEquals(headers, actual); calls.incrementAndGet()
            DashStartupBytes(resource, when(resource.position) { 10L -> sidx; 0L -> ByteArray(10); else -> ByteArray(20) })
        })
        try {
            file.writeText("""<MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><Representation codecs="avc1.640028" bandwidth="100"><BaseURL>https://media.test/v.mp4?token=one</BaseURL><SegmentBase indexRange="10-53"><Initialization range="0-9"/></SegmentBase></Representation></AdaptationSet></Period></MPD>""")
            val handle = session.register(VesperPlayerSource.localDash(file.toURI().toString(), "local", headers))
            val result = handle.preload().await()
            assertEquals(VesperPreloadState.Completed, result.state)
            assertEquals(VesperPreloadReuse.PlaybackReusable, result.reuse)
            assertEquals(3, calls.get())
            val lease = handle.acquire()
            assertNotNull(cache.read(lease.source.dashStartupScope!!, DashStartupResource("https://media.test/v.mp4?token=one", 54, 20), headers))
            lease.close()
        } finally { session.close(); file.delete() }
    }

    @Test fun cancelledWorkerRetainsCapacityUntilPhysicalExitAndQueuedCancelCompletes() = runBlocking {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val calls = AtomicInteger()
        val session = session(configuration = VesperSourceSessionConfiguration(maxConcurrentPreloads = 1, maxPendingPreloads = 1),
            transport = VesperSequenceWarmupTransport {
                calls.incrementAndGet(); entered.countDown(); release.await(5, TimeUnit.SECONDS)
                throw CancellationException()
            })
        try {
            val firstHandle = session.register(source())
            val first = firstHandle.preload()
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertSame(first, firstHandle.preload())
            val queued = session.register(source()).preload()
            first.cancel()
            assertEquals(VesperPreloadState.Cancelled, first.await().state)
            assertEquals(1, calls.get())
            try { session.register(source()).preload(); fail("Capacity released before worker exit") } catch (_: IllegalStateException) {}
            queued.cancel()
            assertEquals(VesperPreloadState.Cancelled, queued.await().state)
        } finally { release.countDown(); session.close() }
    }

    @Test fun expiryCapacityUnsupportedAndDisabledBudgetsAreExplicit() = runBlocking {
        var now = 1000L
        val session = session(configuration = VesperSourceSessionConfiguration(maxSources = 1), clock = { now })
        val handle = session.register(source(), 1001)
        try { session.register(source()); fail() } catch (_: IllegalStateException) {}
        now = 1001
        try { handle.preload(); fail() } catch (error: IllegalStateException) { assertEquals("source_expired", error.message) }
        now = 1000
        try { handle.acquire(); fail("Expired source revived after clock rollback") }
        catch (error: IllegalStateException) { assertEquals("source_expired", error.message) }
        handle.close()
        val unsupported = session.register(VesperPlayerSource.hls("https://test/a.m3u8", "hls"))
        val unsupportedResult = unsupported.preload().await()
        assertEquals(VesperPreloadState.Unsupported, unsupportedResult.state)
        assertEquals(VesperPreloadGoal.Unsupported, unsupportedResult.goal)
        assertEquals(VesperPreloadReuse.None, unsupportedResult.reuse)
        session.close()
        val disabled = session(configuration = VesperSourceSessionConfiguration(maxMemoryBytes = 0))
        assertEquals("cache_disabled", disabled.register(source()).preload().await().reasonCode)
        disabled.close()
    }

    @Test fun configurationRejectsUnboundedWorkAndDefaultProgressivePrefixIs64KiB() = runBlocking {
        for (invalid in listOf<() -> Unit>(
            { VesperSourceSessionConfiguration(maxSources = 513) },
            { VesperSourceSessionConfiguration(maxConcurrentPreloads = 5) },
            { VesperSourceSessionConfiguration(maxPendingPreloads = 33) },
            { VesperSourceSessionConfiguration(maxMemoryBytes = 16 * 1024 * 1024 + 1) },
            { VesperPreloadOptions(maximumBytes = 0) },
        )) {
            try { invalid(); fail("Invalid configuration accepted") } catch (_: IllegalArgumentException) {}
        }
        val session = session()
        try {
            val handle = session.register(source())
            assertEquals(64 * 1024L, handle.preload().await().actualBytes)
            val lease = handle.acquire()
            handle.close(); lease.sourceForActivation()
            handle.invalidate()
            try { lease.sourceForActivation(); fail("Revoked lease accepted") } catch (_: IllegalStateException) {}
            lease.close()
        } finally { session.close() }
    }

    @Test fun invalidationDuringDownloadFencesCommit() = runBlocking {
        val entered = CompletableDeferred<Unit>(); val release = CompletableDeferred<Unit>()
        val cache = VesperDashStartupCache()
        val session = session(cache = cache, transport = VesperSequenceWarmupTransport {
            entered.complete(Unit); release.await(); error("Must be cancelled")
        })
        val task = session.register(source()).preload()
        entered.await(); session.invalidate(); release.complete(Unit)
        assertEquals(VesperPreloadState.Cancelled, task.await().state)
        assertEquals(0, cache.inventory().first)
    }

    @Test fun deadlinesIncludeQueueTimeAndRetainThePhysicalWorkersByteReservation() = runBlocking {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val calls = AtomicInteger()
        val session = session(
            configuration = VesperSourceSessionConfiguration(maxConcurrentPreloads = 2, maxPendingPreloads = 1, maxMemoryBytes = 64),
            transport = VesperSequenceWarmupTransport {
                calls.incrementAndGet()
                entered.countDown()
                release.await(5, TimeUnit.SECONDS)
                throw CancellationException()
            },
        )
        try {
            val first = session.register(source()).preload(VesperPreloadOptions(maximumBytes = 64, timeoutMs = 200))
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val queued = session.register(source()).preload(VesperPreloadOptions(maximumBytes = 64, timeoutMs = 50))
            assertEquals(VesperPreloadState.Queued, queued.snapshot.state)
            val queueTimeout = withTimeout(2_000) { queued.await() }
            assertEquals("timeout", queueTimeout.reasonCode)
            assertEquals(VesperPreloadState.Failed, queueTimeout.state)
            assertEquals("timeout", withTimeout(2_000) { first.await() }.reasonCode)
            val third = session.register(source()).preload(VesperPreloadOptions(maximumBytes = 64))
            assertEquals(VesperPreloadState.Queued, third.snapshot.state)
            assertEquals(1, calls.get())
            try { session.register(source()).preload(); fail("Physical reservation was released too early") }
            catch (_: IllegalStateException) { }
            third.cancel()
        } finally { release.countDown(); session.close() }
    }
}
