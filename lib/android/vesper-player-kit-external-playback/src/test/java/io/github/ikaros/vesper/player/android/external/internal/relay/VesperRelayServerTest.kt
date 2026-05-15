package io.github.ikaros.vesper.player.android.external.internal.relay

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import java.io.File
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import java.util.Collections
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
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
    private var upstream: RecordingHttpServer? = null

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
        assertTrue(handle.url.endsWith("/video.mp4"))

        val head = request(handle.url, method = "HEAD")
        assertEquals(200, head.status)
        assertEquals("", head.body)
        assertEquals("video/mp4", head.headers["Content-Type"]?.firstOrNull())
        assertEquals("Streaming", head.headerValue("transferMode.dlna.org"))
        assertTrue(
            head.headerValue("contentFeatures.dlna.org")
                ?.contains("DLNA.ORG_OP=01") == true,
        )

        val range = request(handle.url, headers = mapOf("Range" to "bytes=2-5"))
        assertEquals(206, range.status)
        assertEquals("cdef", range.body)
        assertEquals("bytes 2-5/10", range.headers["Content-Range"]?.firstOrNull())

        val upstreamRange = requests.last()
        assertEquals("bytes=2-5", upstreamRange.headerValue("Range"))
        assertEquals("https://example.com/player", upstreamRange.headerValue("Referer"))
        assertEquals("VesperRelayTest", upstreamRange.headerValue("User-Agent"))
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
    fun expiresTokensByTtlLazily() {
        var nowMillis = 1_000L
        val ttlRelay = VesperRelayServer(
            advertisedAddressProvider = { loopback },
            bindAddressProvider = { loopback },
            tokenTtlMillis = 50L,
            nowMillisProvider = { nowMillis },
        )
        try {
            val file = File.createTempFile("vesper-relay", ".mp4")
            file.writeText("data")
            file.deleteOnExit()
            val handle = ttlRelay.register(
                VesperPlayerSource.local(uri = file.absolutePath, label = "Local"),
            )

            assertEquals(200, request(handle.url).status)
            nowMillis += 51L

            assertEquals(404, request(handle.url).status)
        } finally {
            ttlRelay.stop()
        }
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
    fun sourcePreparerAdaptsDlnaDashWhenEnabled() {
        val preparer = VesperExternalPlaybackSourcePreparer(relay)
        val dash = VesperPlayerSource.dash(
            uri = "https://example.com/video.mpd",
            label = "Episode",
            headers = mapOf("Cookie" to "secret"),
        )

        val prepared = preparer.prepare(
            VesperExternalSourcePreparationRequest(
                target = VesperExternalPlaybackTarget.Dlna,
                sources = listOf(dash),
                capabilities = VesperExternalRouteCapabilities(
                    supportsProgressive = true,
                    supportsHls = true,
                    supportsMpegTs = true,
                ),
                formatAdaptation = VesperRelayFormatAdaptationConfig(
                    enabled = true,
                    preferredFallback = VesperRelayFallbackFormat.Hls,
                ),
                routeId = "uuid:tv",
                routeName = "Living Room TV",
            ),
        ) as VesperExternalSourcePreparationResult.Prepared

        assertTrue(prepared.relayEnabled)
        assertEquals(VesperRelayFallbackFormat.Hls, prepared.adaptedFormat)
        assertEquals(VesperPlayerSourceProtocol.Hls, prepared.source.protocol)
        assertTrue(prepared.source.uri.endsWith(".m3u8"))
    }

    @Test
    fun sourcePreparerReportsUnsupportedDlnaRemuxCaps() {
        val preparer = VesperExternalPlaybackSourcePreparer(relay)
        val result = preparer.prepare(
            VesperExternalSourcePreparationRequest(
                target = VesperExternalPlaybackTarget.Dlna,
                sources = listOf(
                    VesperPlayerSource.dash(
                        uri = "https://example.com/video.mpd",
                        label = "Episode",
                    ),
                ),
                capabilities = VesperExternalRouteCapabilities(
                    supportsHls = false,
                    supportsMpegTs = false,
                ),
                formatAdaptation = VesperRelayFormatAdaptationConfig(enabled = true),
                routeId = "uuid:tv",
                routeName = "Living Room TV",
            ),
        ) as VesperExternalSourcePreparationResult.Unsupported

        assertEquals("unsupported_device_caps", result.code)
        assertEquals("false", result.details["supportsHls"])
        assertEquals("false", result.details["supportsMpegTs"])
        assertEquals("uuid:tv", result.details["routeId"])
    }

    @Test
    fun relayServerServesAdaptedStreamsWithRangeAndDiagnostics() {
        val diagnostics = Collections.synchronizedList(mutableListOf<VesperRelayDiagnostic>())
        val adapter = RecordingFormatAdapter()
        val adaptedRelay = VesperRelayServer(
            advertisedAddressProvider = { loopback },
            bindAddressProvider = { loopback },
            formatAdapter = adapter,
            diagnosticListener = diagnostics::add,
        )
        try {
            val handle = adaptedRelay.register(
                VesperPlayerSource.dash(
                    uri = "https://example.com/video.mpd",
                    label = "Episode",
                    headers = mapOf("Cookie" to "secret"),
                ),
                VesperRelayFormatAdaptationRegistration(
                    fallbackFormat = VesperRelayFallbackFormat.MpegTs,
                    config = VesperRelayFormatAdaptationConfig(
                        enabled = true,
                        enableRangeCache = true,
                    ),
                    routeId = "uuid:tv",
                    routeName = "Living Room TV",
                ),
            )

            val range = request(handle.url, headers = mapOf("Range" to "bytes=2-5"))

            assertEquals(206, range.status)
            assertEquals("2345", range.body)
            assertEquals("video/mp2t", range.headers["Content-Type"]?.firstOrNull())
            assertEquals("bytes 2-5/10", range.headers["Content-Range"]?.firstOrNull())
            assertEquals("Episode.ts", handle.url.substringAfterLast('/'))
            assertEquals("bytes=2-5", adapter.requests.single().range?.toHeaderValue())

            val invalid = request(handle.url, headers = mapOf("Range" to "bytes=99-100"))

            assertEquals(416, invalid.status)
            assertTrue(invalid.body.contains("range_not_ready"))
            assertEquals("range_not_ready", diagnostics.single().code)
        } finally {
            adaptedRelay.stop()
        }
    }

    @Test
    fun relayServerServesHlsAdaptedPlaylistWithCompatibleMime() {
        val adaptedRelay = VesperRelayServer(
            advertisedAddressProvider = { loopback },
            bindAddressProvider = { loopback },
            formatAdapter = RecordingFormatAdapter(),
        )
        try {
            val handle = adaptedRelay.register(
                VesperPlayerSource.dash(
                    uri = "https://example.com/video.mpd",
                    label = "Episode",
                ),
                VesperRelayFormatAdaptationRegistration(
                    fallbackFormat = VesperRelayFallbackFormat.Hls,
                    config = VesperRelayFormatAdaptationConfig(enabled = true),
                ),
            )

            val response = request(handle.url)

            assertEquals(200, response.status)
            assertEquals("application/x-mpegURL", response.headers["Content-Type"]?.firstOrNull())
        } finally {
            adaptedRelay.stop()
        }
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

    private fun startUpstream(requests: MutableList<RecordedRequest>): RecordingHttpServer {
        val server = RecordingHttpServer(loopback, requests)
        server.start()
        return server
    }
}

private class RecordingFormatAdapter : VesperRelayFormatAdapter {
    val requests = Collections.synchronizedList(mutableListOf<VesperRelayFormatAdaptationRequest>())
    private val payload = "0123456789".toByteArray()

    override fun open(
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayFormatAdaptationResult {
        requests += request
        val resolved = request.range?.resolve(payload.size.toLong())
        if (request.range != null && resolved == null) {
            return VesperRelayFormatAdaptationResult.Failure(
                status = 416,
                diagnostic = VesperRelayDiagnostic(
                    code = "range_not_ready",
                    message = "Requested adapted range is not available.",
                    details = mapOf("sessionId" to request.sessionId),
                ),
            )
        }
        val body = if (resolved == null) {
            payload
        } else {
            payload.copyOfRange(resolved.start.toInt(), (resolved.end + 1).toInt())
        }
        val headers = if (resolved == null) {
            emptyMap()
        } else {
            mapOf("Content-Range" to "bytes ${resolved.start}-${resolved.end}/${payload.size}")
        }
        return VesperRelayFormatAdaptationResult.Stream(
            body.asRelayAdaptedStream(
                contentType = request.fallbackFormat.contentType(),
                status = if (resolved == null) 200 else 206,
                headers = headers,
            ),
        )
    }
}

private data class RecordedRequest(
    val method: String,
    val headers: Map<String, String>,
)

private fun RecordedRequest.headerValue(name: String): String? = headers.valueFor(name)

private data class HttpResponse(
    val status: Int,
    val headers: Map<String, List<String>>,
    val body: String,
)

private fun HttpResponse.headerValue(name: String): String? =
    headers.entries
        .firstOrNull { (key, _) -> key.equals(name, ignoreCase = true) }
        ?.value
        ?.firstOrNull()

private class RecordingHttpServer(
    bindAddress: InetAddress,
    private val requests: MutableList<RecordedRequest>,
) {
    private val running = AtomicBoolean(false)
    private val serverSocket = ServerSocket(0, 50, bindAddress)
    private val thread = Thread(::run, "vesper-relay-test-upstream").apply { isDaemon = true }

    val address: InetSocketAddress
        get() = InetSocketAddress(serverSocket.inetAddress, serverSocket.localPort)

    fun start() {
        if (running.compareAndSet(false, true)) {
            thread.start()
        }
    }

    fun stop(delaySeconds: Int = 0) {
        running.set(false)
        serverSocket.close()
        if (delaySeconds <= 0) {
            thread.join(1_000)
        }
    }

    private fun run() {
        while (running.get()) {
            val socket = try {
                serverSocket.accept()
            } catch (_: Exception) {
                break
            }
            Thread({ handle(socket) }, "vesper-relay-test-upstream-request").apply {
                isDaemon = true
                start()
            }
        }
    }

    private fun handle(socket: Socket) {
        socket.use { client ->
            val input = client.getInputStream().bufferedReader(Charsets.ISO_8859_1)
            val requestLine = input.readLine() ?: return
            val method = requestLine.substringBefore(' ')
            val headers = linkedMapOf<String, String>()
            while (true) {
                val line = input.readLine() ?: break
                if (line.isEmpty()) {
                    break
                }
                val separator = line.indexOf(':')
                if (separator > 0) {
                    headers[line.substring(0, separator)] = line.substring(separator + 1).trim()
                }
            }
            requests += RecordedRequest(method = method, headers = headers)

            val payload = "abcdefghij".toByteArray()
            val range = headers.valueFor("Range")
            val body: ByteArray
            val status: Int
            val extraHeaders = linkedMapOf<String, String>()
            if (range == "bytes=2-5") {
                body = "cdef".toByteArray()
                status = 206
                extraHeaders["Content-Range"] = "bytes 2-5/10"
            } else {
                body = payload
                status = 200
            }

            val responseBody = if (method == "HEAD") ByteArray(0) else body
            val response = buildString {
                append("HTTP/1.1 ").append(status).append(if (status == 206) " Partial Content" else " OK")
                    .append("\r\n")
                append("Content-Type: video/mp4\r\n")
                append("Accept-Ranges: bytes\r\n")
                append("Content-Length: ").append(body.size).append("\r\n")
                extraHeaders.forEach { (key, value) ->
                    append(key).append(": ").append(value).append("\r\n")
                }
                append("Connection: close\r\n")
                append("\r\n")
            }.toByteArray(Charsets.ISO_8859_1)
            client.getOutputStream().use { output ->
                output.write(response)
                output.write(responseBody)
            }
        }
    }
}

private fun Map<String, String>.valueFor(name: String): String? =
    entries.firstOrNull { (key, _) -> key.equals(name, ignoreCase = true) }?.value

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
