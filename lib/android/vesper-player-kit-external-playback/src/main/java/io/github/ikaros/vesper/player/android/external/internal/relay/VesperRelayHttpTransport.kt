package io.github.ikaros.vesper.player.android.external.internal.relay

import java.io.ByteArrayInputStream
import java.io.Closeable
import java.io.IOException
import java.io.InputStream
import java.net.Inet6Address
import java.net.InetAddress
import java.net.Proxy
import java.net.UnknownHostException
import java.util.Locale
import java.util.concurrent.TimeUnit
import okhttp3.Call
import okhttp3.ConnectionPool
import okhttp3.Dns
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response

internal fun interface VesperRelayHostResolver {
    @Throws(UnknownHostException::class)
    fun resolve(host: String): List<InetAddress>
}

internal class VesperRelayHttpExchange(
    private val call: Call,
    private val response: Response,
) : Closeable {
    val responseCode: Int
        get() = response.code

    val responseMessage: String
        get() = response.message

    val contentLength: Long
        get() = response.body?.contentLength() ?: 0L

    fun header(name: String): String? = response.header(name)

    fun bodyStream(): InputStream =
        response.body?.byteStream() ?: ByteArrayInputStream(ByteArray(0))

    fun cancel() {
        call.cancel()
    }

    override fun close() {
        response.close()
    }
}

internal class VesperRelayHttpTransport(
    private val allowPrivateAddresses: Boolean,
    private val resolver: VesperRelayHostResolver = VesperRelayHostResolver { host ->
        InetAddress.getAllByName(host).toList()
    },
    private val baseClient: OkHttpClient = defaultRelayHttpClient(),
) {
    @Throws(IOException::class)
    fun open(
        uri: String,
        method: String,
        headers: Map<String, String> = emptyMap(),
        rangeHeader: String? = null,
    ): VesperRelayHttpExchange {
        val normalizedMethod = method.uppercase(Locale.US)
        if (normalizedMethod != "GET" && normalizedMethod != "HEAD") {
            throw IOException("Relay HTTP transport only supports GET and HEAD")
        }
        val initialUrl = parseRelayHttpUrl(uri)
        var currentUrl = initialUrl
        var redirectsRemaining = MAX_RELAY_HTTP_REDIRECTS

        while (true) {
            val addresses = resolveValidatedAddresses(currentUrl.host)
            val client = baseClient.newBuilder()
                .followRedirects(false)
                .followSslRedirects(false)
                .proxy(Proxy.NO_PROXY)
                .connectionPool(ConnectionPool(0, 1, TimeUnit.NANOSECONDS))
                .dns(PinnedRelayDns(currentUrl.host, addresses))
                .build()
            val requestBuilder = Request.Builder().url(currentUrl)
            val requestHeaders = headers.forRelayHop(initialUrl, currentUrl, rangeHeader)
            try {
                requestHeaders.forEach { (name, value) -> requestBuilder.header(name, value) }
            } catch (error: IllegalArgumentException) {
                throw IOException("Relay HTTP request contained an invalid header", error)
            }
            val request = requestBuilder.method(normalizedMethod, null).build()
            val call = client.newCall(request)
            val response = try {
                call.execute()
            } catch (error: IOException) {
                call.cancel()
                throw error
            }

            if (response.code !in RELAY_HTTP_REDIRECT_STATUSES) {
                return VesperRelayHttpExchange(call, response)
            }

            val location = response.header("Location")
            response.close()
            call.cancel()
            if (location.isNullOrBlank()) {
                throw IOException("Relay HTTP redirect did not include a Location header")
            }
            if (redirectsRemaining == 0) {
                throw IOException("Relay HTTP request exceeded the redirect limit")
            }
            currentUrl = currentUrl.resolve(location)
                ?.takeIf { it.scheme == "http" || it.scheme == "https" }
                ?: throw IOException("Relay HTTP redirect Location must resolve to http or https")
            redirectsRemaining -= 1
        }
    }

    private fun resolveValidatedAddresses(host: String): List<InetAddress> {
        val addresses = try {
            resolver.resolve(host)
        } catch (error: Exception) {
            throw IOException("Relay HTTP host `$host` could not be resolved", error)
        }.distinctBy { address -> address.address.toList() }

        if (addresses.isEmpty()) {
            throw IOException("Relay HTTP host `$host` resolved to no addresses")
        }
        if (!allowPrivateAddresses && addresses.any(InetAddress::isBlockedRelayAddress)) {
            throw IOException("Relay blocked: $host resolves to a private or local address")
        }
        return addresses
    }
}

