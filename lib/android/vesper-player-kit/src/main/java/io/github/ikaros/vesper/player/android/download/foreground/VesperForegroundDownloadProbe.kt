package io.github.ikaros.vesper.player.android

import android.util.Log
import androidx.media3.datasource.DataSpec
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runInterruptible

internal suspend fun VesperForegroundDownloadExecutor.fetchText(
    sourceUri: String,
    requestHeaders: Map<String, String>,
): String {
    if (isHttpUri(sourceUri)) {
        return fetchHttpText(sourceUri, requestHeaders)
    }
    val dataSource = dataSourceFactory.createDataSource()
    return try {
        runInterruptible {
            dataSource.open(
                DataSpec.Builder()
                    .setUri(sourceUri)
                    .setDownloadRequestHeaders(requestHeaders)
                    .build(),
            )
        }
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(32 * 1024)
        while (true) {
            val read =
                runInterruptible {
                    dataSource.read(buffer, 0, buffer.size)
                }
            if (read == -1) {
                break
            }
            output.write(buffer, 0, read)
        }
        output.toString(Charsets.UTF_8.name())
    } finally {
        closeDataSourceQuietly(dataSource, "fetch text $sourceUri")
    }
}

internal suspend fun VesperForegroundDownloadExecutor.probeRequiredSize(
    sourceUri: String,
    byteRange: VesperDownloadByteRange?,
    requestHeaders: Map<String, String>,
): Long {
    if (byteRange != null) {
        return byteRange.length.coerceAtLeast(0L)
    }
    return probeContentLength(sourceUri, requestHeaders)
}

internal suspend fun VesperForegroundDownloadExecutor.probeContentLength(
    sourceUri: String,
    requestHeaders: Map<String, String>,
): Long {
    val scheme = uriScheme(sourceUri)
    if (scheme.equals("file", ignoreCase = true)) {
        val fileSize = runCatching { URI(sourceUri).path?.let(::File)?.length() }.getOrNull() ?: 0L
        if (fileSize > 0L) {
            return fileSize
        }
    }
    if (scheme.equals("http", ignoreCase = true) || scheme.equals("https", ignoreCase = true)) {
        probeHttpContentLength(sourceUri, requestHeaders)?.let { return it }
    }

    val dataSource = dataSourceFactory.createDataSource()
    return try {
        val length =
            runInterruptible {
                dataSource.open(
                    DataSpec.Builder()
                        .setUri(sourceUri)
                        .setDownloadRequestHeaders(requestHeaders)
                        .build(),
                )
            }
        if (length <= 0L) {
            error("remote resource did not expose a stable content length")
        }
        length
    } finally {
        closeDataSourceQuietly(dataSource, "probe content length $sourceUri")
    }
}

internal suspend fun VesperForegroundDownloadExecutor.fetchHttpText(
    sourceUri: String,
    requestHeaders: Map<String, String>,
): String =
    runInterruptible {
        val connection = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
            applyDownloadRequestHeaders(requestHeaders)
            instanceFollowRedirects = true
            connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
        }
        try {
            val status = connection.responseCode
            if (isExpiredHttpStatus(status)) {
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            if (status !in 200..299) {
                throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
            }
            connection.inputStream.use { input ->
                input.readBytes().toString(Charsets.UTF_8)
            }
        } finally {
            connection.disconnect()
        }
    }

internal suspend fun VesperForegroundDownloadExecutor.probeHttpContentLength(
    sourceUri: String,
    requestHeaders: Map<String, String>,
): Long? =
    runInterruptible {
        val head = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
            applyDownloadRequestHeaders(requestHeaders)
            requestMethod = "HEAD"
            instanceFollowRedirects = true
            connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
        }
        val headStatus = head.responseCode
        try {
            head.inputStream.close()
        } catch (error: CancellationException) {
            throw error
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw error
        } catch (error: Exception) {
            Log.w(DOWNLOAD_TAG, "failed to close HEAD probe stream for $sourceUri", error)
            runCatching { head.errorStream?.close() }
                .onFailure { closeError ->
                    Log.w(DOWNLOAD_TAG, "failed to close HEAD probe error stream for $sourceUri", closeError)
                }
        }
        if (isExpiredHttpStatus(headStatus)) {
            head.disconnect()
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP $headStatus) for $sourceUri; refresh the media link and prepare the task again",
            )
        }
        head.getHeaderField("Content-Length")?.toLongOrNull()?.takeIf { it > 0L }?.let {
            head.disconnect()
            return@runInterruptible it
        }
        head.disconnect()

        val range = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
            applyDownloadRequestHeaders(requestHeaders)
            setRequestProperty("Range", "bytes=0-0")
            instanceFollowRedirects = true
            connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
        }
        val rangeStatus = range.responseCode
        try {
            range.inputStream.close()
        } catch (error: CancellationException) {
            throw error
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw error
        } catch (error: Exception) {
            Log.w(DOWNLOAD_TAG, "failed to close range probe stream for $sourceUri", error)
            runCatching { range.errorStream?.close() }
                .onFailure { closeError ->
                    Log.w(DOWNLOAD_TAG, "failed to close range probe error stream for $sourceUri", closeError)
                }
        }
        if (isExpiredHttpStatus(rangeStatus)) {
            range.disconnect()
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP $rangeStatus) for $sourceUri; refresh the media link and prepare the task again",
            )
        }
        val size =
            range.getHeaderField("Content-Range")
                ?.substringAfterLast('/', "")
                ?.toLongOrNull()
                ?.takeIf { it > 0L }
        range.disconnect()
        size
    }
