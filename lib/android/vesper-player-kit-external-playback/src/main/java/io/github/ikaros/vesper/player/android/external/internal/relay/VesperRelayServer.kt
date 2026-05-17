package io.github.ikaros.vesper.player.android.external.internal.relay

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.io.BufferedInputStream
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.URI
import java.net.URL
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
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
) {
    private val appContext = context?.applicationContext
    private val entries = ConcurrentHashMap<String, RelayEntry>()
    private val activeClients = ConcurrentHashMap.newKeySet<Socket>()
    private val random = SecureRandom()
    private val running = AtomicBoolean(false)
    private var serverSocket: ServerSocket? = null
    private var acceptExecutor: ExecutorService? = null
    private var requestExecutor: ExecutorService? = null
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

internal data class LocalRelayReadable(
    val totalLength: Long?,
    val startOffset: Long = 0L,
    val openInput: () -> InputStream,
)

internal fun relayLocalReadable(
    source: VesperPlayerSource,
    headOnly: Boolean,
    range: ByteRangeRequest?,
    output: OutputStream,
    readable: LocalRelayReadable,
) {
    val total = readable.totalLength
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
    headers.addDlnaPlaybackHeaders()
    length?.let { headers["Content-Length"] = it.toString() }
    if (resolved != null) {
        headers["Content-Range"] = "bytes $start-$end/$total"
    }
    output.writeStatusAndHeaders(status, status.reasonPhrase(), headers)
    if (!headOnly && length != 0L) {
        readable.openInput().use { input ->
            input.skipFully(readable.startOffset + start)
            if (length == null) {
                input.copyTo(output)
            } else {
                input.copyLimitedTo(output, length)
            }
        }
    }
    output.flush()
}

private fun VesperPlayerSource.toFormatAdaptationRequest(
    token: String,
    adaptation: VesperRelayFormatAdaptationRegistration,
    resourcePath: String,
    headOnly: Boolean,
    range: ByteRangeRequest?,
    requestHeaders: Map<String, String>,
): VesperRelayFormatAdaptationRequest =
    VesperRelayFormatAdaptationRequest(
        sessionId = token,
        source = this,
        fallbackFormat = adaptation.fallbackFormat,
        resourcePath = resourcePath,
        range = range,
        requestHeaders = requestHeaders,
        enableRangeCache = adaptation.config.enableRangeCache,
        dashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(
            allowRemoteReferences = adaptation.config.allowRemoteDashMediaReferences,
            allowPrivateAddresses = adaptation.config.allowPrivateRemoteDashMediaAddresses,
            allowedRequestHeaders = adaptation.config.remoteDashMediaRequestHeaders,
        ),
        debugDiagnostics = adaptation.config.debugDiagnostics,
        headOnly = headOnly,
        routeId = adaptation.routeId,
        routeName = adaptation.routeName,
    )

private fun VesperRelayDiagnostic.withHttpStatus(status: Int): VesperRelayDiagnostic =
    copy(details = details + ("httpStatus" to status.toString()))

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
        .filter { it.isUsableLanInterface() }
        .flatMap { it.inetAddresses.asSequence() }
        .filterIsInstance<Inet4Address>()
        .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }

@Suppress("DEPRECATION")
private fun Context.findWifiLanIpv4Address(): InetAddress? {
    val connectivityManager =
        getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            ?: return null
    return runCatching {
        connectivityManager.allNetworks
            .asSequence()
            .mapNotNull { network ->
                val capabilities = connectivityManager.getNetworkCapabilities(network)
                    ?: return@mapNotNull null
                if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) ||
                    (!capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
                        !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET))
                ) {
                    return@mapNotNull null
                }
                val linkProperties = connectivityManager.getLinkProperties(network)
                    ?: return@mapNotNull null
                val interfaceName = linkProperties.interfaceName
                val networkInterface = interfaceName
                    ?.let { runCatching { NetworkInterface.getByName(it) }.getOrNull() }
                if (!networkInterface.isUsableLanInterface(interfaceName)) {
                    return@mapNotNull null
                }
                linkProperties.linkAddresses
                    .asSequence()
                    .map { it.address }
                    .filterIsInstance<Inet4Address>()
                    .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                    ?: networkInterface
                        ?.inetAddresses
                        ?.asSequence()
                        ?.filterIsInstance<Inet4Address>()
                        ?.firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
            }
            .firstOrNull()
    }.getOrNull()
}

private fun InetAddress.isBindableLanAddress(): Boolean =
    !isLinkLocalAddress

private fun InetAddress.isAdvertisableLanAddress(): Boolean =
    isBindableLanAddress() && !isAnyLocalAddress

private fun InetAddress.hasSameHostAddress(other: InetAddress): Boolean =
    hostAddress == other.hostAddress

private fun NetworkInterface.isUsableLanInterface(): Boolean =
    isUp && !isLoopback && !isPointToPoint && !isLikelyTunnelInterface()

private fun NetworkInterface?.isUsableLanInterface(interfaceName: String?): Boolean {
    if (interfaceName?.isLikelyTunnelInterfaceName() == true) {
        return false
    }
    val networkInterface = this ?: return true
    return runCatching { networkInterface.isUsableLanInterface() }.getOrDefault(false)
}

private fun NetworkInterface.isLikelyTunnelInterface(): Boolean {
    return name.isLikelyTunnelInterfaceName()
}

private fun String.isLikelyTunnelInterfaceName(): Boolean {
    val normalizedName = lowercase(Locale.US)
    return normalizedName.startsWith("tun") ||
        normalizedName.startsWith("tap") ||
        normalizedName.startsWith("ppp") ||
        normalizedName.startsWith("wg")
}

