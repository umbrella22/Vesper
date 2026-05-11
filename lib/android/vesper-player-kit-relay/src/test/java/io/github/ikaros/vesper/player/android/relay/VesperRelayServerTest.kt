package io.github.ikaros.vesper.player.android.relay

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import java.io.File
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.URL
import java.util.Collections
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperRelayServerTest {
    private val loopback: InetAddress = InetAddress.getByName("127.0.0.1")
    private val relay = VesperRelayServer(
        advertisedAddressProvider = { loopback },
        bindAddressProvider = { loopback },
    )
    private var upstream: HttpServer? = null

    @After
    fun tearDown() {
        relay.stop()
        upstream?.stop(0)
        upstream = null
    }

    @Test
    fun forwardsGetHeadRangeAndSourceHeaders() {
        val requests = Collections.synchronizedList(mutableListOf<RecordedRequest>())
        upstream = startUpstream(requests)
        val source = VesperPlayerSource.remote(
            uri = "http://127.0.0.1:${upstream!!.address.port}/video.mp4",
            label = "Remote",
            protocol = VesperPlayerSourceProtocol.Progressive,
            headers = mapOf(
                "Referer" to "https://example.com/player",
                "User-Agent" to "VesperRelayTest",
            ),
        )
        val handle = relay.register(source)

        val head = request(handle.url, method = "HEAD")
        assertEquals(200, head.status)
        assertEquals("", head.body)
        assertEquals("video/mp4", head.headers["Content-Type"]?.firstOrNull())

        val range = request(handle.url, headers = mapOf("Range" to "bytes=2-5"))
        assertEquals(206, range.status)
        assertEquals("cdef", range.body)
        assertEquals("bytes 2-5/10", range.headers["Content-Range"]?.firstOrNull())

        val upstreamRange = requests.last()
        assertEquals("bytes=2-5", upstreamRange.headers["Range"])
        assertEquals("https://example.com/player", upstreamRange.headers["Referer"])
        assertEquals("VesperRelayTest", upstreamRange.headers["User-agent"])
    }

    @Test
    fun servesLocalFileRangesAndRejectsInvalidRanges() {
        val file = File.createTempFile("vesper-relay", ".mp4")
        file.writeText("0123456789")
        file.deleteOnExit()
        val handle = relay.register(
            VesperPlayerSource.local(uri = file.absolutePath, label = "Local"),
        )

        val range = request(handle.url, headers = mapOf("Range" to "bytes=4-8"))
        assertEquals(206, range.status)
        assertEquals("45678", range.body)
        assertEquals("bytes 4-8/10", range.headers["Content-Range"]?.firstOrNull())

        val invalid = request(handle.url, headers = mapOf("Range" to "bytes=100-200"))
        assertEquals(416, invalid.status)
        assertEquals("bytes */10", invalid.headers["Content-Range"]?.firstOrNull())
    }

    @Test
    fun rejectsExpiredToken() {
        val file = File.createTempFile("vesper-relay", ".mp4")
        file.writeText("data")
        file.deleteOnExit()
        val handle = relay.register(VesperPlayerSource.local(uri = file.absolutePath, label = "Local"))

        assertEquals(200, request(handle.url).status)
        relay.invalidate(handle.token)

        assertEquals(404, request(handle.url).status)
    }

    @Test
    fun handlesConcurrentRangeRequests() {
        val file = File.createTempFile("vesper-relay", ".mp4")
        file.writeText("abcdefghijklmnopqrstuvwxyz")
        file.deleteOnExit()
        val handle = relay.register(VesperPlayerSource.local(uri = file.absolutePath, label = "Local"))
        val executor = Executors.newFixedThreadPool(4)
        try {
            val futures = (0 until 8).map { index ->
                executor.submit(
                    Callable {
                        val start = index * 2
                        request(handle.url, headers = mapOf("Range" to "bytes=$start-${start + 1}"))
                    },
                )
            }
            val bodies = futures.map { it.get().body }
            assertEquals(listOf("ab", "cd", "ef", "gh", "ij", "kl", "mn", "op"), bodies)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun sourcePreparerRelaysHeadersAndRejectsDashRelay() {
        val preparer = VesperExternalPlaybackSourcePreparer(relay)
        val hls = VesperPlayerSource.hls(
            uri = "https://example.com/video.m3u8",
            label = "HLS",
            headers = mapOf("Cookie" to "secret"),
        )
        val prepared = preparer.prepare(
            VesperExternalSourcePreparationRequest(
                target = VesperExternalPlaybackTarget.Cast,
                sources = listOf(hls),
                capabilities = VesperExternalRouteCapabilities(
                    supportsProgressive = true,
                    supportsHls = true,
                    supportsDash = true,
                ),
            ),
        ) as VesperExternalSourcePreparationResult.Prepared

        assertTrue(prepared.relayEnabled)
        assertNotNull(prepared.relayToken)
        assertTrue(prepared.source.uri.startsWith("http://127.0.0.1:"))
        assertTrue(prepared.source.headers.isEmpty())

        val dash = VesperPlayerSource.dash(
            uri = "https://example.com/video.mpd",
            label = "DASH",
            headers = mapOf("Cookie" to "secret"),
        )
        val rejected = preparer.prepare(
            VesperExternalSourcePreparationRequest(
                target = VesperExternalPlaybackTarget.Cast,
                sources = listOf(dash),
                capabilities = VesperExternalRouteCapabilities(
                    supportsProgressive = true,
                    supportsHls = true,
                    supportsDash = true,
                ),
            ),
        )
        assertTrue(rejected is VesperExternalSourcePreparationResult.Unsupported)
    }

    @Test
    fun sourcePreparerHonorsProxyNever() {
        val preparer = VesperExternalPlaybackSourcePreparer(relay)
        val source = VesperPlayerSource.remote(
            uri = "https://example.com/video.mp4",
            label = "Remote",
            headers = mapOf("Referer" to "https://example.com"),
        )

        val result = preparer.prepare(
            VesperExternalSourcePreparationRequest(
                target = VesperExternalPlaybackTarget.Dlna,
                sources = listOf(source),
                proxyPolicy = VesperExternalProxyPolicy.Never,
                capabilities = VesperExternalRouteCapabilities(supportsProgressive = true),
            ),
        )

        assertFalse(result is VesperExternalSourcePreparationResult.Prepared)
    }

    private fun startUpstream(requests: MutableList<RecordedRequest>): HttpServer {
        val server = HttpServer.create(InetSocketAddress(loopback, 0), 0)
        server.createContext("/video.mp4") { exchange ->
            requests += exchange.recordedRequest()
            val payload = "abcdefghij".toByteArray()
            val range = exchange.requestHeaders.getFirst("Range")
            val body: ByteArray
            val status: Int
            if (range == "bytes=2-5") {
                body = "cdef".toByteArray()
                status = 206
                exchange.responseHeaders.add("Content-Range", "bytes 2-5/10")
            } else {
                body = payload
                status = 200
            }
            exchange.responseHeaders.add("Content-Type", "video/mp4")
            exchange.responseHeaders.add("Accept-Ranges", "bytes")
            exchange.sendResponseHeaders(status, if (exchange.requestMethod == "HEAD") -1 else body.size.toLong())
            if (exchange.requestMethod != "HEAD") {
                exchange.responseBody.use { it.write(body) }
            } else {
                exchange.close()
            }
        }
        server.start()
        return server
    }
}

private data class RecordedRequest(
    val method: String,
    val headers: Map<String, String>,
)

private data class HttpResponse(
    val status: Int,
    val headers: Map<String, List<String>>,
    val body: String,
)

private fun HttpExchange.recordedRequest(): RecordedRequest =
    RecordedRequest(
        method = requestMethod,
        headers = requestHeaders.entries.associate { (key, value) ->
            key to (value.firstOrNull() ?: "")
        },
    )

private fun request(
    url: String,
    method: String = "GET",
    headers: Map<String, String> = emptyMap(),
): HttpResponse {
    val connection = URL(url).openConnection() as HttpURLConnection
    connection.requestMethod = method
    headers.forEach(connection::setRequestProperty)
    val status = connection.responseCode
    val stream = runCatching { connection.inputStream }.getOrElse { connection.errorStream }
    val body = if (stream == null || method == "HEAD") {
        ""
    } else {
        stream.bufferedReader().use { it.readText() }
    }
    return HttpResponse(
        status = status,
        headers = connection.headerFields
            .filterKeys { it != null }
            .mapKeys { it.key!! },
        body = body,
    )
}
