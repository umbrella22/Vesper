package io.github.umbrella22.vesper.player.android.external.internal.relay

import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.Closeable
import java.io.IOException
import java.io.InputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import java.util.concurrent.Callable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.SocketPolicy
import org.junit.Assert.*
import org.junit.Test

class VesperRelayCancellationTest {
    private val source = VesperPlayerSource.dash("https://fixture.test/source.mpd", "Fixture")
    private val adaptation = VesperRelayFormatAdaptationRegistration(VesperRelayFallbackFormat.MpegTs)
    private fun sourceRelay(adapter: VesperRelayFormatAdapter) = VesperRelaySourceRelay(null, adapter, {}, false)

    @Test
    fun lateAdaptedInputIsClosedOnceWithoutReadingOrWritingAResponse() {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val reads = AtomicInteger()
        val closes = AtomicInteger()
        val auxiliaryCloses = AtomicInteger()
        val adapter = object : VesperRelayFormatAdapter {
            override fun open(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult {
                entered.countDown()
                check(release.await(5, TimeUnit.SECONDS))
                return VesperRelayFormatAdaptationResult.Stream(VesperRelayAdaptedStream(
                    input = object : InputStream() {
                        override fun read(): Int { reads.incrementAndGet(); return -1 }
                        override fun close() { closes.incrementAndGet() }
                    },
                    contentType = "video/mp2t",
                    closeable = Closeable { auxiliaryCloses.incrementAndGet() },
                ))
            }
        }
        val entry = RelayEntry(source, adaptation, null)
        val output = ByteArrayOutputStream()
        val executor = Executors.newSingleThreadExecutor()
        try {
            val response = executor.submit { sourceRelay(adapter).relay("token", entry, "media.ts", false, null, emptyMap(), output) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            entry.lifetime.close()
            release.countDown()
            response.get(5, TimeUnit.SECONDS)
            assertEquals(0, reads.get())
            assertEquals(0, output.size())
            assertEquals(1, closes.get())
            assertEquals(1, auxiliaryCloses.get())
        } finally {
            release.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun invalidationAndResponseFinallyCloseAdaptedInputExactlyOnce() {
        val readEntered = CountDownLatch(1)
        val readReleased = CountDownLatch(1)
        val closeEntered = CountDownLatch(1)
        val releaseClose = CountDownLatch(1)
        val closes = AtomicInteger()
        val input = object : InputStream() {
            override fun read(): Int {
                readEntered.countDown()
                check(readReleased.await(5, TimeUnit.SECONDS))
                throw IOException("cancelled fixture input")
            }
            override fun close() {
                closes.incrementAndGet()
                closeEntered.countDown()
                readReleased.countDown()
                check(releaseClose.await(5, TimeUnit.SECONDS))
            }
        }
        val adapter = object : VesperRelayFormatAdapter {
            override fun open(request: VesperRelayFormatAdaptationRequest) =
                VesperRelayFormatAdaptationResult.Stream(VesperRelayAdaptedStream(input, "video/mp2t"))
        }
        val entry = RelayEntry(source, adaptation, null)
        val executor = Executors.newFixedThreadPool(2)
        try {
            val response = executor.submit { sourceRelay(adapter).relay("token", entry, "media.ts", false, null, emptyMap(), ByteArrayOutputStream()) }
            assertTrue(readEntered.await(5, TimeUnit.SECONDS))
            val invalidation = executor.submit { entry.lifetime.close() }
            assertTrue(closeEntered.await(5, TimeUnit.SECONDS))
            response.get(5, TimeUnit.SECONDS)
            assertEquals(1, closes.get())
            releaseClose.countDown()
            invalidation.get(5, TimeUnit.SECONDS)
            assertEquals(1, closes.get())
        } finally {
            readReleased.countDown()
            releaseClose.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun retiredEntrySnapshotClosesSocketBeforeAdapterOpen() {
        val lookupEntered = CountDownLatch(1)
        val releaseLookup = CountDownLatch(1)
        val opens = AtomicInteger()
        val entry = RelayEntry(source, adaptation, null)
        val adapter = object : VesperRelayFormatAdapter {
            override fun open(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult {
                opens.incrementAndGet()
                return VesperRelayFormatAdaptationResult.Stream("unexpected".toByteArray().asRelayAdaptedStream("video/mp2t"))
            }
        }
        val executor = Executors.newSingleThreadExecutor()
        ServerSocket(0, 1, InetAddress.getLoopbackAddress()).use { listener ->
            Socket(InetAddress.getLoopbackAddress(), listener.localPort).use { client ->
                val peer = listener.accept()
                client.soTimeout = 5000
                val handler = VesperRelayClientHandler(AtomicBoolean(true), maxActiveClients = 4,
                    entryForToken = {
                        lookupEntered.countDown()
                        check(releaseLookup.await(5, TimeUnit.SECONDS))
                        entry
                    }, relaySource = sourceRelay(adapter))
                try {
                    handler.acceptClient(peer, executor)
                    client.getOutputStream().write("GET /media/token/media.ts HTTP/1.1\r\nHost: localhost\r\n\r\n".toByteArray())
                    assertTrue(lookupEntered.await(5, TimeUnit.SECONDS))
                    entry.lifetime.close()
                    releaseLookup.countDown()
                    assertEquals(-1, client.getInputStream().read())
                    assertEquals(0, opens.get())
                } finally {
                    releaseLookup.countDown()
                    handler.closeActiveClients()
                    executor.shutdownNow()
                }
            }
        }
    }

    @Test
    fun tokenCancellationInterruptsStalledUpstreamHeaders() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.NO_RESPONSE))
            server.start()
            val owner = VesperRelayLifetime()
            val executor = Executors.newSingleThreadExecutor()
            try {
                val result = executor.submit(Callable<Throwable?> {
                    try {
                        VesperRelayHttpTransport(true).open(server.url("/stalled").toString(), "GET", lifetime = owner).close()
                        null
                    } catch (error: Throwable) { error }
                })
                assertNotNull(server.takeRequest(5, TimeUnit.SECONDS))
                owner.close()
                assertTrue(result.get(1, TimeUnit.SECONDS) is IOException)
            } finally {
                owner.close()
                executor.shutdownNow()
            }
        }
    }

    @Test
    fun cancellingOneHttpTokenTruncatesItWhileAnotherTokenCompletes() {
        val readingA = CountDownLatch(1)
        val readingB = CountDownLatch(1)
        val unblockA = CountDownLatch(1)
        val unblockB = CountDownLatch(1)
        val closedB = CountDownLatch(1)
        val closesA = AtomicInteger()
        val closesB = AtomicInteger()
        val adapter = object : VesperRelayFormatAdapter {
            override fun open(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult {
                val isA = request.source.label == "A"
                val data = ByteArrayInputStream("complete B".toByteArray())
                val input = object : InputStream() {
                    override fun read(): Int {
                        (if (isA) readingA else readingB).countDown()
                        check((if (isA) unblockA else unblockB).await(5, TimeUnit.SECONDS))
                        if (isA) throw IOException("cancelled A")
                        return data.read()
                    }
                    override fun close() {
                        (if (isA) closesA else closesB).incrementAndGet()
                        if (isA) unblockA.countDown() else closedB.countDown()
                    }
                }
                return VesperRelayFormatAdaptationResult.Stream(VesperRelayAdaptedStream(input, "video/mp2t", 10))
            }
        }
        val loopback = InetAddress.getByName("127.0.0.1")
        val relay = VesperRelayServer(advertisedAddressProvider = { loopback }, bindAddressProvider = { loopback }, formatAdapter = adapter)
        val executor = Executors.newFixedThreadPool(2)
        try {
            val a = relay.register(VesperPlayerSource.dash(source.uri, "A"), adaptation)
            val b = relay.register(VesperPlayerSource.dash(source.uri, "B"), adaptation)
            fun request(url: String) = executor.submit(Callable {
                val connection = URL(url).openConnection() as java.net.HttpURLConnection
                connection.readTimeout = 5000
                try { connection.inputStream.use { it.readBytes().toString(Charsets.UTF_8) } }
                catch (_: IOException) { "truncated" }
                finally { connection.disconnect() }
            })
            val responseA = request(a.url)
            val responseB = request(b.url)
            assertTrue(readingA.await(5, TimeUnit.SECONDS))
            assertTrue(readingB.await(5, TimeUnit.SECONDS))
            relay.invalidate(a.token)
            assertNotEquals("complete B", responseA.get(5, TimeUnit.SECONDS))
            assertEquals(1, closesA.get())
            assertEquals(0, closesB.get())
            unblockB.countDown()
            assertEquals("complete B", responseB.get(5, TimeUnit.SECONDS))
            // Receiving all bytes can precede cleanup on the relay request executor.
            assertTrue("Uncancelled token should close after its response", closedB.await(5, TimeUnit.SECONDS))
            assertEquals(1, closesB.get())
        } finally {
            unblockA.countDown()
            unblockB.countDown()
            relay.stop()
            executor.shutdownNow()
        }
    }
}
