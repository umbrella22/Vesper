package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import android.net.Uri
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDashRemoteMediaPolicy
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.io.RandomAccessFile
import java.net.HttpURLConnection
import java.net.Inet6Address
import java.net.InetAddress
import java.net.URI
import java.net.URL
import java.util.Collections
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean

internal data class VesperRelayDashSourceOrigin(
    val kind: String,
    val manifestUri: String,
    val rootUri: String,
    val allowRemoteMediaReferences: Boolean = false,
)

internal open class VesperRelayDashResourceResolver(
    val origin: VesperRelayDashSourceOrigin,
    val manifestLogicalUri: String,
) {
    @Throws(IOException::class)
    open fun readManifest(): String = throw UnsupportedOperationException()

    @Throws(IOException::class, HostInputCancelledException::class)
    open fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        throw UnsupportedOperationException()
    }

    @Throws(IOException::class, HostInputCancelledException::class)
    open fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        throw UnsupportedOperationException()
    }

    @Throws(IOException::class)
    open fun readRange(
        uri: String,
        range: VesperRelayDashByteRange,
    ): ByteArray {
        ByteArrayOutputStream(range.lengthAsInt()).use { output ->
            copyRangeTo(uri, range, output, AtomicBoolean(false))
            return output.toByteArray()
        }
    }

    open fun cancel() = Unit
}

internal class VesperRelayDashResourceResolverFactory {
    fun create(
        context: Context,
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayDashResourceResolver {
        val uri = request.source.uri
        return when {
            uri.startsWith("http://", ignoreCase = true) ||
                uri.startsWith("https://", ignoreCase = true) ->
                VesperRelayHttpDashResourceResolver(
                    source = request.source,
                    requestHeaders = request.requestHeaders,
                    remoteMediaPolicy = request.dashRemoteMediaPolicy,
                )
            uri.startsWith("content://", ignoreCase = true) ->
                VesperRelayContentDashResourceResolver(
                    context = context,
                    source = request.source,
                    remoteHeaders = mergedRemoteHeaders(
                        source = request.source,
                        requestHeaders = request.requestHeaders,
                        allowedHeaderNames = request.dashRemoteMediaPolicy.allowedRequestHeaders,
                    ),
                    remoteMediaPolicy = request.dashRemoteMediaPolicy,
                )
            else ->
                fileDashResolver(request.source, request.requestHeaders, request.dashRemoteMediaPolicy)
        }
    }
}

private fun fileDashResolver(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy,
): VesperRelayFileDashResourceResolver =
    VesperRelayFileDashResourceResolver(
        origin = source.uri.toFileDashOrigin(
            allowRemoteMediaReferences = remoteMediaPolicy.allowRemoteReferences,
        ),
        remoteHeaders = mergedRemoteHeaders(
            source = source,
            requestHeaders = requestHeaders,
            allowedHeaderNames = remoteMediaPolicy.allowedRequestHeaders,
        ),
        remoteMediaPolicy = remoteMediaPolicy,
    )

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

internal class VesperRelayHttpDashResourceResolver(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
) : VesperRelayDashResourceResolver(
    origin = VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = source.uri,
        rootUri = source.uri,
    ),
    manifestLogicalUri = source.uri,
) {
    private val remoteClient = VesperRelayRemoteDashResourceClient(
        headers = mergedRemoteHeaders(source, requestHeaders),
        allowPrivateAddresses = remoteMediaPolicy.allowPrivateAddresses,
    )

    override fun readManifest(): String =
        remoteClient.readUtf8(manifestLogicalUri)

    override fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) = remoteClient.copyTo(uri, output, cancellation)

    override fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) = remoteClient.copyRangeTo(uri, range, output, cancellation)

    override fun cancel() = remoteClient.cancel()
}

