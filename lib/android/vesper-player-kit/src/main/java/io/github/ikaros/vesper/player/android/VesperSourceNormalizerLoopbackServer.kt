package io.github.ikaros.vesper.player.android

import android.util.Log
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.security.SecureRandom
import java.util.Base64
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.min

internal data class VesperNormalizedResourceHandle(
    val token: String,
    val playbackUri: String,
)

internal data class VesperNormalizedResourceRegistration(
    val outputRoute: String,
    val primaryResourcePath: String,
    val primaryContentType: String?,
    val sessionReadBufferBytes: Long,
)

internal class VesperSourceNormalizerLoopbackServer(
    private val tokenTtlMillis: Long = DEFAULT_TOKEN_TTL_MILLIS,
    private val growingReadWaitMillis: Long = DEFAULT_GROWING_READ_WAIT_MILLIS,
    private val growingReadPollMillis: Long = DEFAULT_GROWING_READ_POLL_MILLIS,
    private val nowMillisProvider: () -> Long = System::currentTimeMillis,
) {
    private val random = SecureRandom()
    private val running = AtomicBoolean(false)
    private val entries = ConcurrentHashMap<String, Entry>()

    @Volatile
    private var serverSocket: ServerSocket? = null

    @Volatile
    private var acceptExecutor: ExecutorService? = null

    @Volatile
    private var requestExecutor: ExecutorService? = null

    @Synchronized
    fun register(registration: VesperNormalizedResourceRegistration): VesperNormalizedResourceHandle {
        pruneExpiredEntries()
        start()
        val socket = serverSocket ?: throw IllegalStateException("normalized loopback server is not running")
        val token = nextToken()
        entries[token] = Entry(registration, nowMillisProvider().saturatingAdd(tokenTtlMillis))
        val path = when (registration.outputRoute) {
            "hlsShortWindow" -> "/normalized/$token/index.m3u8"
            else -> "/normalized/$token/primary"
        }
        return VesperNormalizedResourceHandle(
            token = token,
            playbackUri = "http://127.0.0.1:${socket.localPort}$path",
        )
    }

    fun invalidate(token: String) {
        entries.remove(token)
    }

    @Synchronized
    fun stop() {
        running.set(false)
        entries.clear()
        runCatching { serverSocket?.close() }
        serverSocket = null
        acceptExecutor?.shutdownNow()
        requestExecutor?.shutdownNow()
        acceptExecutor = null
        requestExecutor = null
    }

    @Synchronized
    private fun start() {
        if (running.get()) {
            return
        }
        val socket = ServerSocket(0, 50, InetAddress.getLoopbackAddress())
        serverSocket = socket
        requestExecutor = Executors.newFixedThreadPool(DEFAULT_MAX_REQUEST_THREADS) { runnable ->
            Thread(runnable, "vesper-source-normalizer-loopback-request").apply {
                isDaemon = true
            }
        }
        acceptExecutor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "vesper-source-normalizer-loopback-accept").apply {
                isDaemon = true
            }
        }
        running.set(true)
        acceptExecutor?.execute { acceptLoop(socket) }
    }

    private fun acceptLoop(socket: ServerSocket) {
        while (running.get() && !Thread.currentThread().isInterrupted) {
            val client = try {
                socket.accept()
            } catch (error: Exception) {
                if (error is InterruptedException) {
                    Thread.currentThread().interrupt()
                }
                if (!running.get() || socket.isClosed) {
                    break
                }
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

    private fun handleClientSafely(client: Socket) {
        try {
            client.use(::handleClient)
        } catch (error: IOException) {
            Log.d(TAG, "normalized loopback client disconnected: ${error.message}")
        } catch (error: Exception) {
            Log.w(TAG, "normalized loopback request failed", error)
        }
    }

    private fun handleClient(client: Socket) {
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
        val route = NormalizedRoute.parse(path)
        if (route == null) {
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        val entry = entries[route.token]
        if (entry == null || entry.expiresAtMillis <= nowMillisProvider()) {
            entries.remove(route.token)
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        val file = entry.fileFor(route)
        if (file == null || !file.isFile) {
            output.writeSimpleResponse(404, "Not Found")
            return
        }
        val range = headers["range"]?.let(::parseByteRange)
        writeFileResponse(
            output = output,
            file = file,
            contentType = entry.contentTypeFor(route, file),
            headOnly = method == "HEAD",
            range = range,
            readBufferBytes = entry.registration.sessionReadBufferBytes,
            waitForGrowingBytes = entry.isGrowingPrimary(route),
            growingReadWaitMillis = growingReadWaitMillis,
            growingReadPollMillis = growingReadPollMillis,
        )
    }

    private fun pruneExpiredEntries() {
        val now = nowMillisProvider()
        entries.entries.removeIf { it.value.expiresAtMillis <= now }
    }

    private fun nextToken(): String {
        val bytes = ByteArray(24)
        random.nextBytes(bytes)
        return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
    }

    private data class Entry(
        val registration: VesperNormalizedResourceRegistration,
        val expiresAtMillis: Long,
    ) {
        private val primaryFile = File(registration.primaryResourcePath)
        private val rootDir = primaryFile.parentFile
        private val rootCanonicalPath = rootDir?.let { root ->
            runCatching { root.canonicalPath }.getOrNull()
        }

        fun fileFor(route: NormalizedRoute): File? =
            when (route.resourcePath) {
                "primary" -> primaryFile
                "index.m3u8" -> primaryFile
                else -> rootDir?.resolve(route.resourcePath)?.takeIf { candidate ->
                    val rootPath = rootCanonicalPath ?: return@takeIf false
                    runCatching {
                        candidate.canonicalPath.startsWith(rootPath + File.separator)
                    }.getOrDefault(false)
                }
            }

        fun contentTypeFor(route: NormalizedRoute, file: File): String =
            when {
                route.resourcePath.endsWith(".m3u8", ignoreCase = true) ->
                    "application/vnd.apple.mpegurl"
                route.resourcePath.endsWith(".ts", ignoreCase = true) -> "video/mp2t"
                route.resourcePath.endsWith(".mp4", ignoreCase = true) ||
                    route.resourcePath.endsWith(".m4s", ignoreCase = true) -> "video/mp4"
                route.resourcePath == "primary" -> registration.primaryContentType ?: "video/mp4"
                else -> URLConnectionMime.contentType(file)
            }

        fun isGrowingPrimary(route: NormalizedRoute): Boolean =
            registration.outputRoute == "fmp4LocalStream" && route.resourcePath == "primary"
    }

    private data class NormalizedRoute(
        val token: String,
        val resourcePath: String,
    ) {
        companion object {
            fun parse(path: String): NormalizedRoute? {
                val prefix = "/normalized/"
                if (!path.startsWith(prefix)) {
                    return null
                }
                val remainder = path.removePrefix(prefix)
                val separator = remainder.indexOf('/')
                if (separator <= 0 || separator == remainder.lastIndex) {
                    return null
                }
                val token = remainder.substring(0, separator)
                val resource = remainder.substring(separator + 1)
                if (token.isBlank() || resource.isBlank() || resource.contains("..")) {
                    return null
                }
                return NormalizedRoute(token, resource)
            }
        }
    }

    private object URLConnectionMime {
        fun contentType(file: File): String =
            java.net.URLConnection.guessContentTypeFromName(file.name) ?: "application/octet-stream"
    }
}

internal data class VesperByteRangeRequest(
    val start: Long?,
    val end: Long?,
) {
    fun resolve(totalLength: Long): VesperResolvedByteRange? {
        if (totalLength <= 0) {
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
        return VesperResolvedByteRange(resolvedStart, resolvedEnd)
    }
}

internal data class VesperResolvedByteRange(
    val start: Long,
    val end: Long,
) {
    val length: Long
        get() = end - start + 1
}

internal fun parseByteRange(header: String): VesperByteRangeRequest? {
    if (!header.startsWith("bytes=", ignoreCase = true)) {
        return null
    }
    val range = header.substringAfter('=').substringBefore(',').trim()
    val separator = range.indexOf('-')
    if (separator < 0) {
        return null
    }
    val start = range.substring(0, separator).trim().takeIf(String::isNotEmpty)?.toLongOrNull()
    val end = range.substring(separator + 1).trim().takeIf(String::isNotEmpty)?.toLongOrNull()
    if (start == null && end == null) {
        return null
    }
    if (start != null && end != null && end < start) {
        return null
    }
    return VesperByteRangeRequest(start, end)
}

private fun writeFileResponse(
    output: OutputStream,
    file: File,
    contentType: String,
    headOnly: Boolean,
    range: VesperByteRangeRequest?,
    readBufferBytes: Long,
    waitForGrowingBytes: Boolean,
    growingReadWaitMillis: Long,
    growingReadPollMillis: Long,
) {
    if (
        waitForGrowingBytes &&
        !headOnly &&
        (range == null || (range.start == 0L && range.end == null))
    ) {
        // Growing primary streams are close-delimited on purpose: ExoPlayer can
        // keep reading until the session closes, while Range and HEAD requests
        // still use fixed Content-Length responses below.
        output.writeStatusAndHeaders(
            200,
            200.reasonPhrase(),
            linkedMapOf(
                "Content-Type" to contentType,
                "Accept-Ranges" to "bytes",
            ),
        )
        FileInputStream(file).use { input ->
            input.copyGrowingTo(
                output = output,
                readBufferBytes = readBufferBytes,
                idleTimeoutMillis = growingReadWaitMillis,
                pollMillis = growingReadPollMillis,
            )
        }
        output.flush()
        return
    }

    var totalLength = file.length()
    if (waitForGrowingBytes && range?.start != null) {
        val targetLength = range.end?.plus(1) ?: range.start + 1
        totalLength = file.waitForLengthAtLeast(
            targetLength,
            timeoutMillis = growingReadWaitMillis,
            pollMillis = growingReadPollMillis,
        )
        if (range.end == null && totalLength > range.start) {
            totalLength = file.waitForStableLength(
                initialLength = totalLength,
                timeoutMillis = min(growingReadWaitMillis, DEFAULT_GROWING_READ_STABLE_WAIT_MILLIS),
                pollMillis = growingReadPollMillis,
            )
        }
    }
    val resolvedRange = range?.resolve(totalLength)
    if (range != null && resolvedRange == null) {
        output.writeSimpleResponse(
            416,
            "Range Not Satisfiable",
            mapOf("Content-Range" to "bytes */$totalLength", "Accept-Ranges" to "bytes"),
        )
        return
    }
    val status = if (resolvedRange == null) 200 else 206
    val length = resolvedRange?.length ?: totalLength
    val headers = linkedMapOf(
        "Content-Type" to contentType,
        "Accept-Ranges" to "bytes",
        "Content-Length" to length.toString(),
    )
    if (resolvedRange != null) {
        headers["Content-Range"] = "bytes ${resolvedRange.start}-${resolvedRange.end}/$totalLength"
    }
    output.writeStatusAndHeaders(status, status.reasonPhrase(), headers)
    if (!headOnly) {
        FileInputStream(file).use { input ->
            val start = resolvedRange?.start ?: 0L
            input.skipFully(start)
            input.copyLimitedTo(output, length, readBufferBytes)
        }
    }
    output.flush()
}

private fun File.waitForLengthAtLeast(
    targetLength: Long,
    timeoutMillis: Long,
    pollMillis: Long,
): Long {
    val deadline = System.nanoTime() + timeoutMillis.coerceAtLeast(0L) * 1_000_000L
    var currentLength = length()
    while (currentLength < targetLength && System.nanoTime() < deadline) {
        sleepForGrowingRead(pollMillis)
        currentLength = length()
    }
    return currentLength
}

private fun File.waitForStableLength(
    initialLength: Long,
    timeoutMillis: Long,
    pollMillis: Long,
): Long {
    val deadline = System.nanoTime() + timeoutMillis.coerceAtLeast(0L) * 1_000_000L
    var currentLength = initialLength
    var stablePollCount = 0
    while (System.nanoTime() < deadline) {
        sleepForGrowingRead(pollMillis)
        val nextLength = length()
        if (nextLength == currentLength) {
            stablePollCount += 1
            if (stablePollCount >= 2) {
                return nextLength
            }
        } else {
            currentLength = nextLength
            stablePollCount = 0
        }
    }
    return currentLength
}

private fun sleepForGrowingRead(pollMillis: Long) {
    try {
        Thread.sleep(pollMillis.coerceAtLeast(1L))
    } catch (_: InterruptedException) {
        Thread.currentThread().interrupt()
    }
}

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

private fun FileInputStream.copyLimitedTo(
    output: OutputStream,
    length: Long,
    readBufferBytes: Long,
) {
    val bufferSize = readBufferBytes.coerceIn(16 * 1024, 1024 * 1024).toInt()
    val buffer = ByteArray(bufferSize)
    var remaining = length
    while (remaining > 0) {
        val read = read(buffer, 0, min(buffer.size.toLong(), remaining).toInt())
        if (read < 0) {
            break
        }
        output.write(buffer, 0, read)
        remaining -= read
    }
}

private fun FileInputStream.copyGrowingTo(
    output: OutputStream,
    readBufferBytes: Long,
    idleTimeoutMillis: Long,
    pollMillis: Long,
) {
    val bufferSize = readBufferBytes.coerceIn(16 * 1024, 1024 * 1024).toInt()
    val buffer = ByteArray(bufferSize)
    var idleDeadline = System.nanoTime() + idleTimeoutMillis.coerceAtLeast(0L) * 1_000_000L
    while (!Thread.currentThread().isInterrupted) {
        val read = read(buffer)
        if (read > 0) {
            output.write(buffer, 0, read)
            output.flush()
            idleDeadline = System.nanoTime() + idleTimeoutMillis.coerceAtLeast(0L) * 1_000_000L
            continue
        }
        if (System.nanoTime() >= idleDeadline) {
            break
        }
        sleepForGrowingRead(pollMillis)
    }
}

private fun FileInputStream.skipFully(bytes: Long) {
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

private fun Int.reasonPhrase(): String =
    when (this) {
        200 -> "OK"
        206 -> "Partial Content"
        400 -> "Bad Request"
        404 -> "Not Found"
        405 -> "Method Not Allowed"
        416 -> "Range Not Satisfiable"
        else -> "OK"
    }

private fun Long.saturatingAdd(other: Long): Long {
    val result = this + other
    return if (result < this) Long.MAX_VALUE else result
}

private const val DEFAULT_TOKEN_TTL_MILLIS = 30 * 60 * 1000L
private const val DEFAULT_GROWING_READ_WAIT_MILLIS = 2_000L
private const val DEFAULT_GROWING_READ_POLL_MILLIS = 25L
private const val DEFAULT_GROWING_READ_STABLE_WAIT_MILLIS = 250L
private const val DEFAULT_MAX_REQUEST_THREADS = 8
private const val TAG = "VesperSourceNormalizer"
