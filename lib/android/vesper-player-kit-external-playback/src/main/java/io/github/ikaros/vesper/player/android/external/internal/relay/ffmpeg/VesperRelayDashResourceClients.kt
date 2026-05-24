package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.io.IOException
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean

internal class VesperRelayRemoteDashResourceClient(
    headers: Map<String, String>,
    private val allowPrivateAddresses: Boolean = false,
) {
    private val headers = headers.filterRemoteFetchHeaders()
    private val activeConnections = Collections.synchronizedSet(mutableSetOf<HttpURLConnection>())

    fun readUtf8(uri: String): String {
        val connection = openValidatedConnection(uri, headers)
        activeConnections += connection
        return try {
            val status = connection.responseCode
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            connection.inputStream.use { input ->
                input.readBytes().toString(Charsets.UTF_8)
            }
        } finally {
            activeConnections -= connection
            connection.disconnect()
        }
    }

    fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val connection = openValidatedConnection(uri, headers)
        activeConnections += connection
        try {
            val status = connection.responseCode
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            val input = connection.inputStream
            input.use { stream ->
                stream.copyToCancellable(output, cancellation)
            }
        } finally {
            activeConnections -= connection
            connection.disconnect()
        }
    }

    fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val connection = openValidatedConnection(uri, headers + ("Range" to range.toHeaderValue()))
        activeConnections += connection
        try {
            val status = connection.responseCode
            if (status == HttpURLConnection.HTTP_PARTIAL) {
                val contentRange = connection.getHeaderField("Content-Range")
                if (!contentRangeMatches(contentRange, range)) {
                    throw DashResourceException(
                        code = "host_fetch_failed",
                        status = 502,
                        message = "DASH HTTP resource returned invalid Content-Range for ${range.toHeaderValue()}.",
                    )
                }
                connection.inputStream.use { stream ->
                    stream.copyLimitedToCancellable(output, range.length, cancellation)
                }
                return
            }
            if (status == HttpURLConnection.HTTP_OK && range.start == 0L) {
                connection.inputStream.use { stream ->
                    stream.copyLimitedToCancellable(output, range.length, cancellation)
                }
                return
            }
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            throw DashResourceException(
                code = "host_fetch_failed",
                status = 502,
                message = "DASH HTTP resource did not honor byte range ${range.toHeaderValue()}: HTTP $status",
            )
        } finally {
            activeConnections -= connection
            connection.disconnect()
        }
    }

    fun cancel() {
        activeConnections.toList().forEach { connection ->
            runCatching { connection.disconnect() }
        }
    }

    private fun openValidatedConnection(
        uri: String,
        headers: Map<String, String>,
    ): HttpURLConnection {
        var current = uri
        repeat(MAX_REMOTE_DASH_REDIRECTS + 1) { redirectCount ->
            val connection = openConnection(current, headers)
            val status = connection.responseCode
            if (status !in HTTP_REDIRECT_STATUSES) {
                return connection
            }
            val location = connection.getHeaderField("Location")
            activeConnections -= connection
            connection.disconnect()
            if (location.isNullOrBlank()) {
                throw DashResourceException(
                    code = "host_fetch_failed",
                    status = 502,
                    message = "DASH HTTP resource redirect did not include a Location header.",
                )
            }
            if (redirectCount >= MAX_REMOTE_DASH_REDIRECTS) {
                throw DashResourceException(
                    code = "host_fetch_failed",
                    status = 502,
                    message = "DASH HTTP resource exceeded the redirect limit.",
                )
            }
            current = URI(current).resolve(location).toString()
        }
        throw DashResourceException(
            code = "host_fetch_failed",
            status = 502,
            message = "DASH HTTP resource exceeded the redirect limit.",
        )
    }

    private fun openConnection(
        uri: String,
        headers: Map<String, String>,
    ): HttpURLConnection {
        validateRemoteDashUri(uri, allowPrivateAddresses)
        val connection = URL(uri).openConnection() as HttpURLConnection
        connection.instanceFollowRedirects = false
        connection.connectTimeout = 10_000
        connection.readTimeout = 20_000
        connection.requestMethod = "GET"
        headers.forEach { (name, value) ->
            if (name.isNotBlank() && value.isNotBlank()) {
                connection.setRequestProperty(name, value)
            }
        }
        return connection
    }
}

private const val MAX_REMOTE_DASH_REDIRECTS = 5

private val HTTP_REDIRECT_STATUSES = setOf(
    HttpURLConnection.HTTP_MOVED_PERM,
    HttpURLConnection.HTTP_MOVED_TEMP,
    HttpURLConnection.HTTP_SEE_OTHER,
    307,
    308,
)
