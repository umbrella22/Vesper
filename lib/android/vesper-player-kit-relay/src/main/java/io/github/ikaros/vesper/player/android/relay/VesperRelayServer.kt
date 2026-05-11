package io.github.ikaros.vesper.player.android.relay

import android.content.Context
import android.net.Uri
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.io.BufferedInputStream
import java.io.File
import java.io.FileInputStream
import java.io.InputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import java.security.SecureRandom
import java.util.Base64
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.min

data class VesperRelayHandle(
    val token: String,
    val url: String,
)

class VesperRelayServer @JvmOverloads constructor(
    context: Context? = null,
    private val advertisedAddressProvider: () -> InetAddress? = ::findLanIpv4Address,
    private val bindAddressProvider: () -> InetAddress = { InetAddress.getByName("0.0.0.0") },
    private val tokenTtlMillis: Long? = DEFAULT_TOKEN_TTL_MILLIS,
    private val nowMillisProvider: () -> Long = System::currentTimeMillis,
) {
    private val appContext = context?.applicationContext
    private val entries = ConcurrentHashMap<String, RelayEntry>()
    private val activeClients = ConcurrentHashMap.newKeySet<Socket>()
    private val random = SecureRandom()
    private val running = AtomicBoolean(false)
    private var serverSocket: ServerSocket? = null
    private var acceptExecutor: ExecutorService? = null
    private var requestExecutor: ExecutorService? = null
    private var advertisedAddress: InetAddress? = null

    @Synchronized
    fun start() {
        if (running.get()) {
            return
        }
        val bindAddress = bindAddressProvider()
        val socket = ServerSocket(0, 50, bindAddress)
        serverSocket = socket
        advertisedAddress = advertisedAddressProvider() ?: bindAddress.takeUnless { it.isAnyLocalAddress }
        requestExecutor = Executors.newCachedThreadPool { runnable ->
            Thread(runnable, "vesper-relay-request").apply { isDaemon = true }
        }
        acceptExecutor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "vesper-relay-accept").apply { isDaemon = true }
        }
        running.set(true)
        acceptExecutor?.execute { runAcceptLoop(socket) }
    }

    @Synchronized
    fun stop() {
        running.set(false)
        entries.clear()
        runCatching { serverSocket?.close() }
        activeClients.forEach { client -> runCatching { client.close() } }
        activeClients.clear()
        serverSocket = null
        acceptExecutor?.shutdownNow()
        requestExecutor?.shutdownNow()
        acceptExecutor = null
        requestExecutor = null
        advertisedAddress = null
    }

    fun register(source: VesperPlayerSource): VesperRelayHandle {
        pruneExpiredEntries()
        start()
        val socket = serverSocket ?: throw IllegalStateException("Relay server is not running.")
        val host = advertisedAddress?.hostAddress
            ?: throw IllegalStateException("No LAN address is available for relay.")
        val token = nextToken()
        entries[token] = RelayEntry(
            source = source,
            expiresAtMillis = tokenExpiresAtMillis(),
        )
        return VesperRelayHandle(
            token = token,
            url = "http://$host:${socket.localPort}/media/$token",
        )
    }

    fun invalidate(token: String) {
        entries.remove(token)
    }

    fun invalidateAll() {
        entries.clear()
    }

    private fun sourceForToken(token: String): VesperPlayerSource? {
        val entry = entries[token] ?: return null
        val expiresAtMillis = entry.expiresAtMillis ?: return entry.source
        if (expiresAtMillis <= nowMillisProvider()) {
            entries.remove(token, entry)
            return null
        }
        return entry.source
    }

    private fun pruneExpiredEntries() {
        val now = nowMillisProvider()
        entries.entries.removeIf { (_, entry) ->
            entry.expiresAtMillis?.let { it <= now } ?: false
        }
    }

    private fun tokenExpiresAtMillis(): Long? {
        val ttl = tokenTtlMillis?.takeIf { it > 0L } ?: return null
        return nowMillisProvider() + ttl
    }

    private fun runAcceptLoop(socket: ServerSocket) {
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
            if (!running.get()) {
                runCatching { client.close() }
                continue
            }
            val executor = requestExecutor
            if (executor == null || executor.isShutdown) {
                runCatching { client.close() }
                continue
            }
            try {
                executor.execute { handleClientSafely(client) }
            } catch (_: RejectedExecutionException) {
                runCatching { client.close() }
            }
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
            runCatching { socket.close() }
        } finally {
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
            val token = path.removePrefix("/media/").takeIf { path.startsWith("/media/") }
            val source = token?.let(::sourceForToken)
            if (source == null) {
                output.writeSimpleResponse(404, "Not Found")
                return
            }

            val range = headers["range"]?.let(::parseRangeHeader)
            relaySource(source, method == "HEAD", range, output)
        }
    }

    private fun relaySource(
        source: VesperPlayerSource,
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
        source: VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        val connection = (URL(source.uri).openConnection() as HttpURLConnection)
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
        output.writeStatusAndHeaders(status, connection.responseMessage ?: status.reasonPhrase(), responseHeaders)
        if (!headOnly) {
            val stream = runCatching { connection.inputStream }.getOrElse { connection.errorStream }
            stream?.use { it.copyTo(output) }
        }
        output.flush()
        connection.disconnect()
    }

    private fun relayFile(
        source: VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        val file = source.uri.toFile()
        if (!file.isFile) {
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        val total = file.length()
        val resolved = range?.resolve(total)
        if (range != null && resolved == null) {
            output.writeSimpleResponse(
                416,
                "Range Not Satisfiable",
                mapOf("Content-Range" to "bytes */$total", "Accept-Ranges" to "bytes"),
            )
            return
        }
        val start = resolved?.start ?: 0L
        val end = resolved?.end ?: total.saturatingMinusOne()
        val length = if (total == 0L) 0L else end - start + 1
        val status = if (resolved == null) 200 else 206
        val headers = linkedMapOf(
            "Content-Type" to source.contentTypeGuess(),
            "Accept-Ranges" to "bytes",
            "Content-Length" to length.toString(),
        )
        if (resolved != null) {
            headers["Content-Range"] = "bytes $start-$end/$total"
        }
        output.writeStatusAndHeaders(status, status.reasonPhrase(), headers)
        if (!headOnly && length > 0) {
            FileInputStream(file).use { input ->
                input.skipFully(start)
                input.copyLimitedTo(output, length)
            }
        }
        output.flush()
    }

    private fun relayContent(
        source: VesperPlayerSource,
        headOnly: Boolean,
        range: ByteRangeRequest?,
        output: OutputStream,
    ) {
        val context = appContext
        if (context == null) {
            output.writeSimpleResponse(501, "Not Implemented")
            return
        }
        val uri = Uri.parse(source.uri)
        val descriptor = context.contentResolver.openAssetFileDescriptor(uri, "r")
        if (descriptor == null) {
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        descriptor.use { afd ->
            val total = afd.length.takeIf { it >= 0 }
            val resolved = total?.let { range?.resolve(it) }
            if (range != null && total != null && resolved == null) {
                output.writeSimpleResponse(
                    416,
                    "Range Not Satisfiable",
                    mapOf("Content-Range" to "bytes */$total", "Accept-Ranges" to "bytes"),
                )
                return
            }
            val start = resolved?.start ?: 0L
            val end = resolved?.end ?: total?.saturatingMinusOne()
            val length = when {
                resolved != null && end != null -> end - start + 1
                total != null -> total
                else -> null
            }
            val status = if (resolved == null) 200 else 206
            val headers = linkedMapOf(
                "Content-Type" to source.contentTypeGuess(),
                "Accept-Ranges" to "bytes",
            )
            length?.let { headers["Content-Length"] = it.toString() }
            if (resolved != null) {
                headers["Content-Range"] = "bytes $start-$end/$total"
            }
            output.writeStatusAndHeaders(status, status.reasonPhrase(), headers)
            if (!headOnly) {
                FileInputStream(afd.fileDescriptor).use { input ->
                    input.skipFully(afd.startOffset + start)
                    if (length == null) {
                        input.copyTo(output)
                    } else {
                        input.copyLimitedTo(output, length)
                    }
                }
            }
        }
        output.flush()
    }

    private fun nextToken(): String {
        val bytes = ByteArray(24)
        random.nextBytes(bytes)
        return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
    }
}

private data class RelayEntry(
    val source: VesperPlayerSource,
    val expiresAtMillis: Long?,
)

private const val DEFAULT_TOKEN_TTL_MILLIS = 30 * 60 * 1000L

data class ByteRangeRequest(
    val start: Long?,
    val end: Long?,
) {
    fun resolve(totalLength: Long): ResolvedByteRange? {
        if (totalLength < 0) {
            return null
        }
        if (totalLength == 0L) {
            return null
        }
        val resolvedStart: Long
        val resolvedEnd: Long
        if (start == null) {
            val suffixLength = end ?: return null
            if (suffixLength <= 0) {
                return null
            }
            resolvedStart = (totalLength - suffixLength).coerceAtLeast(0)
            resolvedEnd = totalLength - 1
        } else {
            resolvedStart = start
            resolvedEnd = min(end ?: totalLength - 1, totalLength - 1)
        }
        if (resolvedStart < 0 || resolvedStart >= totalLength || resolvedEnd < resolvedStart) {
            return null
        }
        return ResolvedByteRange(resolvedStart, resolvedEnd)
    }

    fun toHeaderValue(): String =
        "bytes=${start?.toString() ?: ""}-${end?.toString() ?: ""}"
}

data class ResolvedByteRange(
    val start: Long,
    val end: Long,
)

fun parseRangeHeader(header: String): ByteRangeRequest? {
    if (!header.startsWith("bytes=", ignoreCase = true)) {
        return null
    }
    val range = header.substringAfter('=').substringBefore(',').trim()
    val separator = range.indexOf('-')
    if (separator < 0) {
        return null
    }
    val start = range.substring(0, separator).trim().takeIf { it.isNotEmpty() }?.toLongOrNull()
    val end = range.substring(separator + 1).trim().takeIf { it.isNotEmpty() }?.toLongOrNull()
    if (start == null && end == null) {
        return null
    }
    if (start != null && end != null && end < start) {
        return null
    }
    return ByteRangeRequest(start = start, end = end)
}

fun findLanIpv4Address(): InetAddress? =
    NetworkInterface.getNetworkInterfaces()
        .asSequence()
        .filter { it.isUp && !it.isLoopback }
        .flatMap { it.inetAddresses.asSequence() }
        .filterIsInstance<Inet4Address>()
        .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }

