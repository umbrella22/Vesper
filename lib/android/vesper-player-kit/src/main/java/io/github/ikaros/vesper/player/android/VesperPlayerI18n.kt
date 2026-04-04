package io.github.ikaros.vesper.player.android

import android.content.Context
import androidx.annotation.StringRes
import java.util.Locale

private fun interface VesperStringResolver {
    fun resolve(@StringRes resId: Int, args: Array<out Any>): String
}

internal class VesperPlayerI18n private constructor(
    private val resolver: VesperStringResolver,
) {
    fun playerTitle(): String = string(R.string.vesper_player_title)

    fun nativeBridgeReady(): String = string(R.string.vesper_bridge_native_ready)

    fun previewBridgeReady(): String = string(R.string.vesper_bridge_preview_ready)

    fun noSourceSelected(): String = string(R.string.vesper_bridge_no_source_selected)

    fun selectSourcePrompt(): String = string(R.string.vesper_bridge_select_source_prompt)

    fun stubError(message: String): String = string(R.string.vesper_bridge_stub_error, message)

    fun nativeBindingsUnavailable(): String =
        string(R.string.vesper_bridge_native_bindings_unavailable)

    fun openingSource(sourceLabel: String): String =
        string(R.string.vesper_bridge_opening_source, sourceLabel)

    fun sourceSubtitle(source: VesperPlayerSource): String =
        when (source.kind) {
            VesperPlayerSourceKind.Local -> string(R.string.vesper_bridge_source_local)
            VesperPlayerSourceKind.Remote ->
                string(
                    R.string.vesper_bridge_source_remote,
                    source.protocol.name.lowercase(Locale.ROOT),
                )
        }

    fun previewSourceSubtitle(source: VesperPlayerSource): String =
        when (source.kind) {
            VesperPlayerSourceKind.Local -> string(R.string.vesper_bridge_preview_source_local)
            VesperPlayerSourceKind.Remote ->
                string(
                    R.string.vesper_bridge_preview_source_remote,
                    source.protocol.name.lowercase(Locale.ROOT),
                )
        }

    fun surfaceAttached(sourceSubtitle: String?): String =
        if (sourceSubtitle != null) {
            string(R.string.vesper_bridge_source_surface_attached, sourceSubtitle)
        } else {
            string(R.string.vesper_bridge_surface_attached_no_source)
        }

    fun surfaceDetached(sourceSubtitle: String?): String =
        if (sourceSubtitle != null) {
            string(R.string.vesper_bridge_source_surface_detached, sourceSubtitle)
        } else {
            string(R.string.vesper_bridge_surface_detached_no_source)
        }

    fun retryScheduled(delay: String, attempt: Int): String =
        string(R.string.vesper_bridge_retry_scheduled, delay, attempt)

    fun nativeError(message: String): String =
        string(R.string.vesper_bridge_native_error, message)

    fun retryDelay(delayMs: Long): String {
        val seconds = delayMs.toDouble() / 1_000.0
        return if (seconds >= 10.0 || seconds == seconds.toInt().toDouble()) {
            string(R.string.vesper_bridge_retry_delay_seconds_int, seconds.toInt())
        } else {
            string(R.string.vesper_bridge_retry_delay_seconds_decimal, seconds)
        }
    }

    private fun string(@StringRes resId: Int, vararg args: Any): String = resolver.resolve(resId, args)

    companion object {
        fun fromContext(context: Context?): VesperPlayerI18n =
            if (context == null) {
                VesperPlayerI18n(VesperStringResolver(::resolveFallback))
            } else {
                VesperPlayerI18n(VesperStringResolver { resId, args ->
                    if (args.isEmpty()) {
                        context.getString(resId)
                    } else {
                        context.getString(resId, *args)
                    }
                })
            }

        private fun resolveFallback(@StringRes resId: Int, args: Array<out Any>): String {
            val format =
                when (resId) {
                    R.string.vesper_player_title -> "Vesper"
                    R.string.vesper_bridge_native_ready -> "Android JNI/ExoPlayer bridge"
                    R.string.vesper_bridge_preview_ready -> "Android host preview bridge"
                    R.string.vesper_bridge_no_source_selected -> "No source selected"
                    R.string.vesper_bridge_select_source_prompt ->
                        "Select a media source to begin playback"
                    R.string.vesper_bridge_stub_error -> "Android JNI bridge stub: %1\$s"
                    R.string.vesper_bridge_native_bindings_unavailable ->
                        "native bindings unavailable"
                    R.string.vesper_bridge_opening_source -> "Opening %1\$s"
                    R.string.vesper_bridge_source_local ->
                        "Android JNI + ExoPlayer ready (local source)"
                    R.string.vesper_bridge_source_remote ->
                        "Android JNI + ExoPlayer ready (%1\$s remote source)"
                    R.string.vesper_bridge_source_surface_attached -> "%1\$s / surface attached"
                    R.string.vesper_bridge_source_surface_detached -> "%1\$s / surface detached"
                    R.string.vesper_bridge_surface_attached_no_source ->
                        "Android JNI + ExoPlayer ready / surface attached"
                    R.string.vesper_bridge_surface_detached_no_source ->
                        "Android JNI + ExoPlayer ready / surface detached"
                    R.string.vesper_bridge_retry_scheduled ->
                        "Retrying in %1\$s (attempt %2\$d)"
                    R.string.vesper_bridge_native_error -> "Android native bridge error: %1\$s"
                    R.string.vesper_bridge_preview_source_local ->
                        "Android host preview bridge (local source)"
                    R.string.vesper_bridge_preview_source_remote ->
                        "Android host preview bridge (%1\$s remote source)"
                    R.string.vesper_bridge_retry_delay_seconds_int -> "%1\$ds"
                    R.string.vesper_bridge_retry_delay_seconds_decimal -> "%1\$.1fs"
                    else -> "missing-string($resId)"
                }
            return if (args.isEmpty()) {
                format
            } else {
                String.format(Locale.ENGLISH, format, *args)
            }
        }
    }
}
