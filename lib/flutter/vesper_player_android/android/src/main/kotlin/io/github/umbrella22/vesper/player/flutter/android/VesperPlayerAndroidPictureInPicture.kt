package io.github.umbrella22.vesper.player.flutter.android

import android.app.Activity
import android.app.PictureInPictureParams
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Rect
import android.os.Build
import android.util.Rational
import android.view.View
import io.github.umbrella22.vesper.player.android.VesperPlayerSurfaceView
import io.github.umbrella22.vesper.player.android.VesperPictureInPictureError
import io.github.umbrella22.vesper.player.android.VesperPictureInPictureErrorCode
import io.github.umbrella22.vesper.player.android.VesperPictureInPictureReadiness
import kotlin.math.roundToInt
import kotlin.math.ceil
import kotlin.math.floor

internal data class FlutterPictureInPictureConfiguration(
    val enabled: Boolean = true,
    val autoEnter: Boolean = false,
    val preferredAspectRatio: Double? = null,
) {
    init {
        require(preferredAspectRatio == null || (preferredAspectRatio.isFinite() && preferredAspectRatio > 0)) {
            "preferredAspectRatio must be finite and positive."
        }
    }
    fun toMap(): Map<String, Any?> =
        mapOf(
            "enabled" to enabled,
            "autoEnter" to autoEnter,
            "preferredAspectRatio" to preferredAspectRatio,
        )
}

internal class PictureInPictureRequestException(
    val pipError: VesperPictureInPictureError,
) : IllegalStateException(pipError.message)

internal fun Map<String, Any?>?.toPictureInPictureConfiguration():
    FlutterPictureInPictureConfiguration {
    if (this == null) {
        return FlutterPictureInPictureConfiguration()
    }
    return FlutterPictureInPictureConfiguration(
        enabled = this["enabled"] as? Boolean ?: true,
        autoEnter = this["autoEnter"] as? Boolean ?: false,
        preferredAspectRatio = (this["preferredAspectRatio"] as? Number)?.toDouble(),
    )
}

internal fun Map<String, Any?>.toPictureInPictureError(): VesperPictureInPictureError =
    VesperPictureInPictureError(
        code = (this["code"] as? String).toPictureInPictureErrorCode(),
        message = this["message"] as? String
            ?: "Current playback cannot enter Picture in Picture.",
        userMessage = this["userMessage"] as? String
            ?: "Current playback cannot enter Picture in Picture.",
        diagnostics = (this["diagnostics"] as? Map<*, *>)?.stringMap() ?: emptyMap(),
    )

internal fun VesperPictureInPictureReadiness.toFlutterMap(
    activity: Activity?,
    platformSupportsPictureInPicture: Boolean,
    hostSupportsPictureInPicture: Boolean,
    isActive: Boolean,
    canAutoEnter: Boolean,
): Map<String, Any?> {
    val platformError =
        when {
            !platformSupportsPictureInPicture ->
                pipError(
                    VesperPictureInPictureErrorCode.PictureInPictureNotSupported,
                    "Android Picture in Picture is not supported on this device.",
                )
            activity == null ->
                pipError(
                    VesperPictureInPictureErrorCode.PictureInPictureDisabledByHost,
                    "No Activity is attached for Picture in Picture.",
                )
            !hostSupportsPictureInPicture ->
                pipError(
                    VesperPictureInPictureErrorCode.PictureInPictureDisabledByHost,
                    "Host Activity has not enabled Picture in Picture.",
                )
            else -> null
        }
    val resolvedError = platformError ?: error
    val available = isAvailable && platformError == null
    return mapOf(
        "isAvailable" to available,
        "isActive" to isActive,
        "canAutoEnter" to canAutoEnter,
        "source" to "system",
        "error" to resolvedError?.toFlutterMap(),
        "diagnostics" to
            diagnostics + mapOf(
                "platform" to "android",
                "platformSupportsPictureInPicture" to platformSupportsPictureInPicture,
                "hostSupportsPictureInPicture" to hostSupportsPictureInPicture,
                "sdkInt" to Build.VERSION.SDK_INT,
            ),
    )
}

