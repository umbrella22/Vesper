package io.github.ikaros.vesper.player.android.external.internal.relay

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.io.InputStream
import java.io.OutputStream

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
