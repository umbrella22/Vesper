package io.github.ikaros.vesper.player.android

import androidx.media3.datasource.DataSpec
import java.net.HttpURLConnection
import java.net.URI

internal fun sanitizeDownloadRequestHeaders(headers: Map<String, String>): Map<String, String> =
    headers
        .mapNotNull { (name, value) ->
            val sanitizedName = name.trim().takeIf { it.isNotEmpty() } ?: return@mapNotNull null
            val sanitizedValue = value.takeIf { it.isNotBlank() } ?: return@mapNotNull null
            sanitizedName to sanitizedValue
        }
        .toMap()

internal fun NativeDownloadSource.downloadSourceHeaders(): Map<String, String> =
    sanitizeDownloadRequestHeaders(
        headerNames
            .zip(headerValues)
            .toMap(),
    )

internal fun HttpURLConnection.applyDownloadRequestHeaders(headers: Map<String, String>) {
    sanitizeDownloadRequestHeaders(headers).forEach { (name, value) ->
        setRequestProperty(name, value)
    }
}

internal fun DataSpec.Builder.setDownloadRequestHeaders(headers: Map<String, String>): DataSpec.Builder =
    setHttpRequestHeaders(sanitizeDownloadRequestHeaders(headers))

internal fun isHttpUri(uri: String): Boolean {
    val scheme = uriScheme(uri) ?: return false
    return scheme.equals("http", ignoreCase = true) || scheme.equals("https", ignoreCase = true)
}

internal fun uriScheme(uri: String): String? =
    runCatching { URI(uri).scheme }.getOrNull()

internal fun lastPathSegmentFromUri(uri: String): String? =
    runCatching {
        URI(uri).path
            ?.substringAfterLast('/')
            ?.takeIf { it.isNotBlank() }
    }.getOrNull()

internal fun requestedHttpRangeHeader(
    byteRange: VesperDownloadByteRange?,
    resumeOffset: Long,
): String? {
    val offset = resumeOffset.coerceAtLeast(0L)
    if (byteRange != null) {
        val remaining = byteRange.length.coerceAtLeast(0L) - offset
        if (remaining <= 0L) {
            return null
        }
        val start = byteRange.offset.coerceAtLeast(0L) + offset
        val end = start + remaining - 1L
        return "bytes=$start-$end"
    }
    return if (offset > 0L) "bytes=$offset-" else null
}

internal fun requestedHttpRangeStart(
    byteRange: VesperDownloadByteRange?,
    resumeOffset: Long,
): Long? {
    val offset = resumeOffset.coerceAtLeast(0L)
    return when {
        byteRange != null -> byteRange.offset.coerceAtLeast(0L) + offset
        offset > 0L -> offset
        else -> null
    }
}

internal fun parseHttpContentRangeStart(contentRange: String?): Long? {
    val range = contentRange?.substringAfter(' ', "")?.takeIf { it.isNotBlank() } ?: return null
    if (range.startsWith("*")) {
        return null
    }
    return range.substringBefore('-').toLongOrNull()
}

internal fun isExpiredHttpStatus(status: Int): Boolean =
    status == HttpURLConnection.HTTP_UNAUTHORIZED ||
        status == HttpURLConnection.HTTP_FORBIDDEN ||
        status == HttpURLConnection.HTTP_NOT_FOUND ||
        status == HttpURLConnection.HTTP_GONE

internal class VesperStaleDownloadResourceException(
    message: String,
    val resourceId: String? = null,
    val segmentId: String? = null,
    val uri: String? = null,
    val phase: VesperDownloadStaleResourcePhase? = null,
    val statusCode: Int? = null,
    val receivedBytes: Long = 0L,
) : IllegalStateException(message) {
    fun toStaleResource(
        taskId: VesperDownloadTaskId,
        fallbackResourceId: String? = null,
        fallbackSegmentId: String? = null,
        fallbackUri: String? = null,
        fallbackPhase: VesperDownloadStaleResourcePhase,
        fallbackReceivedBytes: Long = 0L,
    ): VesperDownloadStaleResource =
        VesperDownloadStaleResource(
            taskId = taskId,
            resourceId = resourceId ?: fallbackResourceId,
            segmentId = segmentId ?: fallbackSegmentId,
            uri = uri ?: fallbackUri,
            phase = phase ?: fallbackPhase,
            statusCode = statusCode,
            receivedBytes = receivedBytes.takeIf { it > 0L } ?: fallbackReceivedBytes,
            message = message ?: "offline download resource is stale or expired",
        )
}

internal fun staleDownloadResource(
    message: String,
    resourceId: String? = null,
    segmentId: String? = null,
    uri: String? = null,
    phase: VesperDownloadStaleResourcePhase? = null,
    statusCode: Int? = null,
    receivedBytes: Long = 0L,
): VesperStaleDownloadResourceException =
    VesperStaleDownloadResourceException(message, resourceId, segmentId, uri, phase, statusCode, receivedBytes)

internal class DownloadProgressThrottle(
    minProgressBytes: Long,
    minProgressIntervalMs: Long,
) {
    private val minBytes = minProgressBytes.coerceAtLeast(1L)
    private val minIntervalNs = minProgressIntervalMs.coerceAtLeast(0L) * 1_000_000L
    private var lastReportedBytes = 0L
    private var lastReportedNs = 0L

    fun shouldReport(receivedBytes: Long, force: Boolean = false): Boolean {
        if (force || receivedBytes < lastReportedBytes) {
            markReported(receivedBytes)
            return true
        }
        if (receivedBytes - lastReportedBytes < minBytes) {
            return false
        }
        val now = System.nanoTime()
        if (lastReportedNs != 0L && now - lastReportedNs < minIntervalNs) {
            return false
        }
        markReported(receivedBytes, now)
        return true
    }

    fun markReported(receivedBytes: Long) {
        markReported(receivedBytes, System.nanoTime())
    }

    private fun markReported(
        receivedBytes: Long,
        now: Long,
    ) {
        lastReportedBytes = receivedBytes
        lastReportedNs = now
    }
}

