package io.github.ikaros.vesper.player.android.relay.ffmpeg

import android.content.Context
import io.github.ikaros.vesper.player.android.relay.VesperRelayAdaptedStream
import io.github.ikaros.vesper.player.android.relay.VesperRelayDiagnostic
import io.github.ikaros.vesper.player.android.relay.VesperRelayFallbackFormat
import io.github.ikaros.vesper.player.android.relay.VesperRelayFormatAdaptationRequest
import io.github.ikaros.vesper.player.android.relay.VesperRelayFormatAdaptationResult
import io.github.ikaros.vesper.player.android.relay.VesperRelayFormatAdapter
import java.io.InputStream
import org.json.JSONObject

class VesperRelayFfmpegAdapter @JvmOverloads constructor(
    context: Context? = null,
) : VesperRelayFormatAdapter {
    private val runtimeProfileHash = context?.applicationContext?.readRuntimeProfileHash()

    init {
        VesperRelayFfmpegNative.ensureLoaded()
    }

    override val profileHash: String?
        get() = runCatching {
            JSONObject(VesperRelayFfmpegNative.runtimeMetadata()).optString("profileHash")
                .takeIf { it.isNotBlank() }
        }.getOrNull()

    override fun open(
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayFormatAdaptationResult {
        val pluginProfileHash = profileHash
        val runtimeHash = runtimeProfileHash
        if (!runtimeHash.isNullOrBlank() && runtimeHash != pluginProfileHash) {
            return VesperRelayFormatAdaptationResult.Failure(
                status = 500,
                diagnostic = VesperRelayDiagnostic(
                    code = "profile_mismatch",
                    message = "FFmpeg relay runtime profile does not match the relay JNI profile.",
                    details = request.baseDiagnosticDetails() + mapOf(
                        "runtimeProfileHash" to runtimeHash,
                        "pluginProfileHash" to (pluginProfileHash ?: "unknown"),
                    ),
                ),
            )
        }

        val nativeRequest = request.toNativeJson()
        val opened = runCatching {
            VesperRelayFfmpegNative.open(nativeRequest)
        }.getOrElse { error ->
            return VesperRelayFormatAdaptationResult.Failure(
                status = 503,
                diagnostic = VesperRelayDiagnostic(
                    code = "missing_runtime",
                    message = error.message ?: "Failed to open FFmpeg relay runtime.",
                    details = request.baseDiagnosticDetails(),
                ),
            )
        }

        val errorCode = opened.errorCode
        if (!errorCode.isNullOrBlank()) {
            return VesperRelayFormatAdaptationResult.Failure(
                status = opened.status.takeIf { it > 0 } ?: 503,
                diagnostic = VesperRelayDiagnostic(
                    code = errorCode,
                    message = opened.errorMessage ?: "FFmpeg relay failed.",
                    details = request.baseDiagnosticDetails() + opened.errorDetails,
                ),
            )
        }

        return VesperRelayFormatAdaptationResult.Stream(
            VesperRelayAdaptedStream(
                input = VesperRelayFfmpegInputStream(opened.handle),
                contentType = opened.contentType,
                contentLength = opened.contentLength.takeIf { it >= 0 },
                headers = opened.headers,
                status = opened.status.takeIf { it > 0 } ?: 200,
            ),
        )
    }

    override fun invalidate(sessionId: String) {
        runCatching { VesperRelayFfmpegNative.invalidate(sessionId) }
    }
}

data class VesperRelayFfmpegOpenResult(
    val handle: Long,
    val status: Int,
    val contentType: String,
    val contentLength: Long,
    val headers: Map<String, String>,
    val errorCode: String?,
    val errorMessage: String?,
    val errorDetails: Map<String, String>,
)

private class VesperRelayFfmpegInputStream(
    private var handle: Long,
) : InputStream() {
    override fun read(): Int {
        val buffer = ByteArray(1)
        val read = read(buffer, 0, 1)
        return if (read <= 0) -1 else buffer[0].toInt() and 0xff
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        if (handle == 0L) {
            return -1
        }
        if (length == 0) {
            return 0
        }
        val target = if (offset == 0 && length == buffer.size) {
            buffer
        } else {
            ByteArray(length)
        }
        val read = VesperRelayFfmpegNative.read(handle, target, length)
        if (read > 0 && target !== buffer) {
            System.arraycopy(target, 0, buffer, offset, read)
        }
        return read
    }

    override fun close() {
        val current = handle
        handle = 0
        if (current != 0L) {
            VesperRelayFfmpegNative.close(current)
        }
    }
}

private object VesperRelayFfmpegNative {
    @Volatile
    private var loaded = false

    fun ensureLoaded() {
        if (loaded) {
            return
        }
        synchronized(this) {
            if (!loaded) {
                System.loadLibrary("vesper_player_relay_ffmpeg")
                loaded = true
            }
        }
    }

    @JvmStatic
    external fun runtimeMetadata(): String

    @JvmStatic
    external fun open(requestJson: String): VesperRelayFfmpegOpenResult

    @JvmStatic
    external fun read(handle: Long, buffer: ByteArray, length: Int): Int

    @JvmStatic
    external fun close(handle: Long)

    @JvmStatic
    external fun invalidate(sessionId: String)
}

private fun VesperRelayFormatAdaptationRequest.toNativeJson(): String {
    val headers = JSONObject()
    source.headers.forEach { (key, value) -> headers.put(key, value) }
    val requestHeaders = JSONObject()
    this.requestHeaders.forEach { (key, value) -> requestHeaders.put(key, value) }
    val rangeJson = range?.let {
        JSONObject()
            .put("start", it.start)
            .put("end", it.end)
    }
    return JSONObject()
        .put("sessionId", sessionId)
        .put("sourceUri", source.uri)
        .put("sourceLabel", source.label)
        .put("sourceProtocol", source.protocol.name.lowercase())
        .put("fallbackFormat", fallbackFormat.nativeName())
        .put("resourcePath", resourcePath)
        .put("range", rangeJson)
        .put("sourceHeaders", headers)
        .put("requestHeaders", requestHeaders)
        .put("enableRangeCache", enableRangeCache)
        .put("debugDiagnostics", debugDiagnostics)
        .put("routeId", routeId)
        .put("routeName", routeName)
        .toString()
}

private fun VesperRelayFormatAdaptationRequest.baseDiagnosticDetails(): Map<String, String> =
    mapOf(
        "sessionId" to sessionId,
        "fallbackFormat" to fallbackFormat.name,
        "resourcePath" to resourcePath,
    ) + listOfNotNull(
        routeId?.let { "routeId" to it },
        routeName?.let { "routeName" to it },
    ).toMap()

private fun VesperRelayFallbackFormat.nativeName(): String =
    when (this) {
        VesperRelayFallbackFormat.MpegTs -> "mpeg_ts"
        VesperRelayFallbackFormat.Hls -> "hls"
    }

private fun Context.readRuntimeProfileHash(): String? =
    runCatching {
        assets.open("vesper-ffmpeg-runtime/profile-hash.txt").bufferedReader().use { reader ->
            reader.readText().trim().takeIf { it.isNotBlank() }
        }
    }.getOrNull()
