package io.github.ikaros.vesper.example.androidcomposehost

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL

internal suspend fun saveVideoToGallery(
    context: Context,
    completedPath: String,
): Uri = withContext(Dispatchers.IO) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
        error("This demo saves to the gallery on Android 10 or newer.")
    }

    val sourceFile = resolveCompletedFile(completedPath)
    require(sourceFile.isFile) { "The completed download output is unavailable." }

    val resolver = context.contentResolver
    val targetCollection =
        MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
    val displayName =
        sourceFile.name.takeIf { it.isNotBlank() }
            ?: "vesper-download-${System.currentTimeMillis()}.mp4"
    val mimeType = guessVideoMimeType(sourceFile)
    val values =
        ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, displayName)
            put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
            put(
                MediaStore.MediaColumns.RELATIVE_PATH,
                "${Environment.DIRECTORY_MOVIES}/Vesper Player Host",
            )
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }

    val targetUri =
        resolver.insert(targetCollection, values)
            ?: error("Failed to create a gallery entry for the exported video.")

    try {
        sourceFile.inputStream().use { input ->
            resolver.openOutputStream(targetUri)?.use { output ->
                input.copyTo(output)
            } ?: error("Failed to open the gallery output stream.")
        }

        resolver.update(
            targetUri,
            ContentValues().apply {
                put(MediaStore.MediaColumns.IS_PENDING, 0)
            },
            null,
            null,
        )
        targetUri
    } catch (error: Throwable) {
        resolver.delete(targetUri, null, null)
        throw error
    }
}

internal fun createDownloadExportFile(
    context: Context,
    task: VesperDownloadTaskSnapshot,
): File {
    val exportDirectory =
        File(context.cacheDir, "vesper-exported-videos").apply {
            mkdirs()
        }
    val safeStem =
        task.assetId
            .ifBlank { "download-${task.taskId}" }
            .replace(Regex("[^A-Za-z0-9._-]"), "_")
    return File(exportDirectory, "$safeStem.mp4")
}

internal suspend fun prepareSegmentedExportManifestIfNeeded(
    task: VesperDownloadTaskSnapshot,
): DownloadExportManifestMutation? = withContext(Dispatchers.IO) {
    val completedPath = task.assetIndex.completedPath ?: return@withContext null
    val manifestFile = resolveCompletedFile(completedPath)
    if (!manifestFile.isFile) {
        return@withContext null
    }
    if (
        task.assetIndex.resources.isNotEmpty() ||
        task.assetIndex.segments.isNotEmpty()
    ) {
        return@withContext null
    }

    val manifestUri =
        task.source.manifestUri
            ?.takeIf { it.isNotBlank() }
            ?: task.source.source.uri.takeIf { it.isNotBlank() }
            ?: return@withContext null
    val extension = manifestFile.extension.lowercase()
    val originalText = manifestFile.readText()
    val rewrittenText =
        when {
            task.source.contentFormat == VesperDownloadContentFormat.HlsSegments &&
                extension == "m3u8" -> materializeHlsExportManifestText(originalText, manifestUri)

            task.source.contentFormat == VesperDownloadContentFormat.DashSegments &&
                extension == "mpd" -> rewriteDashManifestForRemoteExport(originalText, manifestUri)

            else -> originalText
        }

    if (rewrittenText == originalText) {
        return@withContext null
    }

    val originalBytes = manifestFile.readBytes()
    manifestFile.writeText(rewrittenText)
    DownloadExportManifestMutation(manifestFile, originalBytes)
}

internal class DownloadExportManifestMutation(
    private val manifestFile: File,
    private val originalBytes: ByteArray,
) {
    suspend fun restore() = withContext(Dispatchers.IO) {
        manifestFile.writeBytes(originalBytes)
    }
}

private fun resolveCompletedFile(path: String): File {
    if (path.startsWith("file://")) {
        val uriPath = Uri.parse(path).path
        if (!uriPath.isNullOrBlank()) {
            return File(uriPath)
        }
    }
    return File(path)
}

