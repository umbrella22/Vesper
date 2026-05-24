package io.github.ikaros.vesper.player.android.external.internal.relay

import android.content.Context
import android.net.Uri
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.io.FileInputStream
import java.io.IOException
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.InetAddress
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

data class VesperRelayHandle(
    val token: String,
    val url: String,
)

class VesperRelayRegistrationException(
    val status: Int,
    val diagnostic: VesperRelayDiagnostic,
) : Exception(diagnostic.message)

class VesperRelayServer @JvmOverloads constructor(
    context: Context? = null,
    private val advertisedAddressProvider: () -> InetAddress? = ::findLanIpv4Address,
    private val bindAddressProvider: () -> InetAddress? = { context?.findWifiLanIpv4Address() },
    private val tokenTtlMillis: Long? = DEFAULT_TOKEN_TTL_MILLIS,
    private val nowMillisProvider: () -> Long = System::currentTimeMillis,
    private val formatAdapter: VesperRelayFormatAdapter = VesperUnavailableRelayFormatAdapter(),
    private val diagnosticListener: (VesperRelayDiagnostic) -> Unit = {},
    private val maxRequestThreads: Int = DEFAULT_MAX_REQUEST_THREADS,
    private val maxActiveClients: Int = DEFAULT_MAX_ACTIVE_CLIENTS,
) {
    private val appContext = context?.applicationContext
    private val entries = ConcurrentHashMap<String, RelayEntry>()
    private val activeClients = ConcurrentHashMap.newKeySet<Socket>()
    private val random = SecureRandom()
    private val running = AtomicBoolean(false)
    @Volatile
    private var serverSocket: ServerSocket? = null
    @Volatile
    private var acceptExecutor: ExecutorService? = null
    @Volatile
    private var requestExecutor: ExecutorService? = null
    @Volatile
    private var boundAddress: InetAddress? = null

    @Synchronized
    @JvmOverloads
    fun start(preferredBindAddress: InetAddress? = null) {
        if (running.get()) {
            val preferredAddress = preferredBindAddress?.takeIf { it.isBindableLanAddress() }
            val currentAddress = boundAddress
            if (preferredAddress == null ||
                currentAddress?.isAnyLocalAddress == true ||
                currentAddress?.hasSameHostAddress(preferredAddress) == true ||
                entries.isNotEmpty()
            ) {
                return
            }
            stop()
        }
        val bindAddress = preferredBindAddress?.takeIf { it.isBindableLanAddress() }
            ?: bindAddressProvider()
            ?: appContext?.findWifiLanIpv4Address()
            ?: throw IllegalStateException("No Wi-Fi LAN address is available for relay.")
        val socket = ServerSocket(0, 50, bindAddress)
        serverSocket = socket
        boundAddress = bindAddress
        requestExecutor = Executors.newFixedThreadPool(maxRequestThreads.coerceAtLeast(1)) { runnable ->
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
        entries.keys.forEach(formatAdapter::invalidate)
        entries.clear()
        runCatching { serverSocket?.close() }
        activeClients.forEach { client -> runCatching { client.close() } }
        activeClients.clear()
        serverSocket = null
        boundAddress = null
        acceptExecutor?.shutdownNow()
        requestExecutor?.shutdownNow()
        acceptExecutor = null
        requestExecutor = null
    }

    @JvmOverloads
    fun register(
        source: VesperPlayerSource,
        adaptation: VesperRelayFormatAdaptationRegistration? = null,
        preferredAddress: InetAddress? = null,
    ): VesperRelayHandle {
        pruneExpiredEntries()
        val token = nextToken()
        adaptation?.let { registration ->
            val validationRequest = source.toFormatAdaptationRequest(
                token = token,
                adaptation = registration,
                resourcePath = "",
                headOnly = false,
                range = null,
                requestHeaders = emptyMap(),
            )
            formatAdapter.validate(validationRequest)?.let { failure ->
                val diagnostic = failure.diagnostic.withHttpStatus(failure.status)
                emitDiagnostic(diagnostic)
                throw VesperRelayRegistrationException(failure.status, diagnostic)
            }
        }
        start(preferredAddress)
        val socket = serverSocket ?: throw IllegalStateException("Relay server is not running.")
        val host = advertisedHost(preferredAddress)
            ?: throw IllegalStateException("No LAN address is available for relay.")
        val relayPath = source.relayPath(token, adaptation)
        entries[token] = RelayEntry(
            source = source,
            adaptation = adaptation,
            expiresAtMillis = tokenExpiresAtMillis(),
        )
        try {
            adaptation?.let { registration ->
                val prewarmRequest = source.toFormatAdaptationRequest(
                    token = token,
                    adaptation = registration,
                    resourcePath = relayPath.substringAfterLast('/', missingDelimiterValue = ""),
                    headOnly = false,
                    range = null,
                    requestHeaders = emptyMap(),
                )
                formatAdapter.prewarm(prewarmRequest)?.let { failure ->
                    val diagnostic = failure.diagnostic.withHttpStatus(failure.status)
                    emitDiagnostic(diagnostic)
                    throw VesperRelayRegistrationException(failure.status, diagnostic)
                }
            }
        } catch (error: VesperRelayRegistrationException) {
            entries.remove(token)
            formatAdapter.invalidate(token)
            throw error
        } catch (error: RuntimeException) {
            entries.remove(token)
            formatAdapter.invalidate(token)
            throw error
        }
        return VesperRelayHandle(
            token = token,
            url = "http://$host:${socket.localPort}$relayPath",
        )
    }

    private fun advertisedHost(preferredAddress: InetAddress?): String? {
        val activeBind = boundAddress
        val preferred = preferredAddress?.takeIf { it.isAdvertisableLanAddress() }
        val address = when {
            preferred != null &&
                (activeBind == null ||
                    activeBind.isAnyLocalAddress ||
                    activeBind.hasSameHostAddress(preferred)) -> preferred
            activeBind != null && !activeBind.isAnyLocalAddress -> activeBind
            else -> appContext?.findWifiLanIpv4Address() ?: advertisedAddressProvider()
        }
        return address?.toRelayHost()
    }

    fun invalidate(token: String) {
        entries.remove(token)
        formatAdapter.invalidate(token)
    }

    fun invalidateAll() {
        entries.keys.forEach(formatAdapter::invalidate)
        entries.clear()
    }

    private fun entryForToken(token: String): RelayEntry? {
        val entry = entries[token] ?: return null
        val expiresAtMillis = entry.expiresAtMillis ?: return entry
        if (expiresAtMillis <= nowMillisProvider()) {
            entries.remove(token, entry)
            formatAdapter.invalidate(token)
            return null
        }
        return entry
    }

    private fun pruneExpiredEntries() {
        val now = nowMillisProvider()
        entries.forEach { (token, entry) ->
            val expired = entry.expiresAtMillis?.let { it <= now } ?: false
            if (expired && entries.remove(token, entry)) {
                formatAdapter.invalidate(token)
            }
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
            if (activeClients.size >= maxActiveClients.coerceAtLeast(1)) {
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
            if (entry.adaptation != null) {
                relayAdaptedSource(token, entry, resourcePath, method == "HEAD", range, headers, output)
            } else {
                relaySource(entry.source, method == "HEAD", range, output)
            }
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
                    } catch (error: IOException) {
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

    private fun emitDiagnostic(diagnostic: VesperRelayDiagnostic) {
        diagnosticListener(diagnostic)
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
        relayLocalReadable(
            source = source,
            headOnly = headOnly,
            range = range,
            output = output,
            readable = LocalRelayReadable(
                totalLength = file.length(),
                openInput = { FileInputStream(file) },
            ),
        )
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
            relayLocalReadable(
                source = source,
                headOnly = headOnly,
                range = range,
                output = output,
                readable = LocalRelayReadable(
                    totalLength = afd.length.takeIf { it >= 0 },
                    startOffset = afd.startOffset,
                    openInput = { FileInputStream(afd.fileDescriptor) },
                ),
            )
        }
    }

    private fun nextToken(): String {
        val bytes = ByteArray(24)
        random.nextBytes(bytes)
        return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
    }
}

private data class RelayEntry(
    val source: VesperPlayerSource,
    val adaptation: VesperRelayFormatAdaptationRegistration?,
    val expiresAtMillis: Long?,
)

private const val DEFAULT_TOKEN_TTL_MILLIS = 30 * 60 * 1000L
private const val DEFAULT_MAX_REQUEST_THREADS = 16
private const val DEFAULT_MAX_ACTIVE_CLIENTS = 32
