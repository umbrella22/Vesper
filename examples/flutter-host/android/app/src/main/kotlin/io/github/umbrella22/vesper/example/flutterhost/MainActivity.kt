package io.github.umbrella22.vesper.example.flutterhost

import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.Canvas
import android.hardware.display.DisplayManager
import android.media.AudioManager
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.provider.OpenableColumns
import android.view.Display
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import io.github.umbrella22.vesper.player.flutter.android.VesperPlayerAndroidPlugin
import java.io.ByteArrayOutputStream
import java.io.File
import kotlin.math.roundToInt

class MainActivity : FlutterFragmentActivity() {
  private var pendingPickerResult: MethodChannel.Result? = null
  private var pictureInPictureHostChannel: MethodChannel? = null
  private val videoPickerLauncher =
    registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
      completeVideoPicker(uri)
    }

  override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
    super.configureFlutterEngine(flutterEngine)
    pictureInPictureHostChannel =
      MethodChannel(
        flutterEngine.dartExecutor.binaryMessenger,
        PICTURE_IN_PICTURE_CHANNEL,
      )
    MethodChannel(
      flutterEngine.dartExecutor.binaryMessenger,
      MEDIA_PICKER_CHANNEL,
    ).setMethodCallHandler { call, result ->
      when (call.method) {
        "pickVideo" -> launchVideoPicker(result)
        "saveVideoToGallery" -> saveVideoToGallery(call, result)
        "hdrEvidenceOutputRoot" -> result.success(hdrEvidenceOutputRoot().absolutePath)
        "hdrEvidenceDevice" -> result.success(hdrEvidenceDevice())
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
        "subtitleOverlaySnapshot" ->
          handleSubtitleOverlayEvidence(call, captureImage = false, result = result)
        "captureSubtitleOverlayEvidence" ->
          handleSubtitleOverlayEvidence(call, captureImage = true, result = result)
        else -> result.notImplemented()
      }
    }
  }

  override fun onPictureInPictureModeChanged(
    isInPictureInPictureMode: Boolean,
    newConfig: Configuration,
  ) {
    super.onPictureInPictureModeChanged(isInPictureInPictureMode, newConfig)
    VesperPlayerAndroidPlugin.dispatchPictureInPictureModeChanged(
      this,
      isInPictureInPictureMode,
    )
  }

  override fun onUserLeaveHint() {
    VesperPlayerAndroidPlugin.dispatchPictureInPictureUserLeaveHint(this)
    pictureInPictureHostChannel?.invokeMethod("onUserLeaveHint", null)
    super.onUserLeaveHint()
  }

  private fun launchVideoPicker(result: MethodChannel.Result) {
    if (pendingPickerResult != null) {
      result.error("busy", "A media picker request is already active.", null)
      return
    }

    pendingPickerResult = result
    try {
      videoPickerLauncher.launch(arrayOf("video/*"))
    } catch (error: Throwable) {
      pendingPickerResult = null
      result.error("picker_unavailable", error.message, null)
    }
  }

  private fun completeVideoPicker(uri: Uri?) {
    val result = pendingPickerResult ?: return
    pendingPickerResult = null

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

  private fun hdrEvidenceOutputRoot(): File {
    val root =
      getExternalFilesDir(null)?.let { File(it, "hdr-dv-evidence") }
        ?: File(filesDir, "hdr-dv-evidence")
    if (!root.exists()) {
      root.mkdirs()
    }
    return root
  }

  private fun hdrEvidenceDevice(): Map<String, Any?> {
    val display =
      requireNotNull(
        getSystemService(DisplayManager::class.java)?.getDisplay(Display.DEFAULT_DISPLAY),
      ) { "The default Android display is unavailable." }
    return mapOf(
      "android" to mapOf(
        "manufacturer" to Build.MANUFACTURER,
        "model" to Build.MODEL,
        "apiLevel" to Build.VERSION.SDK_INT,
        "buildFingerprint" to Build.FINGERPRINT,
        "displayHdrTypes" to hdrTypeNames(display),
        "displayRefreshRate" to display.refreshRate.toDouble(),
        "displayModes" to display.supportedModes.map { mode ->
          "${mode.physicalWidth}x${mode.physicalHeight}@${"%.2f".format(mode.refreshRate)}"
        },
        "media3Version" to "1.11.0",
        "decoderCandidates" to mapOf(
          "hevc" to decoderCandidates("video/hevc"),
          "dolbyVision" to decoderCandidates("video/dolby-vision"),
        ),
      ),
    )
  }

  private fun hdrTypeNames(display: Display): List<String> {
    val supportedHdrTypes =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        display.mode.supportedHdrTypes
      } else {
        display.legacySupportedHdrTypes()
      }
    return supportedHdrTypes.map { type ->
      when (type) {
        Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION -> "DOLBY_VISION"
        Display.HdrCapabilities.HDR_TYPE_HDR10 -> "HDR10"
        Display.HdrCapabilities.HDR_TYPE_HLG -> "HLG"
        Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS -> "HDR10_PLUS"
        else -> "UNKNOWN_$type"
      }
    }
  }

  @Suppress("DEPRECATION")
  private fun Display.legacySupportedHdrTypes(): IntArray = hdrCapabilities.supportedHdrTypes

  private fun decoderCandidates(mimeType: String): List<String> {
    return runCatching {
      MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
        .filter { codecInfo ->
          !codecInfo.isEncoder &&
            codecInfo.supportedTypes.any { type -> type.equals(mimeType, ignoreCase = true) } &&
            codecInfo.isHardwareAcceleratedCompat()
        }
        .map(MediaCodecInfo::getName)
        .distinct()
    }.getOrDefault(emptyList())
  }

  private fun MediaCodecInfo.isHardwareAcceleratedCompat(): Boolean {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      isHardwareAccelerated
    } else {
      val lowerName = name.lowercase()
      !lowerName.startsWith("omx.google.") &&
        !lowerName.startsWith("c2.android.") &&
        !lowerName.contains("software")
    }
  }

  private fun handleSubtitleOverlayEvidence(
    call: io.flutter.plugin.common.MethodCall,
    captureImage: Boolean,
    result: MethodChannel.Result,
  ) {
    val playerId = call.argument<String>("playerId")?.takeIf(String::isNotBlank)
    if (playerId == null) {
      result.error(
        "invalid_argument",
        "Missing playerId for subtitle overlay evidence.",
        null,
      )
      return
    }

    val surfaceIdentifier = "$PLAYER_SURFACE_TAG_PREFIX$playerId"
    val surface =
      descendantView(window.decorView) { view -> view.tag == surfaceIdentifier } as? ViewGroup
    val subtitleLabel =
      surface?.let { host ->
        descendantView(host) { view -> view.tag == SUBTITLE_OVERLAY_TAG } as? TextView
      }
    if (surface == null || subtitleLabel == null) {
      result.error(
        "subtitle_overlay_unavailable",
        "Unable to locate the subtitle overlay for playerId=$playerId.",
        mapOf("surfaceIdentifier" to surfaceIdentifier),
      )
      return
    }

    val snapshot = subtitleOverlaySnapshot(subtitleLabel)
    if (!captureImage) {
      result.success(snapshot)
      return
    }
    if (surface.width <= 0 || surface.height <= 0) {
      result.error(
        "subtitle_overlay_unavailable",
        "The subtitle surface has an empty frame.",
        snapshot,
      )
      return
    }

    val png =
      try {
        captureViewPng(surface)
      } catch (error: Throwable) {
        result.error(
          "subtitle_overlay_capture_failed",
          error.message ?: "Android could not capture the subtitle surface.",
          snapshot,
        )
        return
      }
    result.success(mapOf("snapshot" to snapshot, "png" to png))
  }

  private fun descendantView(view: View, predicate: (View) -> Boolean): View? {
    if (predicate(view)) {
      return view
    }
    val group = view as? ViewGroup ?: return null
    for (index in 0 until group.childCount) {
      descendantView(group.getChildAt(index), predicate)?.let { match -> return match }
    }
    return null
  }

  private fun subtitleOverlaySnapshot(label: TextView): Map<String, Any> {
    val text = label.text?.toString().orEmpty()
    val hidden = label.visibility != View.VISIBLE
    val alpha = label.alpha.toDouble()
    val windowAttached = label.isAttachedToWindow && label.windowToken != null
    val visible =
      text.isNotEmpty() && !hidden && alpha > 0.0 && windowAttached &&
        label.width > 0 && label.height > 0 && label.isShown
    return mapOf(
      "text" to text,
      "hidden" to hidden,
      "alpha" to alpha,
      "windowAttached" to windowAttached,
      "frame" to
        mapOf(
          "x" to label.x.toDouble(),
          "y" to label.y.toDouble(),
          "width" to label.width.toDouble(),
          "height" to label.height.toDouble(),
        ),
      "visible" to visible,
    )
  }

  private fun captureViewPng(view: View): ByteArray {
    val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
    return try {
      view.draw(Canvas(bitmap))
      ByteArrayOutputStream().use { output ->
        check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) {
          "Android failed to encode the subtitle surface as PNG."
        }
        output.toByteArray()
      }
    } finally {
      bitmap.recycle()
    }
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
    private const val MEDIA_PICKER_CHANNEL =
      "io.github.umbrella22.vesper.example.flutter_host/media_picker"
    private const val DEVICE_CONTROLS_CHANNEL =
      "io.github.umbrella22.vesper.example.flutter_host/device_controls"
    private const val PLAYER_SURFACE_TAG_PREFIX =
      "io.github.umbrella22.vesper.player.surface."
    private const val SUBTITLE_OVERLAY_TAG =
      "io.github.umbrella22.vesper.player.subtitle-overlay"
    private const val PICTURE_IN_PICTURE_CHANNEL =
      "io.github.umbrella22.vesper.example.flutter_host/picture_in_picture"
  }
}
