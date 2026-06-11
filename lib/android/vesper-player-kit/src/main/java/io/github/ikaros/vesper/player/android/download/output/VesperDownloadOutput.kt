package io.github.ikaros.vesper.player.android

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import java.io.File
import java.io.FileInputStream

internal fun shareDownloadTaskOutput(
    context: Context,
    source: File,
    fileName: String?,
    mimeType: String?,
    authority: String,
) {
    val sharedFile = preparedShareFile(context, source, fileName)
    val uri = FileProvider.getUriForFile(context, authority, sharedFile)
    val intent =
        Intent(Intent.ACTION_SEND)
            .setType(mimeType ?: guessMimeType(sharedFile))
            .putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    val chooser = Intent.createChooser(intent, null)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    if (context !is android.app.Activity) {
        chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    context.startActivity(chooser)
}

internal fun saveDownloadTaskOutput(
    context: Context,
    source: File,
    fileName: String?,
    collection: VesperDownloadPublicCollection,
): Uri {
    check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        "saveTaskOutput requires Android 10 or newer MediaStore scoped storage"
    }
    val displayName = sanitizedOutputFileName(fileName ?: source.name)
    val mimeType = guessMimeType(source)
    val values =
        ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, displayName)
            put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
            put(MediaStore.MediaColumns.RELATIVE_PATH, collection.relativePath)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
    val resolver = context.contentResolver
    val collectionUri = collection.contentUri
    val uri = checkNotNull(resolver.insert(collectionUri, values)) {
        "MediaStore did not allocate an output URI"
    }
    runCatching {
        resolver.openOutputStream(uri)?.use { output ->
            FileInputStream(source).use { input ->
                input.copyTo(output)
            }
        } ?: error("MediaStore output stream was unavailable")
        values.clear()
        values.put(MediaStore.MediaColumns.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
    }.onFailure { error ->
        resolver.delete(uri, null, null)
        throw error
    }.getOrThrow()
    return uri
}

internal fun preparedShareFile(
    context: Context,
    source: File,
    fileName: String?,
): File {
    val safeFileName = fileName?.takeIf { it.isNotBlank() }?.let(::sanitizedOutputFileName)
        ?: source.name
    if (safeFileName == source.name && source.absolutePath.startsWith(context.filesDir.absolutePath)) {
        return source
    }
    val directory = File(context.cacheDir, "vesper-download-share")
    directory.mkdirs()
    val target = File(directory, safeFileName)
    if (target.absolutePath != source.absolutePath) {
        source.copyTo(target, overwrite = true)
    }
    return target
}

internal fun sanitizedOutputFileName(value: String): String {
    val sanitized = value.replace(Regex("[^A-Za-z0-9._ -]+"), "_").trim('.', ' ')
    return sanitized.takeIf { it.isNotBlank() && it != ".." } ?: "vesper-download"
}

internal fun guessMimeType(file: File): String {
    val extension = file.extension.takeIf { it.isNotBlank() } ?: return "application/octet-stream"
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension.lowercase())
        ?: when (extension.lowercase()) {
            "m3u8" -> "application/vnd.apple.mpegurl"
            "mpd" -> "application/dash+xml"
            "mp4" -> "video/mp4"
            "mkv" -> "video/x-matroska"
            "ts" -> "video/mp2t"
            else -> "application/octet-stream"
        }
}

internal val VesperDownloadPublicCollection.relativePath: String
    get() =
        when (this) {
            VesperDownloadPublicCollection.Downloads -> Environment.DIRECTORY_DOWNLOADS
            VesperDownloadPublicCollection.Movies -> Environment.DIRECTORY_MOVIES
        }

internal val VesperDownloadPublicCollection.contentUri: Uri
    get() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            error("MediaStore public collection output requires Android 10 or newer")
        }
        return when (this) {
            VesperDownloadPublicCollection.Downloads -> MediaStore.Downloads.EXTERNAL_CONTENT_URI
            VesperDownloadPublicCollection.Movies -> MediaStore.Video.Media.EXTERNAL_CONTENT_URI
        }
    }
