package io.github.ikaros.vesper.player.flutter.android

import android.app.Activity
import android.app.PictureInPictureParams
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Rect
import android.os.Build
import android.util.Rational
import android.view.View
import io.github.ikaros.vesper.player.android.VesperPictureInPictureError
import io.github.ikaros.vesper.player.android.VesperPictureInPictureErrorCode
import io.github.ikaros.vesper.player.android.VesperPictureInPictureReadiness
import kotlin.math.roundToInt

internal data class FlutterPictureInPictureConfiguration(
    val enabled: Boolean = true,
    val autoEnter: Boolean = false,
    val preferredAspectRatio: Double? = null,
) {
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
    val ratio = pictureInPictureConfiguration.preferredAspectRatio ?: inferredAspectRatio()
    builder.setAspectRatio(ratio.toRational())
    hostView?.sourceRectHint()?.let(builder::setSourceRectHint)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        builder.setAutoEnterEnabled(
            pictureInPictureConfiguration.enabled && pictureInPictureConfiguration.autoEnter,
        )
    }
    return builder.build()
}

private fun PlayerSession.inferredAspectRatio(): Double {
    val viewport = viewport
    if (viewport != null && viewport.width > 0.0 && viewport.height > 0.0) {
        return viewport.width / viewport.height
    }
    val host = hostView
    if (host != null && host.width > 0 && host.height > 0) {
        return host.width.toDouble() / host.height.toDouble()
    }
    return 16.0 / 9.0
}

private fun Double.toRational(): Rational {
    val clamped = coerceIn(0.418410, 2.390000)
    val denominator = 10_000
    val numerator = (clamped * denominator).roundToInt().coerceAtLeast(1)
    return Rational(numerator, denominator)
}

private fun View.sourceRectHint(): Rect? {
    if (width <= 0 || height <= 0 || !isAttachedToWindow) {
        return null
    }
    val location = IntArray(2)
    getLocationOnScreen(location)
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
