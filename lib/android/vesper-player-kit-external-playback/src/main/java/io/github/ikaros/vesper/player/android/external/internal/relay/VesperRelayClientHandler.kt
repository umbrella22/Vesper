package io.github.ikaros.vesper.player.android.external.internal.relay

import io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg.VesperRelayIOException
import java.io.OutputStream
import java.net.Socket
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

internal class VesperRelayClientHandler(
    private val running: AtomicBoolean,
    private val activeClients: MutableSet<Socket> = ConcurrentHashMap.newKeySet(),
    private val maxActiveClients: Int,
    private val entryForToken: (String) -> RelayEntry?,
    private val relaySource: VesperRelaySourceRelay,
) {
    private val activeClientCount = AtomicInteger(0)

    fun closeActiveClients() {
        activeClients.forEach { client -> runCatching { client.close() } }
        activeClients.clear()
        activeClientCount.set(0)
    }

    fun acceptClient(
        client: Socket,
        requestExecutor: ExecutorService?,
    ) {
        if (!running.get()) {
            runCatching { client.close() }
            return
        }
        if (activeClientCount.incrementAndGet() > maxActiveClients.coerceAtLeast(1)) {
            activeClientCount.decrementAndGet()
            runCatching { client.close() }
            return
        }
        activeClients.add(client)
        val executor = requestExecutor
        if (executor == null || executor.isShutdown) {
            releaseClient(client)
            runCatching { client.close() }
            return
        }
        try {
            client.soTimeout = DEFAULT_RELAY_REQUEST_IDLE_TIMEOUT_MILLIS
        } catch (_: Exception) {
            releaseClient(client)
            runCatching { client.close() }
            return
        }
        try {
            executor.execute { handleClientSafely(client) }
        } catch (_: RejectedExecutionException) {
            releaseClient(client)
            runCatching { client.close() }
        }
    }

    private fun handleClientSafely(socket: Socket) {
        try {
            if (running.get()) {
                handleClient(socket)
            } else {
                runCatching { socket.close() }
            }
        } catch (error: Exception) {
            if (error is InterruptedException) {
                Thread.currentThread().interrupt()
            }
        } finally {
            runCatching { socket.close() }
            releaseClient(socket)
        }
    }

    private fun releaseClient(socket: Socket) {
        if (activeClients.remove(socket)) {
            activeClientCount.updateAndGet { count -> (count - 1).coerceAtLeast(0) }
        }
    }

    private fun handleClient(socket: Socket) {
        socket.use { client ->
            val input = client.getInputStream()
            val output = client.getOutputStream()
            val requestLine =
                try {
                    input.readBoundedRelayHttpLine(
                        maxBytes = MAX_RELAY_HTTP_REQUEST_LINE_BYTES,
                        statusCode = 414,
                        message = "URI Too Long",
                    )
                } catch (error: VesperRelayHttpLimitExceeded) {
                    output.writeSimpleResponse(error.statusCode, error.responseMessage)
                    return
                } ?: return
            val parts = requestLine.split(' ')
            if (parts.size < 2) {
                output.writeSimpleResponse(400, "Bad Request")
                return
            }
            val method = parts[0].uppercase(Locale.US)
            val path = parts[1].substringBefore('?')
            val headers = linkedMapOf<String, String>()
            var headerCount = 0
            while (true) {
                val line =
                    try {
                        input.readBoundedRelayHttpLine(
                            maxBytes = MAX_RELAY_HTTP_HEADER_LINE_BYTES,
                            statusCode = 431,
                            message = "Request Header Fields Too Large",
                        )
                    } catch (error: VesperRelayHttpLimitExceeded) {
                        output.writeSimpleResponse(error.statusCode, error.responseMessage)
                        return
                    } ?: break
                if (line.isEmpty()) {
                    break
                }
                headerCount += 1
                if (headerCount > MAX_RELAY_HTTP_HEADERS) {
                    output.writeSimpleResponse(431, "Request Header Fields Too Large")
                    return
                }
                val separator = line.indexOf(':')
                if (separator > 0) {
                    headers[line.substring(0, separator).trim().lowercase(Locale.US)] =
                        line.substring(separator + 1).trim()
                }
            }

            if (method != "GET" && method != "HEAD") {
                output.writeSimpleResponse(405, "Method Not Allowed", mapOf("Allow" to "GET, HEAD"))
                return
            }
            val token = path
                .removePrefix("/media/")
                .substringBefore('/')
                .takeIf { path.startsWith("/media/") && it.isNotBlank() }
            if (token == null) {
                output.writeSimpleResponse(404, "Not Found")
                return
            }
            val entry = entryForToken(token)
            if (entry == null) {
                output.writeSimpleResponse(404, "Not Found")
                return
            }
            val resourcePath = path
                .removePrefix("/media/$token")
                .removePrefix("/")

            val range = headers["range"]?.let(::parseRangeHeader)
            relaySource.relay(
                token = token,
                entry = entry,
                resourcePath = resourcePath,
                headOnly = method == "HEAD",
                range = range,
                headers = headers,
                output = output,
            )
        }
    }
}