internal class VesperRelayFileDashResourceResolver internal constructor(
    origin: VesperRelayDashSourceOrigin,
    remoteHeaders: Map<String, String> = emptyMap(),
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
) : VesperRelayDashResourceResolver(
    origin = origin,
    manifestLogicalUri = origin.manifestUri,
) {
    private val rootDirectory = File(URI(origin.rootUri)).canonicalFile
    private val remoteClient = origin.remoteMediaClient(
        headers = remoteHeaders.filterRemoteFetchHeaders(remoteMediaPolicy.allowedRequestHeaders),
        remoteMediaPolicy = remoteMediaPolicy,
    )

    override fun readManifest(): String {
        val file = fileForLogicalUri(manifestLogicalUri)
        return file.readText(Charsets.UTF_8)
    }

    override fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyTo(uri, output, cancellation)
            return
        }
        fileForLogicalUri(uri).inputStream().use { input ->
            input.copyToCancellable(output, cancellation)
        }
    }

    override fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyRangeTo(uri, range, output, cancellation)
            return
        }
        val file = fileForLogicalUri(uri)
        RandomAccessFile(file, "r").use { input ->
            if (range.end >= input.length()) {
                throw DashResourceException(
                    code = "dash_resource_not_found",
                    status = 416,
                    message = "DASH file resource is shorter than requested byte range.",
                )
            }
            input.seek(range.start)
            input.copyLimitedToCancellable(output, range.length, cancellation)
        }
    }

    override fun cancel() {
        remoteClient?.cancel()
    }

    private fun remoteClientFor(uri: String): VesperRelayRemoteDashResourceClient {
        return remoteClient ?: throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH file resolver cannot fetch remote media references.",
        )
    }

    private fun fileForLogicalUri(uri: String): File {
        val file =
            try {
                File(URI(uri)).canonicalFile
            } catch (error: Exception) {
                throw DashResourceException(
                    code = "unsupported_dash_source_origin",
                    status = 415,
                    message = "DASH file resource URI is invalid: ${error.message ?: error.javaClass.simpleName}",
                )
            }
        if (!file.toPath().startsWith(rootDirectory.toPath())) {
            throw DashResourceException(
                code = "unsupported_mixed_dash_origin",
                status = 415,
                message = "DASH file resource escapes the manifest directory.",
            )
        }
        if (!file.exists()) {
            throw FileNotFoundException(file.absolutePath)
        }
        return file
    }
}

internal class VesperRelayContentDashResourceResolver(
    context: Context,
    source: VesperPlayerSource,
    remoteHeaders: Map<String, String> = emptyMap(),
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
) : VesperRelayDashResourceResolver(
    origin = source.uri.toContentDashOrigin(
        allowRemoteMediaReferences = remoteMediaPolicy.allowRemoteReferences,
    ),
    manifestLogicalUri = source.uri,
) {
    private val resolver = context.contentResolver
    private val rootUri = Uri.parse(origin.rootUri)
    private val remoteClient = origin.remoteMediaClient(
        headers = remoteHeaders.filterRemoteFetchHeaders(remoteMediaPolicy.allowedRequestHeaders),
        remoteMediaPolicy = remoteMediaPolicy,
    )

    override fun readManifest(): String {
        return openInput(manifestLogicalUri).use { input ->
            input.readBytes().toString(Charsets.UTF_8)
        }
    }

    override fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyTo(uri, output, cancellation)
            return
        }
        openInput(uri).use { input ->
            input.copyToCancellable(output, cancellation)
        }
    }

    override fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyRangeTo(uri, range, output, cancellation)
            return
        }
        openInput(uri).use { input ->
            input.skipFullyCancellable(range.start, cancellation)
            input.copyLimitedToCancellable(output, range.length, cancellation)
        }
    }

    override fun cancel() {
        remoteClient?.cancel()
    }

    private fun remoteClientFor(uri: String): VesperRelayRemoteDashResourceClient {
        return remoteClient ?: throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH content resolver cannot fetch remote media references.",
        )
    }

    private fun openInput(uri: String): InputStream {
        val parsed = Uri.parse(uri)
        if (parsed.scheme?.equals("content", ignoreCase = true) != true ||
            parsed.authority != rootUri.authority ||
            !parsed.path.orEmpty().startsWith(rootUri.path.orEmpty())
        ) {
            throw DashResourceException(
                code = "unsupported_mixed_dash_origin",
                status = 415,
                message = "DASH content resource is outside the manifest provider root.",
            )
        }
        return resolver.openInputStream(parsed)
            ?: throw DashResourceException(
                code = "dash_resource_not_found",
                status = 404,
                message = "DASH content resource is not available.",
            )
    }
}

internal class HostInputCancelledException : IOException("Host input cancelled.")

private fun VesperRelayDashSourceOrigin.remoteMediaClient(
    headers: Map<String, String>,
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy,
): VesperRelayRemoteDashResourceClient? =
    if (allowRemoteMediaReferences) {
        VesperRelayRemoteDashResourceClient(
            headers = headers,
            allowPrivateAddresses = remoteMediaPolicy.allowPrivateAddresses,
        )
    } else {
        null
    }

