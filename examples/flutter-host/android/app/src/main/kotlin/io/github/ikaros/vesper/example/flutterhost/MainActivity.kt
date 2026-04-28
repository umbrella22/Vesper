package io.github.ikaros.vesper.example.flutterhost

import android.content.Context
import android.content.Intent
import android.media.AudioManager
import android.net.Uri
import android.provider.Settings
import android.provider.OpenableColumns
import dalvik.system.BaseDexClassLoader
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import kotlin.math.roundToInt

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
        "bundledDownloadPluginLibraryPaths" -> result.success(bundledDownloadPluginLibraryPaths())
        "saveVideoToGallery" -> saveVideoToGallery(call, result)
        else -> result.notImplemented()
      }
    }
    MethodChannel(
      flutterEngine.dartExecutor.binaryMessenger,
      DEVICE_CONTROLS_CHANNEL,
    ).setMethodCallHandler { call, result ->
      when (call.method) {
        "getBrightness" -> result.success(currentBrightnessRatio())
        "setBrightness" -> setBrightnessRatio(call, result)
        "getVolume" -> result.success(currentVolumeRatio())
        "setVolume" -> setVolumeRatio(call, result)
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

  private fun bundledDownloadPluginLibraryPaths(): List<String> {
    val libraryName = "player_remux_ffmpeg"
    val resolvedPath =
      (classLoader as? BaseDexClassLoader)?.findLibrary(libraryName)?.takeIf { path ->
        path.isNotBlank() && File(path).isFile
      }
        ?: run {
          val nativeLibraryDir = applicationInfo.nativeLibraryDir
          val pluginLibrary =
            nativeLibraryDir?.let { directory ->
              File(directory, System.mapLibraryName(libraryName))
            }
          pluginLibrary?.takeIf(File::isFile)?.absolutePath
        }
    return resolvedPath?.let(::listOf) ?: emptyList()
  }

  private fun saveVideoToGallery(call: io.flutter.plugin.common.MethodCall, result: MethodChannel.Result) {
    val completedPath = call.argument<String>("completedPath")?.trim()
    if (completedPath.isNullOrEmpty()) {
      result.error("invalid_argument", "The completed download output is unavailable.", null)
      return
    }

    Thread {
      runCatching {
        saveVideoToGallery(this, completedPath)
      }.fold(
        onSuccess = {
          runOnUiThread {
            result.success(null)
          }
        },
        onFailure = { error ->
          runOnUiThread {
            result.error("save_failed", error.message, null)
          }
        },
      )
    }.start()
  }

  private fun currentBrightnessRatio(): Double {
    val windowBrightness = window.attributes.screenBrightness
    if (windowBrightness >= 0f) {
      return windowBrightness.toDouble().coerceIn(0.0, 1.0)
    }
    return runCatching {
      Settings.System.getInt(contentResolver, Settings.System.SCREEN_BRIGHTNESS) / 255.0
    }.getOrDefault(0.5).coerceIn(0.0, 1.0)
  }

  private fun setBrightnessRatio(call: io.flutter.plugin.common.MethodCall, result: MethodChannel.Result) {
    val ratio = call.argument<Double>("ratio")
    if (ratio == null) {
      result.error("invalid_argument", "Missing brightness ratio.", null)
      return
    }
    val nextRatio = ratio.coerceIn(0.02, 1.0).toFloat()
    val attributes = window.attributes
    attributes.screenBrightness = nextRatio
    window.attributes = attributes
    result.success(nextRatio.toDouble())
  }

  private fun currentVolumeRatio(): Double? {
    val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return null
    val maxVolume = audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    if (maxVolume <= 0) {
      return null
    }
    return (audioManager.getStreamVolume(AudioManager.STREAM_MUSIC).toDouble() / maxVolume)
      .coerceIn(0.0, 1.0)
  }

  private fun setVolumeRatio(call: io.flutter.plugin.common.MethodCall, result: MethodChannel.Result) {
    val ratio = call.argument<Double>("ratio")
    if (ratio == null) {
      result.error("invalid_argument", "Missing volume ratio.", null)
      return
    }
    val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager
    if (audioManager == null) {
      result.success(null)
      return
    }
    val maxVolume = audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
    if (maxVolume <= 0) {
      result.success(null)
      return
    }
    val nextVolume = (ratio.coerceIn(0.0, 1.0) * maxVolume).roundToInt().coerceIn(0, maxVolume)
    runCatching {
      audioManager.setStreamVolume(AudioManager.STREAM_MUSIC, nextVolume, 0)
      result.success(
        (audioManager.getStreamVolume(AudioManager.STREAM_MUSIC).toDouble() / maxVolume)
          .coerceIn(0.0, 1.0),
      )
    }.onFailure { error ->
      result.error("volume_failed", error.message, null)
    }
  }

  companion object {
    private const val REQUEST_PICK_VIDEO = 1042
    private const val MEDIA_PICKER_CHANNEL =
      "io.github.ikaros.vesper.example.flutter_host/media_picker"
    private const val DEVICE_CONTROLS_CHANNEL =
      "io.github.ikaros.vesper.example.flutter_host/device_controls"
  }
}
