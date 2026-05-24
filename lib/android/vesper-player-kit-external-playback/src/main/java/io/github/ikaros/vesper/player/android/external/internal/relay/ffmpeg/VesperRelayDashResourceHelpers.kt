package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.io.RandomAccessFile
import java.net.Inet6Address
import java.net.InetAddress
import java.net.URI
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean

internal class HostInputCancelledException : IOException("Host input cancelled.")

internal fun String.isRemoteDashUri(): Boolean =
    startsWith("http://", ignoreCase = true) || startsWith("https://", ignoreCase = true)

internal fun validateRemoteDashUri(
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

internal fun mergedRemoteHeaders(
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

internal fun Map<String, String>.filterRemoteFetchHeaders(
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

private val CONTENT_RANGE_PATTERN = Regex("""bytes\s+(\d+)-(\d+)/(\d+|\*)""", RegexOption.IGNORE_CASE)

internal fun contentRangeMatches(
    value: String?,
    range: VesperRelayDashByteRange,
): Boolean {
    val match = value?.let { CONTENT_RANGE_PATTERN.matchEntire(it.trim()) } ?: return false
    val start = match.groupValues[1].toLongOrNull() ?: return false
    val end = match.groupValues[2].toLongOrNull() ?: return false
    return start == range.start && end == range.end
}

internal class DashResourceException(
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

internal fun String.toFileDashOrigin(
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

internal fun String.toContentDashOrigin(
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

internal fun InputStream.copyToCancellable(
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

internal fun InputStream.copyLimitedToCancellable(
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

internal fun RandomAccessFile.copyLimitedToCancellable(
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

internal fun InputStream.skipFullyCancellable(
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