internal fun String.isRemoteDashUri(): Boolean =
    startsWith("http://", ignoreCase = true) || startsWith("https://", ignoreCase = true)

private fun validateRemoteDashUri(
    uri: String,
    allowPrivateAddresses: Boolean,
) {
    val parsed =
        try {
            URI(uri)
        } catch (error: Exception) {
            throw DashResourceException(
                code = "unsupported_mixed_dash_origin",
                status = 415,
                message = "DASH remote media URI is invalid: ${error.message ?: error.javaClass.simpleName}",
            )
        }
    val scheme = parsed.scheme?.lowercase(Locale.US)
    if (scheme != "http" && scheme != "https") {
        throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH remote media URI must use http or https.",
        )
    }
    val host = parsed.host?.takeIf { it.isNotBlank() }
        ?: throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH remote media URI must include a host.",
        )
    if (allowPrivateAddresses) {
        return
    }
    val addresses =
        try {
            InetAddress.getAllByName(host)
        } catch (error: Exception) {
            throw DashResourceException(
                code = "host_fetch_failed",
                status = 502,
                message = "DASH remote media host could not be resolved: ${error.message ?: error.javaClass.simpleName}",
            )
        }
    if (addresses.isEmpty() || addresses.any(InetAddress::isPrivateDashAddress)) {
        throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH remote media URI resolves to a private or local address.",
        )
    }
}

private fun InetAddress.isPrivateDashAddress(): Boolean =
    isAnyLocalAddress ||
        isLoopbackAddress ||
        isLinkLocalAddress ||
        isSiteLocalAddress ||
        isMulticastAddress ||
        isUniqueLocalIpv6Address()

private fun InetAddress.isUniqueLocalIpv6Address(): Boolean {
    if (this !is Inet6Address) {
        return false
    }
    val first = address.firstOrNull()?.toInt()?.and(0xff) ?: return false
    return first and 0xfe == 0xfc
}

private fun mergedRemoteHeaders(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
    allowedHeaderNames: Set<String>? = null,
): Map<String, String> {
    val merged = linkedMapOf<String, String>()
    source.headers.forEach { (name, value) ->
        if (name.isRemoteFetchHeaderAllowed(allowedHeaderNames) && value.isNotBlank()) {
            merged[name] = value
        }
    }
    requestHeaders.forEach { (name, value) ->
        if (name.isRemoteFetchHeaderAllowed(allowedHeaderNames) && value.isNotBlank()) {
            merged[name] = value
        }
    }
    return merged
}

private fun Map<String, String>.filterRemoteFetchHeaders(
    allowedHeaderNames: Set<String>? = null,
): Map<String, String> =
    filter { (name, value) -> name.isRemoteFetchHeaderAllowed(allowedHeaderNames) && value.isNotBlank() }

private fun String.isRemoteFetchHeaderAllowed(allowedHeaderNames: Set<String>? = null): Boolean {
    val normalized = lowercase(Locale.US)
    if (normalized in REMOTE_FETCH_NEVER_HEADERS) {
        return false
    }
    return allowedHeaderNames == null ||
        allowedHeaderNames.any { allowed -> allowed.equals(this, ignoreCase = true) }
}

private val REMOTE_FETCH_NEVER_HEADERS = setOf(
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
    "proxy-connection",
)

private const val MAX_REMOTE_DASH_REDIRECTS = 5

private val HTTP_REDIRECT_STATUSES = setOf(
    HttpURLConnection.HTTP_MOVED_PERM,
    HttpURLConnection.HTTP_MOVED_TEMP,
    HttpURLConnection.HTTP_SEE_OTHER,
    307,
    308,
)

private val CONTENT_RANGE_PATTERN = Regex("""bytes\s+(\d+)-(\d+)/(\d+|\*)""", RegexOption.IGNORE_CASE)

private fun contentRangeMatches(
    value: String?,
    range: VesperRelayDashByteRange,
): Boolean {
    val match = value?.let { CONTENT_RANGE_PATTERN.matchEntire(it.trim()) } ?: return false
    val start = match.groupValues[1].toLongOrNull() ?: return false
    val end = match.groupValues[2].toLongOrNull() ?: return false
    return start == range.start && end == range.end
}

private class DashResourceException(
    val code: String,
    val status: Int,
    override val message: String,
) : IOException(message)

