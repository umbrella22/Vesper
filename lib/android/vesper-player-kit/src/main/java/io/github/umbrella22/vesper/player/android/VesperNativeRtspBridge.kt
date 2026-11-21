package io.github.umbrella22.vesper.player.android

import android.security.NetworkSecurityPolicy
import android.util.Log
import java.io.Closeable
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONObject

internal object VesperNativeRtspBridgeJni {
    @JvmStatic external fun open(sourceUri: String, loopbackCleartextPermitted: Boolean): String
    @JvmStatic external fun status(handle: Long): String
    @JvmStatic external fun close(handle: Long)
}

internal class NativeRtspBridge private constructor(
    handle: Long,
    val playlistUri: String,
) : Closeable {
    private val nativeHandle = AtomicLong(handle)

    suspend fun awaitReady() {
        val ready = withTimeoutOrNull(20_000L) {
            while (true) {
                val status = JSONObject(VesperNativeRtspBridgeJni.status(nativeHandle.get()))
                when (val phase = status.getString("phase")) {
                    "ready" -> {
                        if (status.optBoolean("audioOmitted")) {
                            Log.w(NATIVE_JNI_BINDINGS_TAG, "rtsp_bridge_video_only: the origin advertises an audio track")
                        }
                        return@withTimeoutOrNull true
                    }
                    "connecting", "reconnecting" -> delay(75L)
                    "failed" -> throw status.failure()
                    else -> error("Unrecognized RTSP bridge phase: $phase")
                }
            }
        }
        if (ready == null) {
            throw rtspBridgeFailure("rtsp_origin_timeout", "Timed out waiting for RTSP video fragments.")
        }
    }

    override fun close() {
        val handle = nativeHandle.getAndSet(0L)
        if (handle != 0L) VesperNativeRtspBridgeJni.close(handle)
    }

    companion object {
        fun open(source: VesperPlayerSource): NativeRtspBridge {
            if (source.headers.isNotEmpty() || source.drmConfiguration != null) {
                throw rtspBridgeFailure(
                    "rtsp_unsupported_input",
                    "The RTSP bridge requires a source without HTTP headers or DRM.",
                )
            }
            VesperNativeLibrary.ensureLoaded()
            val cleartextPermitted = NetworkSecurityPolicy.getInstance().isCleartextTrafficPermitted("127.0.0.1")
            val opened = JSONObject(VesperNativeRtspBridgeJni.open(source.uri, cleartextPermitted))
            if (opened.optString("phase") == "failed") throw opened.failure()
            val handle = opened.getLong("handle")
            check(handle != 0L)
            try {
                return NativeRtspBridge(handle, opened.getString("playlistUri"))
            } catch (error: Throwable) {
                VesperNativeRtspBridgeJni.close(handle)
                throw error
            }
        }
    }
}

private fun JSONObject.failure(): RuntimeException =
    rtspBridgeFailure(getString("code"), getString("message"))

internal fun rtspBridgeFailure(code: String, message: String): RuntimeException {
    val details = mapOf("reason" to code, "route" to "rtspBridge", "protocol" to "rtsp")
    val (errorCode, category, retriable) = when (code) {
        "rtsp_origin_timeout" -> Triple(VesperPlayerErrorCode.Timeout, VesperPlayerErrorCategory.Network, true)
        "rtsp_origin_unavailable" -> Triple(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCategory.Network, true)
        "rtsp_invalid_url", "rtsp_invalid_response" ->
            Triple(VesperPlayerErrorCode.InvalidSource, VesperPlayerErrorCategory.Source, false)
        "rtsp_bridge_local_http" -> Triple(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCategory.Platform, false)
        "rtsp_bridge_busy" -> Triple(VesperPlayerErrorCode.InvalidState, VesperPlayerErrorCategory.Platform, true)
        else -> return VesperPlayerUnsupportedOperation(message, details)
    }
    return VesperPlayerCommandException(VesperPlayerErrorState(message, errorCode, category, retriable, details))
}