internal fun runRelayAcceptLoop(
    running: AtomicBoolean,
    socket: java.net.ServerSocket,
    requestExecutorProvider: () -> ExecutorService?,
    clientHandler: VesperRelayClientHandler,
) {
    while (running.get() && !Thread.currentThread().isInterrupted) {
        val client = try {
            socket.accept()
        } catch (error: Exception) {
            if (error is InterruptedException) {
                Thread.currentThread().interrupt()
                break
            }
            if (!running.get() || socket.isClosed) {
                break
            }
            continue
        }
        clientHandler.acceptClient(client, requestExecutorProvider())
    }
}

private const val DEFAULT_RELAY_REQUEST_IDLE_TIMEOUT_MILLIS = 5_000

internal class VesperRelaySourceRelay(
    private val appContext: android.content.Context?,
    private val formatAdapter: VesperRelayFormatAdapter,
    private val emitDiagnostic: (VesperRelayDiagnostic) -> Unit,
    private val allowPrivateRemoteSources: Boolean,
) {
    private val remoteTransport = VesperRelayHttpTransport(
        allowPrivateAddresses = allowPrivateRemoteSources,
    )

    fun relay(
        token: String,
        entry: RelayEntry,
        resourcePath: String,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        headers: Map<String, String>,
        output: OutputStream,
    ) {
        if (entry.adaptation != null) {
            relayAdaptedSource(token, entry, resourcePath, headOnly, range, headers, output)
        } else {
            relaySource(entry.source, headOnly, range, output)
        }
    }

    private fun relayAdaptedSource(
        token: String,
        entry: RelayEntry,
        resourcePath: String,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        headers: Map<String, String>,
        output: OutputStream,
    ) {
        val adaptation = entry.adaptation ?: return relaySource(entry.source, headOnly, range, output)
        val request = entry.source.toFormatAdaptationRequest(
            token = token,
            adaptation = adaptation,
            resourcePath = resourcePath,
            headOnly = headOnly,
            range = range,
            requestHeaders = headers,
        )
        when (val result = formatAdapter.open(request)) {
            is VesperRelayFormatAdaptationResult.Failure -> {
                val diagnostic = result.diagnostic.withHttpStatus(result.status)
                emitDiagnostic(diagnostic)
                output.writeDiagnosticResponse(result.status, diagnostic)
            }
            is VesperRelayFormatAdaptationResult.Stream -> {
                val adapted = result.stream
                try {
                    val responseHeaders = linkedMapOf(
                        "Content-Type" to adapted.contentType,
                        "Accept-Ranges" to "bytes",
                    )
                    adapted.contentLength?.let { responseHeaders["Content-Length"] = it.toString() }
                    responseHeaders.putAll(adapted.headers)
                    val usesChunkedFraming = !headOnly && adapted.contentLength == null
                    if (usesChunkedFraming) {
                        responseHeaders.keys.removeAll { name -> name.equals("Content-Length", ignoreCase = true) }
                        responseHeaders["Transfer-Encoding"] = "chunked"
                    } else {
                        responseHeaders.keys.removeAll { name -> name.equals("Transfer-Encoding", ignoreCase = true) }
                    }
                    responseHeaders.addDlnaPlaybackHeaders()
                    output.writeStatusAndHeaders(
                        adapted.status,
                        adapted.status.reasonPhrase(),
                        responseHeaders,
                    )
                    if (!headOnly) {
                        if (usesChunkedFraming) {
                            val chunked = VesperRelayChunkedOutputStream(output)
                            adapted.input.copyTo(chunked)
                            chunked.finish()
                        } else {
                            adapted.input.copyTo(output)
                        }
                    }
                    output.flush()
                } catch (error: java.io.IOException) {
                    val relayError = error as? VesperRelayIOException
                    emitDiagnostic(
                        VesperRelayDiagnostic(
                            code = relayError?.code ?: "client_cancelled",
                            severity = if (relayError == null) "info" else "error",
                            message = error.message ?: "Relay client disconnected while receiving adapted media.",
                            details = mapOf("sessionId" to token),
                        ),
                    )
                } finally {
                    runCatching { adapted.input.close() }
                    runCatching { adapted.closeable?.close() }
                }
            }
        }
    }

    private fun relaySource(
        source: io.github.ikaros.vesper.player.android.VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        when {
            source.uri.startsWith("http://", ignoreCase = true) ||
                source.uri.startsWith("https://", ignoreCase = true) ->
                relayRemote(source, headOnly, range, output)
            source.uri.startsWith("content://", ignoreCase = true) ->
                relayContent(source, headOnly, range, output)
            else ->
                relayFile(source, headOnly, range, output)
        }
    }

    private fun relayRemote(
        source: io.github.ikaros.vesper.player.android.VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        remoteTransport.open(
            uri = source.uri,
            method = if (headOnly) "HEAD" else "GET",
            headers = source.headers,
            rangeHeader = range?.toHeaderValue(),
        ).use { exchange ->
            val status = exchange.responseCode
            val responseHeaders = linkedMapOf<String, String>()
            exchange.header("Content-Type")?.let { responseHeaders["Content-Type"] = it }
            exchange.header("Content-Length")?.let { responseHeaders["Content-Length"] = it }
            exchange.header("Content-Range")?.let { responseHeaders["Content-Range"] = it }
            responseHeaders["Accept-Ranges"] = exchange.header("Accept-Ranges") ?: "bytes"
            responseHeaders.addDlnaPlaybackHeaders()
            output.writeStatusAndHeaders(status, exchange.responseMessage.ifBlank { status.reasonPhrase() }, responseHeaders)
            if (!headOnly) {
                exchange.bodyStream().use { input -> input.copyTo(output) }
            }
            output.flush()
        }
    }

    private fun relayFile(
        source: io.github.ikaros.vesper.player.android.VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        val file = source.uri.toFile()
        if (!file.isFile) {
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        relayLocalReadable(
            source = source,
            headOnly = headOnly,
            range = range,
            output = output,
            readable = LocalRelayReadable(
                totalLength = file.length(),
                openInput = { java.io.FileInputStream(file) },
            ),
        )
    }

    private fun relayContent(
        source: io.github.ikaros.vesper.player.android.VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        val context = appContext
        if (context == null) {
            output.writeSimpleResponse(501, "Not Implemented")
            return
        }
        val uri = android.net.Uri.parse(source.uri)
        val descriptor = context.contentResolver.openAssetFileDescriptor(uri, "r")
        if (descriptor == null) {
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        descriptor.use { afd ->
            relayLocalReadable(
                source = source,
                headOnly = headOnly,
                range = range,
                output = output,
                readable = LocalRelayReadable(
                    totalLength = afd.length.takeIf { it >= 0 },
                    startOffset = afd.startOffset,
                    openInput = { java.io.FileInputStream(afd.fileDescriptor) },
                ),
            )
        }
    }
}
