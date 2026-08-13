package io.github.umbrella22.vesper.player.android

import android.util.Log
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import java.io.File
import java.io.FileOutputStream
import java.net.URI
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.runInterruptible

internal fun VesperForegroundDownloadExecutor.closeDataSourceQuietly(dataSource: DataSource, context: String) {
    runCatching { dataSource.close() }
        .onFailure { error -> Log.w(DOWNLOAD_TAG, "failed to close download data source for $context", error) }
}

internal suspend fun VesperForegroundDownloadExecutor.copyUriToFile(
    sourceUri: String,
    byteRange: VesperDownloadByteRange?,
    requestHeaders: Map<String, String>,
    destination: File,
    expectedSizeBytes: Long?,
    resumeFromBytes: Long,
    onProgress: (Long) -> Unit,
): Long {
    if (isHttpUri(sourceUri)) {
        return copyHttpUriToFile(
            sourceUri = sourceUri,
            byteRange = byteRange,
            requestHeaders = requestHeaders,
            destination = destination,
            expectedSizeBytes = expectedSizeBytes,
            resumeFromBytes = resumeFromBytes,
            allowRestartAfterRangeMismatch = true,
            onProgress = onProgress,
        )
    }
    if (uriScheme(sourceUri).equals("file", ignoreCase = true)) {
        return copyLocalFileUriToFile(
            sourceUri = sourceUri,
            byteRange = byteRange,
            destination = destination,
            expectedSizeBytes = expectedSizeBytes,
            resumeFromBytes = resumeFromBytes,
            onProgress = onProgress,
        )
    }
    return copyDataSourceUriToFile(
        sourceUri = sourceUri,
        byteRange = byteRange,
        requestHeaders = requestHeaders,
        destination = destination,
        expectedSizeBytes = expectedSizeBytes,
        resumeFromBytes = resumeFromBytes,
        onProgress = onProgress,
    )
}

internal suspend fun VesperForegroundDownloadExecutor.copyLocalFileUriToFile(
    sourceUri: String,
    byteRange: VesperDownloadByteRange?,
    destination: File,
    expectedSizeBytes: Long?,
    resumeFromBytes: Long,
    onProgress: (Long) -> Unit,
): Long {
    val copyContext = currentCoroutineContext()
    val expected = expectedSizeBytes?.coerceAtLeast(0L)
    val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
    if (expected != null && resumeOffset >= expected) {
        return expected
    }
    val sourceFile = File(URI(sourceUri))
    val startOffset = (byteRange?.offset ?: 0L).coerceAtLeast(0L) + resumeOffset
    var remaining = byteRange?.let { (it.length.coerceAtLeast(0L) - resumeOffset).coerceAtLeast(0L) }
    var totalWritten = resumeOffset
    var reportedBytes = resumeOffset
    runInterruptible {
        sourceFile.inputStream().use { input ->
            if (startOffset > 0L) {
                input.skip(startOffset)
            }
            FileOutputStream(destination, resumeOffset > 0L).use { output ->
                val buffer = ByteArray(64 * 1024)
                while (remaining == null || remaining!! > 0L) {
                    copyContext.ensureActive()
                    val limit = minOf(buffer.size.toLong(), remaining ?: buffer.size.toLong()).toInt()
                    val read = input.read(buffer, 0, limit)
                    if (read == -1) {
                        break
                    }
                    output.write(buffer, 0, read)
                    totalWritten += read.toLong()
                    remaining = remaining?.let { (it - read).coerceAtLeast(0L) }
                    if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                        reportedBytes = totalWritten
                        onProgress(totalWritten)
                    }
                }
            }
        }
    }
    if (expected != null && totalWritten != expected) {
        error("copied $totalWritten bytes for $sourceUri, expected $expected")
    }
    return totalWritten
}

internal suspend fun VesperForegroundDownloadExecutor.copyDataSourceUriToFile(
    sourceUri: String,
    byteRange: VesperDownloadByteRange?,
    requestHeaders: Map<String, String>,
    destination: File,
    expectedSizeBytes: Long?,
    resumeFromBytes: Long,
    onProgress: (Long) -> Unit,
): Long {
    val copyContext = currentCoroutineContext()
    val expected = expectedSizeBytes?.coerceAtLeast(0L)
    val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
    if (expected != null && resumeOffset >= expected) {
        return expected
    }
    val dataSource = dataSourceFactory.createDataSource()
    val dataSpecBuilder = DataSpec.Builder()
        .setUri(sourceUri)
        .setDownloadRequestHeaders(requestHeaders)
    if (byteRange != null) {
        val remaining = byteRange.length.coerceAtLeast(0L) - resumeOffset
        dataSpecBuilder
            .setPosition(byteRange.offset.coerceAtLeast(0L) + resumeOffset)
            .setLength(remaining.coerceAtLeast(0L))
    } else if (resumeOffset > 0L) {
        dataSpecBuilder.setPosition(resumeOffset)
    }
    val dataSpec = dataSpecBuilder.build()
    var totalWritten = resumeOffset
    var reportedBytes = resumeOffset
    FileOutputStream(destination, resumeOffset > 0L).use { output ->
        try {
            copyContext.ensureActive()
            runInterruptible {
                dataSource.open(dataSpec)
            }
            val buffer = ByteArray(64 * 1024)
            while (true) {
                copyContext.ensureActive()
                val read =
                    runInterruptible {
                        dataSource.read(buffer, 0, buffer.size)
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
                    runCatching { destination.delete() }
                    error("remote server did not honor the requested resume range for $sourceUri")
                }
                if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                    copyContext.ensureActive()
                    reportedBytes = totalWritten
                    onProgress(totalWritten)
                }
            }
        } finally {
            closeDataSourceQuietly(dataSource, "copy $sourceUri")
        }
    }
    if (expected != null && totalWritten != expected) {
        error("downloaded ${totalWritten} bytes for $sourceUri, expected $expected")
    }
    return totalWritten
}

internal fun VesperForegroundDownloadExecutor.resumableExistingBytes(
    destination: File,
    expectedSizeBytes: Long?,
): Long {
    if (!destination.exists()) {
        return 0L
    }
    if (!resumePartialDownloads) {
        destination.delete()
        return 0L
    }
    val expected = expectedSizeBytes?.coerceAtLeast(0L)
    if (expected == null) {
        destination.delete()
        return 0L
    }
    val existing = destination.length().coerceAtLeast(0L)
    return when {
        existing == expected -> existing
        existing in 1 until expected -> existing
        else -> {
            destination.delete()
            0L
        }
    }
}

internal fun VesperForegroundDownloadExecutor.inferredFileName(uri: String): String =
    lastPathSegmentFromUri(uri) ?: "media.bin"
