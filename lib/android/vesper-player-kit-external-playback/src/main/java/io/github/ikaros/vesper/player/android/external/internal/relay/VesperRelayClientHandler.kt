package io.github.ikaros.vesper.player.android.external.internal.relay

import java.io.OutputStream
import java.net.Socket
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean

internal class VesperRelayClientHandler(
    private val running: AtomicBoolean,
    private val activeClients: MutableSet<Socket> = ConcurrentHashMap.newKeySet(),
    private val maxActiveClients: Int,
    private val entryForToken: (String) -> RelayEntry?,
    private val relaySource: VesperRelaySourceRelay,
) {
    fun closeActiveClients() {
        activeClients.forEach { client -> runCatching { client.close() } }
        activeClients.clear()
    }

    fun acceptClient(
        client: Socket,
        requestExecutor: ExecutorService?,
    ) {
        if (!running.get()) {
            runCatching { client.close() }
            return
        }
        if (activeClients.size >= maxActiveClients.coerceAtLeast(1)) {
            runCatching { client.close() }
            return
        }
        val executor = requestExecutor
        if (executor == null || executor.isShutdown) {
            runCatching { client.close() }
            return
        }
        try {
            executor.execute { handleClientSafely(client) }
        } catch (_: RejectedExecutionException) {
            runCatching { client.close() }
        }
    }

    private fun handleClientSafely(socket: Socket) {
        activeClients.add(socket)
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
            activeClients.remove(socket)
        }
    }

    private fun handleClient(socket: Socket) {
        socket.use { client ->
            val input = client.getInputStream().bufferedReader(Charsets.ISO_8859_1)
            val output = client.getOutputStream()
            val requestLine = input.readLine() ?: return
            val parts = requestLine.split(' ')
            if (parts.size < 2) {
                output.writeSimpleResponse(400, "Bad Request")
                return
            }
            val method = parts[0].uppercase(Locale.US)
            val path = parts[1].substringBefore('?')
            val headers = linkedMapOf<String, String>()
            while (true) {
                val line = input.readLine() ?: break
                if (line.isEmpty()) {
                    break
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

internal class VesperRelaySourceRelay(
    private val appContext: android.content.Context?,
    private val formatAdapter: VesperRelayFormatAdapter,
    private val emitDiagnostic: (VesperRelayDiagnostic) -> Unit,
) {
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
                val responseHeaders = linkedMapOf(
                    "Content-Type" to adapted.contentType,
                    "Accept-Ranges" to "bytes",
                )
                adapted.contentLength?.let { responseHeaders["Content-Length"] = it.toString() }
                responseHeaders.putAll(adapted.headers)
                responseHeaders.addDlnaPlaybackHeaders()
                output.writeStatusAndHeaders(
                    adapted.status,
                    adapted.status.reasonPhrase(),
                    responseHeaders,
                )
                var clientCancelled = false
                if (!headOnly) {
                    try {
                        adapted.input.use { input -> input.copyTo(output) }
                    } catch (error: java.io.IOException) {
                        clientCancelled = true
                        emitDiagnostic(
                            VesperRelayDiagnostic(
                                code = "client_cancelled",
                                severity = "info",
                                message = error.message ?: "Relay client disconnected while receiving adapted media.",
                                details = mapOf("sessionId" to token),
                            ),
                        )
                    } finally {
                        if (clientCancelled) {
                            runCatching { adapted.closeable?.close() }
                        }
                    }
                } else {
                    runCatching { adapted.input.close() }
                }
                output.flush()
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
        val connection = (java.net.URL(source.uri).openConnection() as java.net.HttpURLConnection)
        connection.instanceFollowRedirects = true
        connection.connectTimeout = 10_000
        connection.readTimeout = 20_000
        connection.requestMethod = if (headOnly) "HEAD" else "GET"
        source.headers.forEach { (name, value) ->
            if (name.isNotBlank() && value.isNotBlank() && !name.isHopByHopHeader()) {
                connection.setRequestProperty(name, value)
            }
        }
        range?.toHeaderValue()?.let { connection.setRequestProperty("Range", it) }

        val status = connection.responseCode
        val responseHeaders = linkedMapOf<String, String>()
        connection.contentType?.let { responseHeaders["Content-Type"] = it }
        connection.getHeaderField("Content-Length")?.let { responseHeaders["Content-Length"] = it }
        connection.getHeaderField("Content-Range")?.let { responseHeaders["Content-Range"] = it }
        responseHeaders["Accept-Ranges"] = connection.getHeaderField("Accept-Ranges") ?: "bytes"
        responseHeaders.addDlnaPlaybackHeaders()
        output.writeStatusAndHeaders(status, connection.responseMessage ?: status.reasonPhrase(), responseHeaders)
        if (!headOnly) {
            val stream = runCatching { connection.inputStream }.getOrElse { connection.errorStream }
            stream?.use { it.copyTo(output) }
        }
        output.flush()
        connection.disconnect()
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