private class PinnedRelayDns(
    private val expectedHost: String,
    addresses: List<InetAddress>,
) : Dns {
    private val addresses = addresses.toList()

    override fun lookup(hostname: String): List<InetAddress> {
        if (!hostname.equals(expectedHost, ignoreCase = true)) {
            throw UnknownHostException("Relay DNS lookup escaped pinned host `$expectedHost`")
        }
        return addresses
    }
}

private data class RelayOrigin(
    val scheme: String,
    val host: String,
    val port: Int,
)

private fun HttpUrl.relayOrigin(): RelayOrigin =
    RelayOrigin(
        scheme = scheme.lowercase(Locale.US),
        host = host.lowercase(Locale.US),
        port = port,
    )

private fun Map<String, String>.forRelayHop(
    initialUrl: HttpUrl,
    targetUrl: HttpUrl,
    rangeHeader: String?,
): Map<String, String> {
    val connectionNamedHeaders = entries
        .filter { (name, _) -> name.equals("Connection", ignoreCase = true) }
        .flatMap { (_, value) -> value.split(',') }
        .map { name -> name.trim().lowercase(Locale.US) }
        .filter(String::isNotEmpty)
        .toSet()
    val sameOrigin = initialUrl.relayOrigin() == targetUrl.relayOrigin()
    val filtered = linkedMapOf<String, String>()

    forEach { (name, value) ->
        val normalized = name.trim().lowercase(Locale.US)
        if (normalized.isEmpty() || value.isBlank()) {
            return@forEach
        }
        if (normalized in RELAY_NEVER_FORWARD_HEADERS || normalized in connectionNamedHeaders) {
            return@forEach
        }
        if (!sameOrigin && normalized !in RELAY_CROSS_ORIGIN_HEADERS) {
            return@forEach
        }
        filtered[name] = value
    }
    if (!rangeHeader.isNullOrBlank()) {
        filtered["Range"] = rangeHeader
    }
    return filtered
}

private fun parseRelayHttpUrl(uri: String): HttpUrl =
    uri.toHttpUrlOrNull()
        ?.takeIf { it.scheme == "http" || it.scheme == "https" }
        ?: throw IOException("Relay HTTP URI must be an absolute HTTP(S) URL")

private fun InetAddress.isBlockedRelayAddress(): Boolean =
    isAnyLocalAddress ||
        isLoopbackAddress ||
        isLinkLocalAddress ||
        isSiteLocalAddress ||
        isMulticastAddress ||
        (this is Inet6Address && isUniqueLocalRelayIpv6())

private fun Inet6Address.isUniqueLocalRelayIpv6(): Boolean {
    val first = address.firstOrNull()?.toInt()?.and(0xff) ?: return false
    return first and 0xfe == 0xfc
}

private fun defaultRelayHttpClient(): OkHttpClient =
    OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .followRedirects(false)
        .followSslRedirects(false)
        .proxy(Proxy.NO_PROXY)
        .build()

private const val MAX_RELAY_HTTP_REDIRECTS = 5

private val RELAY_HTTP_REDIRECT_STATUSES = setOf(301, 302, 303, 307, 308)

private val RELAY_NEVER_FORWARD_HEADERS = setOf(
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "range",
)

private val RELAY_CROSS_ORIGIN_HEADERS = setOf(
    "accept",
    "accept-encoding",
    "accept-language",
    "range",
    "if-range",
    "user-agent",
)
