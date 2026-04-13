package io.github.ikaros.vesper.example.flutterhost

import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
  private var pendingPickerResult: MethodChannel.Result? = null

  override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
    super.configureFlutterEngine(flutterEngine)
    MethodChannel(
      flutterEngine.dartExecutor.binaryMessenger,
      MEDIA_PICKER_CHANNEL,
    ).setMethodCallHandler { call, result ->
      when (call.method) {
        "pickVideo" -> launchVideoPicker(result)
        else -> result.notImplemented()
      }
    }
  }

  private fun launchVideoPicker(result: MethodChannel.Result) {
    if (pendingPickerResult != null) {
      result.error("busy", "A media picker request is already active.", null)
      return
    }

    pendingPickerResult = result
    try {
      val intent =
        Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
          addCategory(Intent.CATEGORY_OPENABLE)
          type = "video/*"
          putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("video/*"))
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
          addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }
      startActivityForResult(intent, REQUEST_PICK_VIDEO)
    } catch (error: Throwable) {
      pendingPickerResult = null
      result.error("picker_unavailable", error.message, null)
    }
  }

  @Deprecated("Deprecated in Java")
  override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
    super.onActivityResult(requestCode, resultCode, data)
    if (requestCode != REQUEST_PICK_VIDEO) {
      return
    }

    val result = pendingPickerResult ?: return
    pendingPickerResult = null

    if (resultCode != RESULT_OK) {
      result.success(null)
      return
    }

    val uri = data?.data
    if (uri == null) {
      result.success(null)
      return
    }

    try {
      contentResolver.takePersistableUriPermission(
        uri,
        Intent.FLAG_GRANT_READ_URI_PERMISSION,
      )
    } catch (_: SecurityException) {
    } catch (_: IllegalArgumentException) {
    }

    result.success(
      mapOf(
        "uri" to uri.toString(),
        "label" to displayNameForUri(uri),
      ),
    )
  }

  private fun displayNameForUri(uri: Uri): String {
    val fallback = uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
    val projection = arrayOf(OpenableColumns.DISPLAY_NAME)
    contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
      if (index >= 0 && cursor.moveToFirst()) {
        val value = cursor.getString(index)
        if (!value.isNullOrBlank()) {
          return value
        }
      }
    }
    return fallback ?: "本地视频"
  }

  companion object {
    private const val REQUEST_PICK_VIDEO = 1042
    private const val MEDIA_PICKER_CHANNEL =
      "io.github.ikaros.vesper.example.flutter_host/media_picker"
  }
}
