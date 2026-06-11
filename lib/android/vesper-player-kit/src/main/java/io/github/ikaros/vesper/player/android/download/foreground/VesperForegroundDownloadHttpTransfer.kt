package io.github.ikaros.vesper.player.android

import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.runInterruptible

internal suspend fun VesperForegroundDownloadExecutor.copyHttpUriToFile(
    sourceUri: String,
    byteRange: VesperDownloadByteRange?,
    requestHeaders: Map<String, String>,
    destination: File,
    expectedSizeBytes: Long?,
    resumeFromBytes: Long,
    allowRestartAfterRangeMismatch: Boolean,
    onProgress: (Long) -> Unit,
): Long {
    val copyContext = currentCoroutineContext()
    val expected = expectedSizeBytes?.coerceAtLeast(0L)
    val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
    if (expected != null && resumeOffset >= expected) {
        return expected
    }
    if (byteRange == null && expected != null && expected > 0L && rangeChunkBytes != null) {
        return copyKnownSizeHttpUriToFile(
            sourceUri = sourceUri,
            requestHeaders = requestHeaders,
            destination = destination,
            expectedSizeBytes = expected,
            resumeFromBytes = resumeOffset,
            rangeChunkBytes = rangeChunkBytes,
            allowRestartAfterRangeMismatch = allowRestartAfterRangeMismatch,
            onProgress = onProgress,
        )
    }

    val rangeHeader = requestedHttpRangeHeader(byteRange, resumeOffset)
    val requestedRangeStart = requestedHttpRangeStart(byteRange, resumeOffset)
    val connection =
        runInterruptible {
            (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                rangeHeader?.let { setRequestProperty("Range", it) }
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
        }

    try {
        val status = runInterruptible { connection.responseCode }
        when {
            status == HttpURLConnection.HTTP_PARTIAL -> {
                val contentRangeStart = parseHttpContentRangeStart(connection.getHeaderField("Content-Range"))
                if (requestedRangeStart == null || contentRangeStart != requestedRangeStart) {
                    throw staleDownloadResource(
                        "remote server returned an unexpected Content-Range for $sourceUri",
                    )
                }
            }
            status == HttpURLConnection.HTTP_OK -> {
                if (requestedRangeStart != null) {
                    if (byteRange == null && resumeOffset > 0L && allowRestartAfterRangeMismatch) {
                        connection.disconnect()
                        runInterruptible { destination.delete() }
                        onProgress(0L)
                        return copyHttpUriToFile(
                            sourceUri = sourceUri,
                            byteRange = byteRange,
                            requestHeaders = requestHeaders,
                            destination = destination,
                            expectedSizeBytes = expectedSizeBytes,
                            resumeFromBytes = 0L,
                            allowRestartAfterRangeMismatch = false,
                            onProgress = onProgress,
                        )
                    }
                    throw staleDownloadResource(
                        "remote server did not honor the requested byte range for $sourceUri",
                    )
                }
            }
            status == ANDROID_HTTP_RANGE_NOT_SATISFIABLE -> {
                if (resumeOffset > 0L && allowRestartAfterRangeMismatch) {
                    connection.disconnect()
                    runInterruptible { destination.delete() }
                    onProgress(0L)
                    return copyHttpUriToFile(
                        sourceUri = sourceUri,
                        byteRange = byteRange,
                        requestHeaders = requestHeaders,
                        destination = destination,
                        expectedSizeBytes = expectedSizeBytes,
                        resumeFromBytes = 0L,
                        allowRestartAfterRangeMismatch = false,
                        onProgress = onProgress,
                    )
                }
                throw staleDownloadResource(
                    "remote resource rejected the requested byte range for $sourceUri",
                )
            }
            isExpiredHttpStatus(status) -> {
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            status !in 200..299 -> {
                throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
            }
        }

        var totalWritten = resumeOffset
        var reportedBytes = resumeOffset
        val append = status == HttpURLConnection.HTTP_PARTIAL && resumeOffset > 0L
        FileOutputStream(destination, append).use { output ->
            val input =
                runInterruptible {
                    connection.inputStream
                }
            input.use { stream ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    copyContext.ensureActive()
                    val read =
                        runInterruptible {
                            stream.read(buffer, 0, buffer.size)
                        }
                    if (read == -1) {
                        break
                    }
                    copyContext.ensureActive()
                    runInterruptible {
                        output.write(buffer, 0, read)
                    }
                    totalWritten += read.toLong()
                    if (expected != null && totalWritten > expected) {
                        runInterruptible { destination.delete() }
                        throw staleDownloadResource(
                            "remote server sent more bytes than expected for $sourceUri",
                        )
                    }
                    if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                        copyContext.ensureActive()
                        reportedBytes = totalWritten
                        onProgress(totalWritten)
                    }
                }
            }
        }
        if (expected != null && totalWritten != expected) {
            error("downloaded ${totalWritten} bytes for $sourceUri, expected $expected")
        }
        return totalWritten
    } finally {
        connection.disconnect()
    }
}

