package io.github.umbrella22.vesper.player.android.external.internal.relay

import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.IOException
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import javax.net.ssl.ExtendedSSLSession
import javax.net.ssl.SNIHostName
import javax.net.ssl.SNIMatcher
import javax.net.ssl.SNIServerName
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory
import javax.net.ssl.StandardConstants
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.tls.HandshakeCertificates
import okhttp3.tls.HeldCertificate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperRelayHttpTransportInstrumentationTest {
    @Test
    fun redirectsUsePinnedTlsOriginsAndStripCrossOriginCredentials() {
        val originHost = "origin.vesper.test"
        val redirectHost = "redirect.vesper.test"
        val certificate =
            HeldCertificate.Builder()
                .commonName(originHost)
                .addSubjectAlternativeName(originHost)
                .addSubjectAlternativeName(redirectHost)
                .build()
        val serverCertificates =
            HandshakeCertificates.Builder()
                .heldCertificate(certificate)
                .build()
        val clientCertificates =
            HandshakeCertificates.Builder()
                .addTrustedCertificate(certificate.certificate)
                .build()
        val originSni = AtomicReference<String>()
        val redirectSni = AtomicReference<String>()

        MockWebServer().use { originServer ->
            MockWebServer().use { redirectServer ->
                originServer.protocols = listOf(Protocol.HTTP_1_1)
                redirectServer.protocols = listOf(Protocol.HTTP_1_1)
                originServer.useHttps(
                    SniCheckingSslSocketFactory(
                        serverCertificates.sslSocketFactory(),
                        originHost,
                        originSni,
                    ),
                    false,
                )
                redirectServer.useHttps(
                    SniCheckingSslSocketFactory(
                        serverCertificates.sslSocketFactory(),
                        redirectHost,
                        redirectSni,
                    ),
                    false,
                )
                originServer.start()
                redirectServer.start()
                originServer.enqueue(
                    MockResponse()
                        .setResponseCode(302)
                        .addHeader("Location", "/same-origin"),
                )
                originServer.enqueue(
                    MockResponse()
                        .setResponseCode(302)
                        .addHeader(
                            "Location",
                            "https://$redirectHost:${redirectServer.port}/media",
                        ),
                )
                redirectServer.enqueue(MockResponse().setBody("media"))

                val resolverCalls = ConcurrentHashMap<String, Int>()
                val transport =
                    VesperRelayHttpTransport(
                        allowPrivateAddresses = true,
                        resolver =
                            VesperRelayHostResolver { host ->
                                resolverCalls.merge(host, 1, Int::plus)
                                listOf(InetAddress.getByName("127.0.0.1"))
                            },
                        baseClient =
                            OkHttpClient.Builder()
                                .sslSocketFactory(
                                    clientCertificates.sslSocketFactory(),
                                    clientCertificates.trustManager,
                                )
                                .build(),
                    )

                transport.open(
                    uri = "https://$originHost:${originServer.port}/start",
                    method = "GET",
                    headers =
                        mapOf(
                            "Authorization" to "Bearer device-secret",
                            "Cookie" to "session=device-secret",
                            "X-Media-Token" to "device-secret-token",
                            "Accept" to "video/mp4",
                            "Accept-Encoding" to "identity",
                            "Accept-Language" to "en-US",
                            "If-Range" to "\"device-etag\"",
                            "User-Agent" to "Vesper-Device-Test",
                            "Connection" to "X-Hop-Token",
                            "X-Hop-Token" to "remove-me",
                            "Host" to "attacker.invalid",
                        ),
                    rangeHeader = "bytes=10-19",
                ).use { exchange ->
                    assertEquals("media", exchange.bodyStream().bufferedReader().readText())
                }

                val initialRequest = checkNotNull(originServer.takeRequest(2, TimeUnit.SECONDS))
                val sameOriginRequest = checkNotNull(originServer.takeRequest(2, TimeUnit.SECONDS))
                val crossOriginRequest = checkNotNull(redirectServer.takeRequest(2, TimeUnit.SECONDS))
                assertEquals("Bearer device-secret", initialRequest.getHeader("Authorization"))
                assertEquals("Bearer device-secret", sameOriginRequest.getHeader("Authorization"))
                assertEquals("session=device-secret", sameOriginRequest.getHeader("Cookie"))
                assertEquals("device-secret-token", sameOriginRequest.getHeader("X-Media-Token"))
                assertNull(crossOriginRequest.getHeader("Authorization"))
                assertNull(crossOriginRequest.getHeader("Cookie"))
                assertNull(crossOriginRequest.getHeader("X-Media-Token"))
                assertNull(crossOriginRequest.getHeader("X-Hop-Token"))
                assertEquals("video/mp4", crossOriginRequest.getHeader("Accept"))
                assertEquals("identity", crossOriginRequest.getHeader("Accept-Encoding"))
                assertEquals("en-US", crossOriginRequest.getHeader("Accept-Language"))
                assertEquals("\"device-etag\"", crossOriginRequest.getHeader("If-Range"))
                assertEquals("Vesper-Device-Test", crossOriginRequest.getHeader("User-Agent"))
                assertEquals("bytes=10-19", crossOriginRequest.getHeader("Range"))
                assertEquals(
                    "$redirectHost:${redirectServer.port}",
                    crossOriginRequest.getHeader("Host"),
                )
                assertEquals(2, resolverCalls[originHost])
                assertEquals(1, resolverCalls[redirectHost])
                assertEquals(originHost, originSni.get())
                assertEquals(redirectHost, redirectSni.get())
            }
        }
    }

    @Test
    fun redirectToPrivateResolutionFailsBeforeOpeningTheSecondSocket() {
        val publicAddress = checkNotNull(findPublicLocalAddress()) {
            "Device must expose a globally routable address for the DNS rebinding test"
        }
        val originHost = "public-origin.vesper.test"
        val privateHost = "private-target.vesper.test"
        val certificate =
            HeldCertificate.Builder()
                .commonName(originHost)
                .addSubjectAlternativeName(originHost)
                .addSubjectAlternativeName(privateHost)
                .build()
        val serverCertificates =
            HandshakeCertificates.Builder()
                .heldCertificate(certificate)
                .build()
        val clientCertificates =
            HandshakeCertificates.Builder()
                .addTrustedCertificate(certificate.certificate)
                .build()

        MockWebServer().use { originServer ->
            MockWebServer().use { privateServer ->
                originServer.protocols = listOf(Protocol.HTTP_1_1)
                privateServer.protocols = listOf(Protocol.HTTP_1_1)
                originServer.useHttps(serverCertificates.sslSocketFactory(), false)
                privateServer.useHttps(serverCertificates.sslSocketFactory(), false)
                originServer.start(publicAddress, 0)
                privateServer.start(InetAddress.getLoopbackAddress(), 0)
                originServer.enqueue(
                    MockResponse()
                        .setResponseCode(302)
                        .addHeader(
                            "Location",
                            "https://$privateHost:${privateServer.port}/media",
                        ),
                )

                val resolverCalls = ConcurrentHashMap<String, Int>()
                val transport =
                    VesperRelayHttpTransport(
                        allowPrivateAddresses = false,
                        resolver =
                            VesperRelayHostResolver { host ->
                                resolverCalls.merge(host, 1, Int::plus)
                                when (host) {
                                    originHost -> listOf(publicAddress)
                                    privateHost -> listOf(InetAddress.getLoopbackAddress())
                                    else -> emptyList()
                                }
                            },
                        baseClient =
                            OkHttpClient.Builder()
                                .sslSocketFactory(
                                    clientCertificates.sslSocketFactory(),
                                    clientCertificates.trustManager,
                                )
                                .build(),
                    )

                val error =
                    assertThrows(IOException::class.java) {
                        transport.open(
                            uri = "https://$originHost:${originServer.port}/start",
                            method = "GET",
                        )
                    }

                assertEquals(
                    true,
                    error.message?.contains("private or local address"),
                )
                assertEquals(1, resolverCalls[originHost])
                assertEquals(1, resolverCalls[privateHost])
                assertEquals(1, originServer.requestCount)
                assertEquals(0, privateServer.requestCount)
            }
        }
    }

    @Test
    fun validatedAddressOverridesMaliciousConnectionDnsResolution() {
        val publicAddress = checkNotNull(findPublicLocalAddress()) {
            "Device must expose a globally routable address for the DNS rebinding test"
        }
        val host = "rebind.vesper.test"
        val certificate =
            HeldCertificate.Builder()
                .commonName(host)
                .addSubjectAlternativeName(host)
                .build()
        val serverCertificates =
            HandshakeCertificates.Builder()
                .heldCertificate(certificate)
                .build()
        val clientCertificates =
            HandshakeCertificates.Builder()
                .addTrustedCertificate(certificate.certificate)
                .build()

        MockWebServer().use { publicServer ->
            MockWebServer().use { privateServer ->
                publicServer.protocols = listOf(Protocol.HTTP_1_1)
                privateServer.protocols = listOf(Protocol.HTTP_1_1)
                publicServer.useHttps(serverCertificates.sslSocketFactory(), false)
                privateServer.useHttps(serverCertificates.sslSocketFactory(), false)
                publicServer.start(publicAddress, 0)
                privateServer.start(InetAddress.getLoopbackAddress(), publicServer.port)
                publicServer.enqueue(MockResponse().setBody("public-media"))
                privateServer.enqueue(MockResponse().setBody("private-media"))

                val validationResolverCalls = AtomicInteger()
                val connectionDnsCalls = AtomicInteger()
                val transport =
                    VesperRelayHttpTransport(
                        allowPrivateAddresses = false,
                        resolver =
                            VesperRelayHostResolver {
                                validationResolverCalls.incrementAndGet()
                                listOf(publicAddress)
                            },
                        baseClient =
                            OkHttpClient.Builder()
                                .dns(
                                    object : okhttp3.Dns {
                                        override fun lookup(hostname: String): List<InetAddress> {
                                            connectionDnsCalls.incrementAndGet()
                                            return listOf(InetAddress.getLoopbackAddress())
                                        }
                                    },
                                )
                                .sslSocketFactory(
                                    clientCertificates.sslSocketFactory(),
                                    clientCertificates.trustManager,
                                )
                                .build(),
                    )

                val responseBody =
                    transport.open(
                        uri = "https://$host:${publicServer.port}/media",
                        method = "GET",
                    ).use { exchange ->
                        exchange.bodyStream().bufferedReader().readText()
                    }

                assertEquals("public-media", responseBody)
                assertEquals(1, validationResolverCalls.get())
                assertEquals(0, connectionDnsCalls.get())
                assertEquals(1, publicServer.requestCount)
                assertEquals(0, privateServer.requestCount)
                assertEquals(
                    "$host:${publicServer.port}",
                    publicServer.takeRequest(2, TimeUnit.SECONDS)?.getHeader("Host"),
                )
            }
        }
    }
}