internal fun IOException.dashResourceErrorCode(): String =
    when (this) {
        is DashResourceException -> code
        is FileNotFoundException -> "dash_resource_not_found"
        else -> message?.httpErrorCode() ?: "host_fetch_failed"
    }

internal fun IOException.dashResourceHttpStatus(): Int =
    when (this) {
        is DashResourceException -> status
        is FileNotFoundException -> 404
        else -> message?.httpErrorStatus() ?: 502
    }

private fun String.httpErrorStatus(): Int? =
    Regex("""HTTP\s+(\d{3})""", RegexOption.IGNORE_CASE)
        .find(this)
        ?.groupValues
        ?.getOrNull(1)
        ?.toIntOrNull()

private fun String.httpErrorCode(): String? =
    httpErrorStatus()?.let { status ->
        if (status == 401 || status == 403) {
            "dash_resource_permission_denied"
        } else if (status == 404) {
            "dash_resource_not_found"
        } else {
            "host_fetch_failed"
        }
    }

private fun String.toFileDashOrigin(
    allowRemoteMediaReferences: Boolean = false,
): VesperRelayDashSourceOrigin {
    val manifestFile = toLocalDashFile().canonicalFile
    val root = manifestFile.parentFile?.canonicalFile ?: manifestFile.canonicalFile
    return VesperRelayDashSourceOrigin(
        kind = "file",
        manifestUri = manifestFile.toURI().toString(),
        rootUri = root.toURI().toString(),
        allowRemoteMediaReferences = allowRemoteMediaReferences,
    )
}

private fun String.toLocalDashFile(): File =
    if (startsWith("file://", ignoreCase = true)) {
        File(URI(this))
    } else {
        File(this)
    }

private fun String.toContentDashOrigin(
    allowRemoteMediaReferences: Boolean = false,
): VesperRelayDashSourceOrigin {
    val uri = URI(this)
    val path = uri.path.orEmpty()
    val rootPath = path.substringBeforeLast('/', missingDelimiterValue = "")
    val rootUri = URI(uri.scheme, uri.authority, rootPath, null, null).toString()
    return VesperRelayDashSourceOrigin(
        kind = "content",
        manifestUri = this,
        rootUri = rootUri,
        allowRemoteMediaReferences = allowRemoteMediaReferences,
    )
}

internal fun contentUriWithinRoot(uri: String, rootUri: String): Boolean {
    val parsed = runCatching { URI(uri) }.getOrNull() ?: return false
    val root = runCatching { URI(rootUri) }.getOrNull() ?: return false
    return parsed.scheme?.equals("content", ignoreCase = true) == true &&
        parsed.authority == root.authority &&
        parsed.path.orEmpty().startsWith(root.path.orEmpty())
}

private fun InputStream.copyToCancellable(
    output: OutputStream,
    cancellation: AtomicBoolean,
) {
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    while (true) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val read = read(buffer)
        if (read < 0) {
            return
        }
        output.write(buffer, 0, read)
    }
}

private fun InputStream.copyLimitedToCancellable(
    output: OutputStream,
    length: Long,
    cancellation: AtomicBoolean,
) {
    var remaining = length
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    while (remaining > 0L) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val read = read(buffer, 0, minOf(buffer.size.toLong(), remaining).toInt())
        if (read < 0) {
            return
        }
        output.write(buffer, 0, read)
        remaining -= read.toLong()
    }
}

private fun RandomAccessFile.copyLimitedToCancellable(
    output: OutputStream,
    length: Long,
    cancellation: AtomicBoolean,
) {
    var remaining = length
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    while (remaining > 0L) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val read = read(buffer, 0, minOf(buffer.size.toLong(), remaining).toInt())
        if (read < 0) {
            return
        }
        output.write(buffer, 0, read)
        remaining -= read.toLong()
    }
}

private fun InputStream.skipFullyCancellable(
    bytes: Long,
    cancellation: AtomicBoolean,
) {
    var remaining = bytes
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    while (remaining > 0L) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val skipped = skip(remaining)
        if (skipped > 0L) {
            remaining -= skipped
            continue
        }
        val read = read(buffer, 0, minOf(buffer.size.toLong(), remaining).toInt())
        if (read < 0) {
            throw DashResourceException(
                code = "dash_resource_not_found",
                status = 416,
                message = "DASH resource is shorter than requested byte range.",
            )
        }
        remaining -= read.toLong()
    }
}
