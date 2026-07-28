package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayHttpExchange
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayHttpTransport
import java.io.IOException
import java.io.OutputStream
import java.net.HttpURLConnection
import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean

internal class VesperRelayRemoteDashResourceClient(
    headers: Map<String, String>,
    private val allowPrivateAddresses: Boolean = false,
) {
    private val headers = headers.filterRemoteFetchHeaders()
    private val transport = VesperRelayHttpTransport(allowPrivateAddresses = allowPrivateAddresses)
    private val activeExchanges = Collections.synchronizedSet(mutableSetOf<VesperRelayHttpExchange>())

    fun readUtf8(uri: String): String {
        val exchange = transport.open(uri = uri, method = "GET", headers = headers)
        activeExchanges += exchange
        return try {
            val status = exchange.responseCode
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            val contentLength = exchange.contentLength
            if (contentLength > MAX_HOST_PREPARED_DASH_MANIFEST_BYTES) {
                throw DashResourceException(
                    code = "dash_manifest_too_large",
                    status = 413,
                    message = "DASH manifest exceeds the $MAX_HOST_PREPARED_DASH_MANIFEST_BYTES byte host-prepared planning limit.",
                )
            }
            exchange.bodyStream().use { input ->
                input.readUtf8Limited()
            }
        } finally {
            activeExchanges -= exchange
            exchange.close()
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
        val exchange = transport.open(uri = uri, method = "GET", headers = headers)
        activeExchanges += exchange
        try {
            val status = exchange.responseCode
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            val input = exchange.bodyStream()
            input.use { stream ->
                stream.copyToCancellable(output, cancellation)
            }
        } finally {
            activeExchanges -= exchange
            exchange.close()
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
        val exchange = transport.open(
            uri = uri,
            method = "GET",
            headers = headers,
            rangeHeader = range.toHeaderValue(),
        )
        activeExchanges += exchange
        try {
            val status = exchange.responseCode
            if (status == HttpURLConnection.HTTP_PARTIAL) {
                val contentRange = exchange.header("Content-Range")
                if (!contentRangeMatches(contentRange, range)) {
                    throw DashResourceException(
                        code = "host_fetch_failed",
                        status = 502,
                        message = "DASH HTTP resource returned invalid Content-Range for ${range.toHeaderValue()}.",
                    )
                }
                exchange.bodyStream().use { stream ->
                    stream.copyLimitedToCancellable(output, range.length, cancellation)
                }
                return
            }
            if (status == HttpURLConnection.HTTP_OK && range.start == 0L) {
                exchange.bodyStream().use { stream ->
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
            activeExchanges -= exchange
            exchange.close()
        }
    }

    fun cancel() {
        val exchanges = synchronized(activeExchanges) { activeExchanges.toList() }
        exchanges.forEach { exchange ->
            runCatching { exchange.cancel() }
            runCatching { exchange.close() }
        }
    }
}