internal fun VesperPictureInPictureError.toFlutterMap(): Map<String, Any?> =
    mapOf(
        "code" to code.wireName,
        "message" to message,
        "userMessage" to userMessage,
        "diagnostics" to diagnostics,
    )

internal fun Throwable.toPictureInPictureErrorMap(): Map<String, Any?> =
    when (this) {
        is PictureInPictureRequestException -> pipError.toFlutterMap()
        else -> toPictureInPictureRequestError().toFlutterMap()
    }

internal fun Throwable.toPictureInPictureRequestError(): VesperPictureInPictureError {
    val rawMessage = message ?: "Android rejected Picture in Picture request."
    val code =
        if (rawMessage.contains("picture-in-picture", ignoreCase = true) ||
            rawMessage.contains("picture in picture", ignoreCase = true)
        ) {
            VesperPictureInPictureErrorCode.PictureInPictureDisabledByHost
        } else {
            VesperPictureInPictureErrorCode.PictureInPicturePlatformRequestRejected
        }
    return VesperPictureInPictureError(
        code = code,
        message = rawMessage,
        diagnostics = mapOf("exception" to this::class.java.name),
    )
}

internal fun PlayerSession.pictureInPictureEventMap(
    state: String = pictureInPictureState,
    error: VesperPictureInPictureError? = null,
    diagnostics: Map<String, Any?> = emptyMap(),
): Map<String, Any?> =
    mapOf(
        "playerId" to id,
        "type" to "pictureInPicture",
        "state" to state,
        "isActive" to pictureInPictureActive,
        "source" to "system",
        "canAutoEnter" to
            (pictureInPictureConfiguration.enabled && pictureInPictureConfiguration.autoEnter),
        "error" to error?.toFlutterMap(),
        "diagnostics" to diagnostics,
    )

internal fun Activity.supportsPictureInPicture(): Boolean {
    if (!platformSupportsPictureInPicture()) {
        return false
    }
    return runCatching {
        val activityInfo =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                packageManager.getActivityInfo(
                    componentName,
                    PackageManager.ComponentInfoFlags.of(0),
                )
            } else {
                @Suppress("DEPRECATION")
                packageManager.getActivityInfo(componentName, 0)
            }
        val field = activityInfo::class.java.getField("supportsPictureInPicture")
        field.getBoolean(activityInfo)
    }.getOrDefault(true)
}

internal fun Activity.platformSupportsPictureInPicture(): Boolean {
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
        packageManager.hasSystemFeature(PackageManager.FEATURE_PICTURE_IN_PICTURE)
}

internal fun Activity.requestPictureInPictureForegroundRestore(): Boolean {
    val launchIntent = packageManager.getLaunchIntentForPackage(packageName)
    val restoreIntent =
        launchIntent ?: Intent(this, this::class.java)
    restoreIntent.addFlags(Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
    restoreIntent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
    startActivity(restoreIntent)
    return true
}

internal fun PlayerSession.buildPictureInPictureParams(): PictureInPictureParams {
    val builder = PictureInPictureParams.Builder()
    val ratio = resolvePictureInPictureAspectRatio(
        pictureInPictureConfiguration.preferredAspectRatio,
        controller.videoPresentation?.value?.displayAspectRatio,
        viewport?.let { it.width / it.height },
        hostView?.let { it.width.toDouble() / it.height },
    )
    builder.setAspectRatio(ratio.toRational())
    hostView?.sourceRectHint()?.let(builder::setSourceRectHint)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        builder.setAutoEnterEnabled(
            pictureInPictureConfiguration.enabled && pictureInPictureConfiguration.autoEnter,
        )
    }
    return builder.build()
}

internal fun resolvePictureInPictureAspectRatio(
    preferred: Double?, display: Double?, viewport: Double?, host: Double?,
): Double = listOf(preferred, display, viewport, host)
    .firstOrNull { it != null && it.isFinite() && it > 0 } ?: (16.0 / 9.0)

private fun Double.toRational(): Rational {
    val (numerator, denominator) = pictureInPictureRatioFraction(this)
    return Rational(numerator, denominator)
}

