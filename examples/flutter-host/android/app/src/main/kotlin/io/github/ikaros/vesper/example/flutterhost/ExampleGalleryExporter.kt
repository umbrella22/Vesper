package io.github.ikaros.vesper.example.flutterhost

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import java.io.File

internal fun saveVideoToGallery(
  context: Context,
  completedPath: String,
): Uri {
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
    return targetUri
  } catch (error: Throwable) {
    resolver.delete(targetUri, null, null)
    throw error
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
  val extension = file.extension.lowercase().takeIf { it.isNotBlank() } ?: return "video/mp4"
  return MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) ?: "video/mp4"
}