private fun findPublicLocalAddress(): InetAddress? =
    NetworkInterface.getNetworkInterfaces()
        .asSequence()
        .flatMap { networkInterface -> networkInterface.inetAddresses.asSequence() }
        .firstOrNull { address ->
            !address.isAnyLocalAddress &&
                !address.isLoopbackAddress &&
                !address.isLinkLocalAddress &&
                !address.isSiteLocalAddress &&
                !address.isMulticastAddress &&
                address.address.firstOrNull()?.toInt()?.and(0xfe) != 0xfc
        }

private class SniCheckingSslSocketFactory(
    private val delegate: SSLSocketFactory,
    private val expectedHost: String,
    private val observedHost: AtomicReference<String>,
) : SSLSocketFactory() {
    override fun getDefaultCipherSuites(): Array<String> = delegate.defaultCipherSuites

    override fun getSupportedCipherSuites(): Array<String> = delegate.supportedCipherSuites

    override fun createSocket(): Socket = configure(delegate.createSocket())

    override fun createSocket(
        socket: Socket,
        host: String,
        port: Int,
        autoClose: Boolean,
    ): Socket = configure(delegate.createSocket(socket, host, port, autoClose))

    override fun createSocket(host: String, port: Int): Socket =
        configure(delegate.createSocket(host, port))

    override fun createSocket(
        host: String,
        port: Int,
        localAddress: InetAddress,
        localPort: Int,
    ): Socket = configure(delegate.createSocket(host, port, localAddress, localPort))

    override fun createSocket(host: InetAddress, port: Int): Socket =
        configure(delegate.createSocket(host, port))

    override fun createSocket(
        address: InetAddress,
        port: Int,
        localAddress: InetAddress,
        localPort: Int,
    ): Socket = configure(delegate.createSocket(address, port, localAddress, localPort))

    private fun configure(socket: Socket): Socket {
        val sslSocket = socket as SSLSocket
        sslSocket.addHandshakeCompletedListener { event ->
            val requestedServerName =
                (event.session as? ExtendedSSLSession)
                    ?.requestedServerNames
                    ?.firstOrNull { it.type == StandardConstants.SNI_HOST_NAME }
            val hostName =
                requestedServerName
                    ?.let { runCatching { SNIHostName(it.encoded).asciiName }.getOrNull() }
            observedHost.set(hostName)
        }
        val parameters = sslSocket.sslParameters
        parameters.sniMatchers =
            listOf(
                object : SNIMatcher(StandardConstants.SNI_HOST_NAME) {
                    override fun matches(serverName: SNIServerName): Boolean {
                        val hostName =
                            runCatching { SNIHostName(serverName.encoded).asciiName }
                                .getOrNull()
                        observedHost.set(hostName)
                        return hostName.equals(expectedHost, ignoreCase = true)
                    }
                },
            )
        sslSocket.sslParameters = parameters
        return sslSocket
    }
}