internal suspend fun VesperForegroundDownloadExecutor.copyKnownSizeHttpUriToFile(
    sourceUri: String,
    requestHeaders: Map<String, String>,
    destination: File,
    expectedSizeBytes: Long,
    resumeFromBytes: Long,
    rangeChunkBytes: Long,
    allowRestartAfterRangeMismatch: Boolean,
    onProgress: (Long) -> Unit,
): Long {
    val expected = expectedSizeBytes.coerceAtLeast(0L)
    var offset = resumeFromBytes.coerceAtLeast(0L)
    if (offset >= expected) {
        return expected
    }
    while (offset < expected) {
        val chunkLength = minOf(rangeChunkBytes, expected - offset)
        val chunkEnd = offset + chunkLength - 1L
        val nextOffset =
            copyHttpUriRangeChunkToFile(
                sourceUri = sourceUri,
                requestHeaders = requestHeaders,
                destination = destination,
                expectedSizeBytes = expected,
                rangeStart = offset,
                rangeEndInclusive = chunkEnd,
                rangeChunkBytes = rangeChunkBytes,
                allowRestartAfterRangeMismatch = allowRestartAfterRangeMismatch,
                onProgress = onProgress,
            )
        check(nextOffset > offset) { "download range transfer did not advance for $sourceUri" }
        offset = nextOffset
    }
    return offset
}

internal suspend fun VesperForegroundDownloadExecutor.copyHttpUriRangeChunkToFile(
    sourceUri: String,
    requestHeaders: Map<String, String>,
    destination: File,
    expectedSizeBytes: Long,
    rangeStart: Long,
    rangeEndInclusive: Long,
    rangeChunkBytes: Long,
    allowRestartAfterRangeMismatch: Boolean,
    onProgress: (Long) -> Unit,
): Long {
    val copyContext = currentCoroutineContext()
    val connection =
        runInterruptible {
            (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                setRequestProperty("Range", "bytes=$rangeStart-$rangeEndInclusive")
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
        }

    try {
        val status = runInterruptible { connection.responseCode }
        val chunkCoversWholeResource = rangeStart == 0L && rangeEndInclusive + 1L >= expectedSizeBytes
        when {
            status == HttpURLConnection.HTTP_PARTIAL -> {
                val contentRangeStart = parseHttpContentRangeStart(connection.getHeaderField("Content-Range"))
                if (contentRangeStart != rangeStart) {
                    throw staleDownloadResource(
                        "remote server returned an unexpected Content-Range for $sourceUri",
                    )
                }
            }
            status == HttpURLConnection.HTTP_OK -> {
                if (!chunkCoversWholeResource) {
                    if (rangeStart > 0L && allowRestartAfterRangeMismatch) {
                        connection.disconnect()
                        runInterruptible { destination.delete() }
                        onProgress(0L)
                        return copyKnownSizeHttpUriToFile(
                            sourceUri = sourceUri,
                            requestHeaders = requestHeaders,
                            destination = destination,
                            expectedSizeBytes = expectedSizeBytes,
                            resumeFromBytes = 0L,
                            rangeChunkBytes = rangeChunkBytes,
                            allowRestartAfterRangeMismatch = false,
                            onProgress = onProgress,
                        )
                    }
                    throw staleDownloadResource(
                        "remote server did not honor the requested byte range for $sourceUri",
                    )
                }
            }
            status == ANDROID_HTTP_RANGE_NOT_SATISFIABLE -> {
                if (rangeStart > 0L && allowRestartAfterRangeMismatch) {
                    connection.disconnect()
                    runInterruptible { destination.delete() }
                    onProgress(0L)
                    return copyKnownSizeHttpUriToFile(
                        sourceUri = sourceUri,
                        requestHeaders = requestHeaders,
                        destination = destination,
                        expectedSizeBytes = expectedSizeBytes,
                        resumeFromBytes = 0L,
                        rangeChunkBytes = rangeChunkBytes,
                        allowRestartAfterRangeMismatch = false,
                        onProgress = onProgress,
                    )
                }
                throw staleDownloadResource(
                    "remote resource rejected the requested byte range for $sourceUri",
                )
            }
            isExpiredHttpStatus(status) -> {
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            status !in 200..299 -> {
                throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
            }
        }

        val append = status == HttpURLConnection.HTTP_PARTIAL && rangeStart > 0L
        var totalWritten = if (append) rangeStart else 0L
        var reportedBytes = totalWritten
        FileOutputStream(destination, append).use { output ->
            val input =
                runInterruptible {
                    connection.inputStream
                }
            input.use { stream ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    copyContext.ensureActive()
                    val read =
                        runInterruptible {
                            stream.read(buffer, 0, buffer.size)
                        }
                    if (read == -1) {
                        break
                    }
                    copyContext.ensureActive()
                    runInterruptible {
                        output.write(buffer, 0, read)
                    }
                    totalWritten += read.toLong()
                    if (totalWritten > expectedSizeBytes) {
                        runInterruptible { destination.delete() }
                        throw staleDownloadResource(
                            "remote server sent more bytes than expected for $sourceUri",
                        )
                    }
                    if (status == HttpURLConnection.HTTP_PARTIAL && totalWritten > rangeEndInclusive + 1L) {
                        runInterruptible { destination.delete() }
                        throw staleDownloadResource(
                            "remote server sent more bytes than the requested byte range for $sourceUri",
                        )
                    }
                    if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                        copyContext.ensureActive()
                        reportedBytes = totalWritten
                        onProgress(totalWritten)
                    }
                }
            }
        }

        return if (status == HttpURLConnection.HTTP_PARTIAL) {
            val expectedNextOffset = rangeEndInclusive + 1L
            if (totalWritten != expectedNextOffset) {
                throw staleDownloadResource(
                    "downloaded range ended at $totalWritten for $sourceUri, expected $expectedNextOffset",
                )
            }
            totalWritten
        } else {
            if (totalWritten != expectedSizeBytes) {
                throw staleDownloadResource(
                    "downloaded $totalWritten bytes for $sourceUri, expected $expectedSizeBytes",
                )
            }
            totalWritten
        }
    } finally {
        connection.disconnect()
    }
}