private fun OutputStream.writeSimpleResponse(
    status: Int,
    reason: String,
    headers: Map<String, String> = emptyMap(),
) {
    val body = reason.toByteArray(Charsets.UTF_8)
    val allHeaders = linkedMapOf(
        "Content-Type" to "text/plain; charset=utf-8",
        "Content-Length" to body.size.toString(),
    )
    allHeaders.putAll(headers)
    writeStatusAndHeaders(status, reason, allHeaders)
    write(body)
    flush()
}

private fun OutputStream.writeStatusAndHeaders(
    status: Int,
    reason: String,
    headers: Map<String, String>,
) {
    write("HTTP/1.1 $status $reason\r\n".toByteArray(Charsets.ISO_8859_1))
    headers.forEach { (name, value) ->
        write("$name: $value\r\n".toByteArray(Charsets.ISO_8859_1))
    }
    write("Connection: close\r\n\r\n".toByteArray(Charsets.ISO_8859_1))
}

private fun InputStream.copyLimitedTo(output: OutputStream, length: Long) {
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    var remaining = length
    val input = if (this is BufferedInputStream) this else BufferedInputStream(this)
    while (remaining > 0) {
        val read = input.read(buffer, 0, min(buffer.size.toLong(), remaining).toInt())
        if (read < 0) {
            break
        }
        output.write(buffer, 0, read)
        remaining -= read
    }
}

