package io.github.umbrella22.vesper.player.android.external.internal.relay

import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import kotlin.math.min

internal fun OutputStream.writeSimpleResponse(
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

internal fun OutputStream.writeDiagnosticResponse(
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

internal fun OutputStream.writeStatusAndHeaders(
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

internal class VesperRelayChunkedOutputStream(
    private val output: OutputStream,
) : OutputStream() {
    private var finished = false

    override fun write(byte: Int) {
        write(byteArrayOf(byte.toByte()), 0, 1)
    }

    override fun write(buffer: ByteArray, offset: Int, length: Int) {
        if (offset < 0 || length < 0 || length > buffer.size - offset) {
            throw IndexOutOfBoundsException(
                "offset=$offset length=$length bufferSize=${buffer.size}",
            )
        }
        check(!finished) { "Relay chunked response is already complete" }
        if (length == 0) {
            return
        }
        output.write(length.toString(16).toByteArray(Charsets.US_ASCII))
        output.write(CRLF)
        output.write(buffer, offset, length)
        output.write(CRLF)
    }

    fun finish() {
        if (finished) {
            return
        }
        finished = true
        output.write(TERMINAL_CHUNK)
        output.flush()
    }

    private companion object {
        val CRLF = "\r\n".toByteArray(Charsets.US_ASCII)
        val TERMINAL_CHUNK = "0\r\n\r\n".toByteArray(Charsets.US_ASCII)
    }
}

internal fun InputStream.copyLimitedTo(output: OutputStream, length: Long) {
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

internal fun InputStream.skipFully(bytes: Long) {
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

internal class VesperRelayHttpLimitExceeded(
    val statusCode: Int,
    val responseMessage: String,
) : IOException(responseMessage)

internal fun InputStream.readBoundedRelayHttpLine(
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
            throw VesperRelayHttpLimitExceeded(statusCode, message)
        }
        buffer.write(byte)
    }
}

internal fun MutableMap<String, String>.addDlnaPlaybackHeaders() {
    put("Access-Control-Allow-Origin", "*")
    put("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
    put("transferMode.dlna.org", "Streaming")
    put(
        "contentFeatures.dlna.org",
        "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
    )
}

internal fun Int.reasonPhrase(): String =
    when (this) {
        200 -> "OK"
        206 -> "Partial Content"
        400 -> "Bad Request"
        404 -> "Not Found"
        405 -> "Method Not Allowed"
        414 -> "URI Too Long"
        415 -> "Unsupported Media Type"
        416 -> "Range Not Satisfiable"
        431 -> "Request Header Fields Too Large"
        503 -> "Service Unavailable"
        504 -> "Gateway Timeout"
        501 -> "Not Implemented"
        else -> "OK"
    }

internal const val MAX_RELAY_HTTP_REQUEST_LINE_BYTES = 8 * 1024
internal const val MAX_RELAY_HTTP_HEADER_LINE_BYTES = 8 * 1024
internal const val MAX_RELAY_HTTP_HEADERS = 64
