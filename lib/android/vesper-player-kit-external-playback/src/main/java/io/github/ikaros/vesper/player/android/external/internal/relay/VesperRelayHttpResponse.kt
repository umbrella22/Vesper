package io.github.ikaros.vesper.player.android.external.internal.relay

import java.io.BufferedInputStream
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
        415 -> "Unsupported Media Type"
        416 -> "Range Not Satisfiable"
        503 -> "Service Unavailable"
        504 -> "Gateway Timeout"
        501 -> "Not Implemented"
        else -> "OK"
    }