private fun InputStream.skipFully(bytes: Long) {
    var remaining = bytes
    while (remaining > 0) {
        val skipped = skip(remaining)
        if (skipped <= 0) {
            if (read() < 0) {
                break
            }
            remaining -= 1
        } else {
            remaining -= skipped
        }
    }
}

private fun String.isHopByHopHeader(): Boolean =
    lowercase(Locale.US) in setOf(
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        "range",
    )

private fun String.toFile(): File =
    if (startsWith("file://", ignoreCase = true)) {
        File(Uri.parse(this).path ?: "")
    } else {
        File(this)
    }

private fun VesperPlayerSource.contentTypeGuess(): String {
    val path = uri.substringBefore('?').substringBefore('#').lowercase(Locale.US)
    return when {
        path.endsWith(".m3u8") -> "application/vnd.apple.mpegurl"
        path.endsWith(".mpd") -> "application/dash+xml"
        path.endsWith(".mp4") -> "video/mp4"
        path.endsWith(".m4a") -> "audio/mp4"
        path.endsWith(".mp3") -> "audio/mpeg"
        else -> "application/octet-stream"
    }
}

private fun Long.saturatingMinusOne(): Long = if (this <= 0L) 0L else this - 1

private fun Int.reasonPhrase(): String =
    when (this) {
        200 -> "OK"
        206 -> "Partial Content"
        400 -> "Bad Request"
        404 -> "Not Found"
        405 -> "Method Not Allowed"
        416 -> "Range Not Satisfiable"
        501 -> "Not Implemented"
        else -> "OK"
    }
