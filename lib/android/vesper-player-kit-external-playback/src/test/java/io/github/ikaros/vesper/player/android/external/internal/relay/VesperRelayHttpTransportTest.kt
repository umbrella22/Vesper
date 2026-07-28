package io.github.ikaros.vesper.player.android.external.internal.relay

import java.net.InetAddress
import java.util.concurrent.atomic.AtomicInteger
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class VesperRelayHttpTransportTest {
    @Test
    fun relativeSameOriginRedirectPreservesCredentials() {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse()
                    .setResponseCode(302)
                    .addHeader("Location", "/final"),
            )
            server.enqueue(MockResponse().setBody("media"))
            server.start()

            val transport = VesperRelayHttpTransport(allowPrivateAddresses = true)
            transport.open(
                uri = server.url("/start").toString(),
                method = "GET",
                headers = mapOf(
                    "Authorization" to "Bearer same-origin",
                    "Cookie" to "session=same-origin",
                    "X-Media-Token" to "same-origin-token",
                ),
            ).use { exchange ->
                assertEquals("media", exchange.bodyStream().bufferedReader().readText())
            }

            server.takeRequest()
            val redirected = server.takeRequest()
            assertEquals("Bearer same-origin", redirected.getHeader("Authorization"))
            assertEquals("session=same-origin", redirected.getHeader("Cookie"))
            assertEquals("same-origin-token", redirected.getHeader("X-Media-Token"))
        }
    }

    @Test
    fun crossOriginRedirectKeepsOnlyExplicitSafeHeaders() {
        MockWebServer().use { first ->
            MockWebServer().use { second ->
                first.start()
                second.start()
                first.enqueue(
                    MockResponse()
                        .setResponseCode(302)
                        .addHeader("Location", second.url("/media")),
                )
                second.enqueue(MockResponse().setBody("media"))

                val transport = VesperRelayHttpTransport(allowPrivateAddresses = true)
                transport.open(
                    uri = first.url("/start").toString(),
                    method = "GET",
                    headers = mapOf(
                        "Authorization" to "Bearer secret",
                        "Cookie" to "session=secret",
                        "X-Media-Token" to "secret-token",
                        "Accept" to "video/mp4",
                        "Accept-Language" to "en-US",
                        "User-Agent" to "Vesper-Test",
                        "Connection" to "X-Connection-Token",
                        "X-Connection-Token" to "remove-me",
                        "Host" to "attacker.invalid",
                    ),
                    rangeHeader = "bytes=10-19",
                ).use { exchange ->
                    assertEquals("media", exchange.bodyStream().bufferedReader().readText())
                }

                first.takeRequest()
                val redirected = second.takeRequest()
                assertNull(redirected.getHeader("Authorization"))
                assertNull(redirected.getHeader("Cookie"))
                assertNull(redirected.getHeader("X-Media-Token"))
                assertNull(redirected.getHeader("X-Connection-Token"))
                assertEquals("video/mp4", redirected.getHeader("Accept"))
                assertEquals("en-US", redirected.getHeader("Accept-Language"))
                assertEquals("Vesper-Test", redirected.getHeader("User-Agent"))
                assertEquals("bytes=10-19", redirected.getHeader("Range"))
                assertEquals(second.hostName + ":" + second.port, redirected.getHeader("Host"))
            }
        }
    }

    @Test
    fun validatedAddressIsPinnedAndResolverRunsOncePerHop() {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setBody("media"))
            server.start()
            val calls = AtomicInteger(0)
            val transport = VesperRelayHttpTransport(
                allowPrivateAddresses = true,
                resolver = VesperRelayHostResolver { host ->
                    assertEquals("media.example.test", host)
                    calls.incrementAndGet()
                    listOf(InetAddress.getByName("127.0.0.1"))
                },
            )

            transport.open(
                uri = "http://media.example.test:${server.port}/video",
                method = "GET",
            ).use { exchange ->
                assertEquals("media", exchange.bodyStream().bufferedReader().readText())
            }

            assertEquals(1, calls.get())
            assertEquals(
                "media.example.test:${server.port}",
                server.takeRequest().getHeader("Host"),
            )
        }
    }

    @Test
    fun blockedAddressFailsBeforeOpeningSocket() {
        MockWebServer().use { server ->
            server.start()
            val transport = VesperRelayHttpTransport(
                allowPrivateAddresses = false,
                resolver = VesperRelayHostResolver {
                    listOf(InetAddress.getByName("127.0.0.1"))
                },
            )

            assertThrows(java.io.IOException::class.java) {
                transport.open(
                    uri = "http://media.example.test:${server.port}/video",
                    method = "GET",
                )
            }
            assertEquals(0, server.requestCount)
        }
    }
}
