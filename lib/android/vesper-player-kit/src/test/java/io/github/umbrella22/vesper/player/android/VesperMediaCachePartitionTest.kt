package io.github.umbrella22.vesper.player.android

import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import com.sun.net.httpserver.HttpServer
import java.io.ByteArrayOutputStream
import java.net.InetSocketAddress
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], manifest = Config.NONE)
class VesperMediaCachePartitionTest {
    @Test
    fun progressiveCachePartitionsEffectiveCredentialsWithoutChangingRequests() {
        val requests = AtomicInteger()
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/video") { exchange ->
            requests.incrementAndGet()
            val identity = exchange.requestHeaders.getFirst("Authorization")
            val cookie = exchange.requestHeaders.getFirst("Cookie") ?: "none"
            val body = "$identity:$cookie".toByteArray()
            exchange.sendResponseHeaders(200, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
        server.start()
        try {
            val uri = "http://127.0.0.1:${server.address.port}/video?signed=unchanged"
            val context = RuntimeEnvironment.getApplication()
            val policy = NativeCachePolicy(0, true, 2 * 1024 * 1024, true, 2 * 1024 * 1024)
            val a = buildDataSourceFactory(context, policy, mapOf("Authorization" to "Bearer A"))
            val b = buildDataSourceFactory(context, policy, mapOf("Authorization" to "Bearer B"))
            repeat(2) { assertArrayEquals("Bearer A:none".toByteArray(), read(a, uri)) }
            assertEquals(1, requests.get())
            repeat(2) { assertArrayEquals("Bearer B:none".toByteArray(), read(b, uri)) }
            assertEquals("B must miss A's cache and then reuse its own bytes", 2, requests.get())
            assertArrayEquals("Bearer B:none".toByteArray(), read(a, uri, mapOf("Authorization" to "Bearer B")))
            assertEquals("same-spelling request overrides use the upstream identity", 2, requests.get())
            val cookie = buildDataSourceFactory(context, policy,
                mapOf("Authorization" to "Bearer B", "Cookie" to "session=C"))
            assertArrayEquals("Bearer B:session=C".toByteArray(), read(cookie, uri))
            assertEquals(3, requests.get())
            val keys = VesperMediaCacheStore.cache(context, policy.maxDiskBytes).keys
            org.junit.Assert.assertFalse(keys.any { it.contains("Bearer") || it.contains("session=C") || it.contains(uri) })
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun playlistCacheUsesOpaqueCredentialAwareKeysWithoutChangingSignedRequests() {
        val requests = AtomicInteger()
        val requestTargets = java.util.concurrent.CopyOnWriteArrayList<String>()
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/playlist") { exchange ->
            requests.incrementAndGet()
            requestTargets.add(exchange.requestURI.toString())
            val body = (exchange.requestHeaders.getFirst("Authorization") ?: "anonymous").toByteArray()
            exchange.sendResponseHeaders(200, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
        server.start()
        try {
            val uri = "http://127.0.0.1:${server.address.port}/playlist?signature=signed-secret"
            val context = RuntimeEnvironment.getApplication()
            val policy = NativeCachePolicy(0, true, 2 * 1024 * 1024, true, 2 * 1024 * 1024)
            val factory = buildPlaylistDataSourceFactory(context, policy)
            repeat(2) { assertArrayEquals("anonymous".toByteArray(), read(factory, uri)) }
            repeat(2) {
                assertArrayEquals("Bearer B".toByteArray(), read(factory, uri, mapOf("Authorization" to "Bearer B")))
            }
            assertEquals("Playlist requests must reuse only matching cache entries", 2, requests.get())
            assertEquals(listOf("/playlist?signature=signed-secret", "/playlist?signature=signed-secret"), requestTargets)
            val dataSource = factory.createDataSource() as androidx.media3.datasource.cache.CacheDataSource
            try {
                val keys = dataSource.cache.keys
                org.junit.Assert.assertTrue("Playlist keys must be opaque v2 identities", keys.all { it.matches(Regex("v2\\.[0-9a-f]{64}")) })
                org.junit.Assert.assertFalse(keys.any { it.contains("signed-secret") || it.contains("Bearer") })
            } finally {
                dataSource.close()
            }
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun coldCacheKeepsConflictingCaseCredentialsSeparateFromSingleCredentialRequests() {
        val requests = AtomicInteger()
        val requestTargets = java.util.concurrent.CopyOnWriteArrayList<String>()
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/cold") { exchange ->
            requests.incrementAndGet()
            requestTargets.add(exchange.requestURI.toString())
            val body = (exchange.requestHeaders.getFirst("Cookie") ?: "anonymous").toByteArray()
            exchange.sendResponseHeaders(200, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
        server.start()
        try {
            val uri = "http://127.0.0.1:${server.address.port}/cold?signature=unchanged"
            val context = RuntimeEnvironment.getApplication()
            val policy = NativeCachePolicy(0, true, 2 * 1024 * 1024, true, 2 * 1024 * 1024)
            val defaults = mapOf("cookie" to "session=A")
            val request = mapOf("Cookie" to "session=B")
            val upstream = buildUpstreamDataSourceFactory(context, defaults)
            val conflicting = buildDataSourceFactory(context, policy, defaults)
            val single = buildDataSourceFactory(context, policy, request)
            val wireResponse = read(upstream, uri, request)
            repeat(2) { assertArrayEquals("Caching must preserve the original transport request", wireResponse, read(conflicting, uri, request)) }
            assertEquals("The conflicting request must reuse only its own cold entry", 2, requests.get())
            repeat(2) { assertArrayEquals("session=B".toByteArray(), read(single, uri)) }
            assertEquals("A single credential must miss the conflicting request's entry", 3, requests.get())
            assertEquals(List(3) { "/cold?signature=unchanged" }, requestTargets)
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun coldCachePreservesConflictingHeaderInputOrder() {
        val requests = AtomicInteger()
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/ordered") { exchange ->
            requests.incrementAndGet()
            val body = (exchange.requestHeaders["Cookie"]?.joinToString("|") ?: "anonymous").toByteArray()
            exchange.sendResponseHeaders(200, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
        server.start()
        try {
            val uri = "http://127.0.0.1:${server.address.port}/ordered"
            val context = RuntimeEnvironment.getApplication()
            val policy = NativeCachePolicy(0, true, 2 * 1024 * 1024, true, 2 * 1024 * 1024)
            val firstHeaders = linkedMapOf("cookie" to "session=A", "cookiE" to "session=B")
            val secondHeaders = linkedMapOf("cookiE" to "session=B", "cookie" to "session=A")
            val firstWire = read(buildUpstreamDataSourceFactory(context, firstHeaders), uri)
            val secondWire = read(buildUpstreamDataSourceFactory(context, secondHeaders), uri)
            org.junit.Assert.assertFalse("Conflicting header input order reaches the pinned transport", firstWire.contentEquals(secondWire))
            val first = buildDataSourceFactory(context, policy, firstHeaders)
            val second = buildDataSourceFactory(context, policy, secondHeaders)
            repeat(2) { assertArrayEquals(firstWire, read(first, uri)) }
            repeat(2) { assertArrayEquals(secondWire, read(second, uri)) }
            assertEquals("Each transport input must retain its own cache entry", 4, requests.get())
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun dashStartupDoesNotReplayWarmupBytesForConflictingHeaderSpellings() {
        val requests = AtomicInteger()
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/startup") { exchange ->
            requests.incrementAndGet()
            val body = (exchange.requestHeaders["Cookie"]?.joinToString("|") ?: "anonymous").toByteArray()
            exchange.sendResponseHeaders(200, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
        server.start()
        try {
            val uri = "http://127.0.0.1:${server.address.port}/startup"
            val context = RuntimeEnvironment.getApplication()
            val headers = linkedMapOf("cookie" to "session=A", "Cookie" to "session=B")
            val upstream = buildUpstreamDataSourceFactory(context, headers)
            val wireResponse = read(upstream, uri)
            val scope = DashStartupScope(namespace = "conflicting-header-spellings")
            val cache = VesperDashStartupCache()
            cache.store(scope, listOf(DashStartupBytes(DashStartupResource(uri), "different-transport".toByteArray())), headers)
            val factory = DataSource.Factory {
                VesperDashStartupDataSource(upstream.createDataSource(), scope, headers, cache)
            }
            assertArrayEquals("Ambiguous names cannot replay bytes from the warmup transport", wireResponse, read(factory, uri))
            assertEquals("The formal transport must resolve ambiguous properties itself", 2, requests.get())
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun cacheIdentityPreservesTransportHeaderProperties() {
        val uri = "https://example.com/video"
        fun key(defaults: Map<String, String>, request: Map<String, String> = emptyMap()) =
            mediaResourceCacheKey("test", uri, defaults, requestHeaders = request)
        val request = mapOf("Cookie" to "session=B")
        org.junit.Assert.assertNotEquals(
            "Differently-cased default and request properties must not collapse to the request alone",
            key(mapOf("cookie" to "session=A"), request),
            key(request),
        )
        org.junit.Assert.assertNotEquals(
            "Conflicting properties must retain which spelling carries each value",
            key(mapOf("Cookie" to "session=B", "cookie" to "session=A")),
            key(mapOf("Cookie" to "session=A", "cookie" to "session=B")),
        )
        assertEquals("Exact-spelling request properties replace factory defaults",
            key(request), key(mapOf("Cookie" to "session=A"), request))
        assertEquals("Unambiguous properties can reuse bytes regardless of map insertion order",
            key(mapOf("Authorization" to "Bearer B", "Cookie" to "session=A")),
            key(mapOf("Cookie" to "session=A", "Authorization" to "Bearer B")))
        org.junit.Assert.assertNotEquals("Conflicting spellings retain original input order",
            key(linkedMapOf("Cookie" to "session=B", "cookie" to "session=A")),
            key(linkedMapOf("cookie" to "session=A", "Cookie" to "session=B")))
        org.junit.Assert.assertNotEquals("Conflicting properties retain their factory/request origin",
            key(mapOf("cookie" to "session=A"), request),
            key(mapOf("cookie" to "session=A", "Cookie" to "session=B")))
    }

    @Test
    fun cacheIdentitySeparatesHttpMethodsAndBodies() {
        val uri = "https://example.com/video"
        fun key(method: Int, body: ByteArray?) = mediaResourceCacheKey("test", uri, emptyMap(), method, body)
        val keys = listOf(
            key(DataSpec.HTTP_METHOD_GET, null),
            key(DataSpec.HTTP_METHOD_POST, null),
            key(DataSpec.HTTP_METHOD_POST, byteArrayOf()),
            key(DataSpec.HTTP_METHOD_POST, "A".toByteArray()),
            key(DataSpec.HTTP_METHOD_POST, "B".toByteArray()),
        )
        assertEquals("Method and body are part of the request identity", keys.size, keys.toSet().size)
    }

    private fun read(factory: DataSource.Factory, uri: String, headers: Map<String, String> = emptyMap()): ByteArray {
        val source = factory.createDataSource()
        try {
            source.open(DataSpec.Builder().setUri(uri).setHttpRequestHeaders(headers).build())
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(32)
            while (true) {
                val count = source.read(buffer, 0, buffer.size)
                if (count == -1) break
                output.write(buffer, 0, count)
            }
            return output.toByteArray()
        } finally { source.close() }
    }
}