internal fun pictureInPictureRatioFraction(ratio: Double): Pair<Int, Int> {
    val minimum = 100.0 / 239.0
    val maximum = 239.0 / 100.0
    if (ratio <= minimum) return 100 to 239
    if (ratio >= maximum) return 239 to 100
    val denominator = 10_000
    // Quantization must stay inside Android's accepted interval at both ends.
    val numerator = (ratio * denominator).roundToInt().coerceIn(
        ceil(minimum * denominator).toInt(), floor(maximum * denominator).toInt(),
    )
    return numerator to denominator
}

private fun View.sourceRectHint(): Rect? {
    if (width <= 0 || height <= 0 || !isAttachedToWindow) {
        return null
    }
    val location = IntArray(2)
    getLocationOnScreen(location)
    val content = (this as? VesperPlayerSurfaceView)?.geometry?.value?.contentRect
    if (content != null) {
        val density = resources.displayMetrics.density
        return Rect(
            location[0] + (content.left * density).roundToInt(),
            location[1] + (content.top * density).roundToInt(),
            location[0] + ((content.left + content.width) * density).roundToInt(),
            location[1] + ((content.top + content.height) * density).roundToInt(),
        )
    }
    return Rect(
        location[0],
        location[1],
        location[0] + width,
        location[1] + height,
    )
}

private fun pipError(
    code: VesperPictureInPictureErrorCode,
    message: String,
): VesperPictureInPictureError =
    VesperPictureInPictureError(
        code = code,
        message = message,
    )

private val VesperPictureInPictureErrorCode.wireName: String
    get() = when (this) {
        VesperPictureInPictureErrorCode.PictureInPictureNotSupported ->
            "pictureInPictureNotSupported"
        VesperPictureInPictureErrorCode.PictureInPictureDisabledByHost ->
            "pictureInPictureDisabledByHost"
        VesperPictureInPictureErrorCode.PictureInPictureSystemPlayerUnavailable ->
            "pictureInPictureSystemPlayerUnavailable"
        VesperPictureInPictureErrorCode.PictureInPictureSourceUnsupportedBySystemPlayer ->
            "pictureInPictureSourceUnsupportedBySystemPlayer"
        VesperPictureInPictureErrorCode.PictureInPictureNativeFrameRouteCannotHandOff ->
            "pictureInPictureNativeFrameRouteCannotHandOff"
        VesperPictureInPictureErrorCode.PictureInPictureSurfaceUnavailable ->
            "pictureInPictureSurfaceUnavailable"
        VesperPictureInPictureErrorCode.PictureInPicturePlatformRequestRejected ->
            "pictureInPicturePlatformRequestRejected"
        VesperPictureInPictureErrorCode.PictureInPictureUnavailableForCurrentRoute ->
            "pictureInPictureUnavailableForCurrentRoute"
    }

private fun String?.toPictureInPictureErrorCode(): VesperPictureInPictureErrorCode =
    when (this) {
        "pictureInPictureNotSupported" ->
            VesperPictureInPictureErrorCode.PictureInPictureNotSupported
        "pictureInPictureDisabledByHost" ->
            VesperPictureInPictureErrorCode.PictureInPictureDisabledByHost
        "pictureInPictureSystemPlayerUnavailable" ->
            VesperPictureInPictureErrorCode.PictureInPictureSystemPlayerUnavailable
        "pictureInPictureSourceUnsupportedBySystemPlayer" ->
            VesperPictureInPictureErrorCode.PictureInPictureSourceUnsupportedBySystemPlayer
        "pictureInPictureNativeFrameRouteCannotHandOff" ->
            VesperPictureInPictureErrorCode.PictureInPictureNativeFrameRouteCannotHandOff
        "pictureInPictureSurfaceUnavailable" ->
            VesperPictureInPictureErrorCode.PictureInPictureSurfaceUnavailable
        "pictureInPicturePlatformRequestRejected" ->
            VesperPictureInPictureErrorCode.PictureInPicturePlatformRequestRejected
        else ->
            VesperPictureInPictureErrorCode.PictureInPictureUnavailableForCurrentRoute
    }