private fun InetAddress.toRelayHost(): String =
    if (address.size == IPV6_ADDRESS_BYTES) {
        "[$hostAddress]"
    } else {
        hostAddress
    }

private const val IPV6_ADDRESS_BYTES = 16

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

private fun OutputStream.writeDiagnosticResponse(
    status: Int,
    diagnostic: VesperRelayDiagnostic,
) {
    val body = buildString {
        append("code=").append(diagnostic.code).append('\n')
        append("message=").append(diagnostic.message).append('\n')
        append("severity=").append(diagnostic.severity).append('\n')
        diagnostic.details.forEach { (key, value) ->
            append("detail.").append(key).append('=').append(value).append('\n')
        }
    }.toByteArray(Charsets.UTF_8)
    val headers = linkedMapOf(
        "Content-Type" to "text/plain; charset=utf-8",
        "Content-Length" to body.size.toString(),
        "X-Vesper-Relay-Error-Code" to diagnostic.code,
        "X-Vesper-Relay-Error-Severity" to diagnostic.severity,
    )
    writeStatusAndHeaders(status, status.reasonPhrase(), headers)
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

private fun VesperPlayerSource.relayPath(
    token: String,
    adaptation: VesperRelayFormatAdaptationRegistration?,
): String {
    if (adaptation != null) {
        val rawBaseName = listOfNotNull(label, uri.fileNameFromUri())
            .firstOrNull { it.isNotBlank() }
            ?: "media"
        val baseName = rawBaseName
            .substringBeforeLast('.', missingDelimiterValue = rawBaseName)
            .takeIf { it.isNotBlank() }
            ?: "media"
        return "/media/$token/${baseName.urlPathSegmentEncoded()}.${adaptation.fallbackFormat.urlExtension()}"
    }
    val fileName = listOfNotNull(uri.fileNameFromUri(), label)
        .firstOrNull { it.contentTypeFromPath() != null }
        ?.urlPathSegmentEncoded()
    return if (fileName == null) {
        "/media/$token"
    } else {
        "/media/$token/$fileName"
    }
}

private fun String.fileNameFromUri(): String? {
    val javaUriPath = runCatching { URI(this).path }.getOrNull()
    val androidUriPath = runCatching { Uri.parse(this).lastPathSegment }.getOrNull()
    return (javaUriPath ?: androidUriPath)
        ?.substringAfterLast('/')
        ?.takeIf { it.isNotBlank() }
}

private fun String.urlPathSegmentEncoded(): String =
    URLEncoder.encode(this, StandardCharsets.UTF_8.name())
        .replace("+", "%20")

private fun String.toFile(): File =
    if (startsWith("file://", ignoreCase = true)) {
        File(Uri.parse(this).path ?: "")
    } else {
        File(this)
    }

private fun VesperPlayerSource.contentTypeGuess(): String {
    return listOf(uri, label)
        .firstNotNullOfOrNull { it.contentTypeFromPath() }
        ?: when (protocol) {
            io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol.Hls ->
                "application/x-mpegURL"
            io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol.Dash ->
                "application/dash+xml"
            io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol.Progressive ->
                "video/mp4"
            else -> "application/octet-stream"
        }
}

private fun String.contentTypeFromPath(): String? {
    val path = substringBefore('?').substringBefore('#').lowercase(Locale.US)
    return when {
        path.endsWith(".m3u8") -> "application/x-mpegURL"
        path.endsWith(".m3u") -> "audio/mpegurl"
        path.endsWith(".mpd") -> "application/dash+xml"
        path.endsWith(".mp4") || path.endsWith(".m4v") -> "video/mp4"
        path.endsWith(".mkv") -> "video/x-matroska"
        path.endsWith(".webm") -> "video/webm"
        path.endsWith(".mov") -> "video/quicktime"
        path.endsWith(".avi") -> "video/x-msvideo"
        path.endsWith(".3gp") -> "video/3gpp"
        path.endsWith(".mts") || path.endsWith(".ts") -> "video/mp2t"
        path.endsWith(".mp3") -> "audio/mpeg"
        path.endsWith(".m4a") -> "audio/mp4"
        path.endsWith(".aac") -> "audio/aac"
        path.endsWith(".ogg") -> "audio/ogg"
        path.endsWith(".opus") -> "audio/opus"
        path.endsWith(".wav") -> "audio/wav"
        path.endsWith(".flac") -> "audio/flac"
        path.endsWith(".wma") -> "audio/x-ms-wma"
        path.endsWith(".jpg") || path.endsWith(".jpeg") -> "image/jpeg"
        path.endsWith(".png") -> "image/png"
        path.endsWith(".gif") -> "image/gif"
        path.endsWith(".bmp") -> "image/bmp"
        path.endsWith(".webp") -> "image/webp"
        path.endsWith(".tif") || path.endsWith(".tiff") -> "image/tiff"
        else -> null
    }
}

private fun MutableMap<String, String>.addDlnaPlaybackHeaders() {
    put("Access-Control-Allow-Origin", "*")
    put("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
    put("transferMode.dlna.org", "Streaming")
    put(
        "contentFeatures.dlna.org",
        "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
    )
}

private fun Long.saturatingMinusOne(): Long = if (this <= 0L) 0L else this - 1

private fun Int.reasonPhrase(): String =
    when (this) {
        200 -> "OK"
        206 -> "Partial Content"
        400 -> "Bad Request"
        404 -> "Not Found"
        405 -> "Method Not Allowed"
        415 -> "Unsupported Media Type"
        416 -> "Range Not Satisfiable"
        503 -> "Service Unavailable"
        504 -> "Gateway Timeout"
        501 -> "Not Implemented"
        else -> "OK"
    }
