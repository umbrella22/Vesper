package io.github.ikaros.vesper.player.android.external.internal.relay

import kotlin.math.min

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

internal fun Long.saturatingMinusOne(): Long = if (this <= 0L) 0L else this - 1
