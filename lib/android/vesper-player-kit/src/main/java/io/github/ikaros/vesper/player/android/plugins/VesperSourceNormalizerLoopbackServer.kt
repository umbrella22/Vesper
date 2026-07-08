package io.github.ikaros.vesper.player.android

import android.util.Log
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.security.SecureRandom
import java.util.Base64
import java.util.Locale
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
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
    private val stateLock = Any()

    @Volatile
    private var serverSocket: ServerSocket? = null

    @Volatile
    private var acceptExecutor: ExecutorService? = null

    @Volatile
    private var requestExecutor: ExecutorService? = null

    @Volatile
    private var starting = false

    private var startEpoch = 0L

    fun register(registration: VesperNormalizedResourceRegistration): VesperNormalizedResourceHandle {
        ensureStarted()
        val token = nextToken()
        val path = when (registration.outputRoute) {
            "hlsShortWindow" -> "/normalized/$token/index.m3u8"
            else -> "/normalized/$token/primary"
        }
        val port =
            synchronized(stateLock) {
                pruneExpiredEntries()
                val socket = serverSocket
                    ?: throw IllegalStateException("normalized loopback server is not running")
                entries[token] = Entry(registration, nowMillisProvider().saturatingAdd(tokenTtlMillis))
                socket.localPort
            }
        return VesperNormalizedResourceHandle(
            token = token,
            playbackUri = "http://$LOOPBACK_HOST:$port$path",
        )
    }

    fun invalidate(token: String) {
        entries.remove(token)
    }

    internal fun entryCountForTest(): Int = entries.size

    fun stop() {
        val resources =
            synchronized(stateLock) {
                startEpoch += 1
                running.set(false)
                entries.clear()
                val socket = serverSocket
                val accept = acceptExecutor
                val request = requestExecutor
                serverSocket = null
                acceptExecutor = null
                requestExecutor = null
                Triple(socket, accept, request)
            }
        runCatching { resources.first?.close() }
        resources.second?.shutdownNow()
        resources.third?.shutdownNow()
    }

    private fun ensureStarted(): ServerSocket {
        var shouldStart = false
        var claimedEpoch = 0L
        while (!shouldStart) {
            synchronized(stateLock) {
                val socket = serverSocket
                if (running.get() && socket != null) {
                    return socket
                }
                if (!starting) {
                    starting = true
                    startEpoch += 1
                    claimedEpoch = startEpoch
                    shouldStart = true
                }
            }
            if (!shouldStart) {
                try {
                    Thread.sleep(1)
                } catch (error: InterruptedException) {
                    Thread.currentThread().interrupt()
                    throw IllegalStateException("interrupted while starting normalized loopback server", error)
                }
            }
        }
        return startAfterClaim(claimedEpoch)
    }

    private fun startAfterClaim(claimedEpoch: Long): ServerSocket {
        var socket: ServerSocket? = null
        var accept: ExecutorService? = null
        var request: ExecutorService? = null
        try {
            val startedSocket = ServerSocket(0, 50, InetAddress.getByName(LOOPBACK_HOST))
            socket = startedSocket
            val startedRequest =
                ThreadPoolExecutor(
                    DEFAULT_MAX_REQUEST_THREADS,
                    DEFAULT_MAX_REQUEST_THREADS,
                    0L,
                    TimeUnit.MILLISECONDS,
                    ArrayBlockingQueue(DEFAULT_MAX_QUEUED_REQUESTS),
                    { runnable ->
                        Thread(runnable, "vesper-source-normalizer-loopback-request").apply {
                            isDaemon = true
                        }
                    },
                    ThreadPoolExecutor.AbortPolicy(),
                )
            request = startedRequest
            val startedAccept = Executors.newSingleThreadExecutor { runnable ->
                Thread(runnable, "vesper-source-normalizer-loopback-accept").apply {
                    isDaemon = true
                }
            }
            accept = startedAccept
            var shouldPublish = false
            synchronized(stateLock) {
                shouldPublish = claimedEpoch == startEpoch
                if (shouldPublish) {
                    serverSocket = startedSocket
                    requestExecutor = startedRequest
                    acceptExecutor = startedAccept
                    running.set(true)
                }
                starting = false
            }
            if (!shouldPublish) {
                runCatching { startedSocket.close() }
                startedAccept.shutdownNow()
                startedRequest.shutdownNow()
                return ensureStarted()
            }
            startedAccept.execute { acceptLoop(startedSocket) }
            return startedSocket
        } catch (error: Exception) {
            synchronized(stateLock) {
                if (serverSocket === socket) {
                    serverSocket = null
                    requestExecutor = null
                    acceptExecutor = null
                    running.set(false)
                }
                starting = false
            }
            runCatching { socket?.close() }
            accept?.shutdownNow()
            request?.shutdownNow()
            throw error
        }
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
            try {
                client.soTimeout = DEFAULT_REQUEST_IDLE_TIMEOUT_MILLIS
            } catch (_: Exception) {
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
        val input = client.getInputStream()
        val output = client.getOutputStream()
        val requestLine =
            try {
                input.readBoundedHttpLine(
                    maxBytes = MAX_HTTP_REQUEST_LINE_BYTES,
                    statusCode = 414,
                    message = "URI Too Long",
                )
            } catch (error: LoopbackHttpLimitExceeded) {
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
                    input.readBoundedHttpLine(
                        maxBytes = MAX_HTTP_HEADER_LINE_BYTES,
                        statusCode = 431,
                        message = "Request Header Fields Too Large",
                    )
                } catch (error: LoopbackHttpLimitExceeded) {
                    output.writeSimpleResponse(error.statusCode, error.responseMessage)
                    return
                } ?: break
            if (line.isEmpty()) {
                break
            }
            headerCount += 1
            if (headerCount > MAX_HTTP_HEADERS) {
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
        pruneExpiredEntries()
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

private class LoopbackHttpLimitExceeded(
    val statusCode: Int,
    val responseMessage: String,
) : IOException(responseMessage)

private fun InputStream.readBoundedHttpLine(
    maxBytes: Int,
    statusCode: Int,
    message: String,
): String? {
    val buffer = ByteArrayOutputStream(min(maxBytes, 256))
    while (true) {
        val byte = read()
        if (byte < 0) {
            if (buffer.size() == 0) {
                return null
            }
            return buffer.toString(Charsets.ISO_8859_1.name()).removeSuffix("\r")
        }
        if (byte == '\n'.code) {
            return buffer.toString(Charsets.ISO_8859_1.name()).removeSuffix("\r")
        }
        if (buffer.size() >= maxBytes) {
            throw LoopbackHttpLimitExceeded(statusCode, message)
        }
        buffer.write(byte)
    }
}

private fun Int.reasonPhrase(): String =
    when (this) {
        200 -> "OK"
        206 -> "Partial Content"
        400 -> "Bad Request"
        404 -> "Not Found"
        405 -> "Method Not Allowed"
        414 -> "URI Too Long"
        416 -> "Range Not Satisfiable"
        431 -> "Request Header Fields Too Large"
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
private const val DEFAULT_MAX_QUEUED_REQUESTS = 64
private const val DEFAULT_REQUEST_IDLE_TIMEOUT_MILLIS = 5_000
private const val MAX_HTTP_REQUEST_LINE_BYTES = 8 * 1024
private const val MAX_HTTP_HEADER_LINE_BYTES = 8 * 1024
private const val MAX_HTTP_HEADERS = 64
private const val LOOPBACK_HOST = "127.0.0.1"
private const val TAG = "VesperSourceNormalizer"