private fun guessVideoMimeType(file: File): String {
    val extension =
        file.extension
            .lowercase()
            .takeIf { value -> value.isNotBlank() }
            ?: return "video/mp4"
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) ?: "video/mp4"
}

private fun rewriteHlsManifestForRemoteExport(
    manifestText: String,
    manifestUri: String,
): String =
    manifestText
        .lineSequence()
        .joinToString(separator = "\n") { line ->
            val trimmed = line.trim()
            when {
                trimmed.isEmpty() -> line
                trimmed.startsWith("#") -> {
                    HLS_URI_ATTRIBUTE_PATTERN.replace(line) { match ->
                        val resolvedUri = resolveRemoteReference(manifestUri, match.groupValues[1])
                        "URI=\"$resolvedUri\""
                    }
                }
                else -> resolveRemoteReference(manifestUri, trimmed)
            }
        }

private fun materializeHlsExportManifestText(
    manifestText: String,
    manifestUri: String,
): String {
    val mediaPlaylistUri = selectPrimaryHlsMediaPlaylistUri(manifestText, manifestUri)
    if (mediaPlaylistUri == null) {
        return rewriteHlsManifestForRemoteExport(manifestText, manifestUri)
    }

    val mediaPlaylistText =
        runCatching { fetchRemoteText(mediaPlaylistUri) }.getOrElse {
            return rewriteHlsManifestForRemoteExport(manifestText, manifestUri)
        }
    return rewriteHlsManifestForRemoteExport(mediaPlaylistText, mediaPlaylistUri)
}

private fun rewriteDashManifestForRemoteExport(
    manifestText: String,
    manifestUri: String,
): String {
    var rewritten = manifestText
    rewritten =
        DASH_BASE_URL_PATTERN.replace(rewritten) { match ->
            val resolvedUri = resolveRemoteReference(manifestUri, match.groupValues[1].trim())
            "<BaseURL>$resolvedUri</BaseURL>"
        }

    if (!DASH_BASE_URL_PATTERN.containsMatchIn(rewritten)) {
        val resolvedBaseUri = resolveRemoteReference(manifestUri, "./")
        rewritten =
            rewritten.replaceFirst(
                DASH_ROOT_TAG_PATTERN,
                "$0\n  <BaseURL>$resolvedBaseUri</BaseURL>",
            )
    }
    return rewritten
}

internal fun resolveRemoteReference(
    manifestUri: String,
    reference: String,
): String {
    if (reference.isBlank()) {
        return reference
    }
    return runCatching {
        URI(manifestUri).resolve(reference).toString()
    }.getOrDefault(reference)
}

private fun selectPrimaryHlsMediaPlaylistUri(
    manifestText: String,
    manifestUri: String,
): String? {
    var expectVariantUri = false
    manifestText.lineSequence().forEach { line ->
        val trimmed = line.trim()
        when {
            trimmed.startsWith("#EXT-X-STREAM-INF", ignoreCase = true) -> {
                expectVariantUri = true
            }
            expectVariantUri && trimmed.isNotEmpty() && !trimmed.startsWith("#") -> {
                return resolveRemoteReference(manifestUri, trimmed)
            }
        }
    }
    return null
}

internal fun fetchRemoteText(uri: String): String {
    val connection = URL(uri).openConnection() as HttpURLConnection
    connection.instanceFollowRedirects = true
    connection.connectTimeout = 15_000
    connection.readTimeout = 15_000
    connection.requestMethod = "GET"
    return try {
        connection.inputStream.bufferedReader().use { reader -> reader.readText() }
    } finally {
        connection.disconnect()
    }
}

private val HLS_URI_ATTRIBUTE_PATTERN = Regex("URI=\"([^\"]+)\"")
private val DASH_BASE_URL_PATTERN = Regex("""<BaseURL>(.*?)</BaseURL>""", RegexOption.IGNORE_CASE)
private val DASH_ROOT_TAG_PATTERN = Regex("""<MPD\b[^>]*>""", RegexOption.IGNORE_CASE)
