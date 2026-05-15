package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayAdaptedStream
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFallbackFormat
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationResult
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdapter
import io.github.ikaros.vesper.player.android.external.internal.relay.contentType
import java.io.ByteArrayInputStream
import java.io.InputStream
import java.util.concurrent.ConcurrentHashMap
import org.json.JSONArray
import org.json.JSONObject

class VesperRelayFfmpegAdapter @JvmOverloads constructor(
    context: Context? = null,
) : VesperRelayFormatAdapter {
    private val appContext = context?.applicationContext
    private val runtimeProfileHash = appContext?.readRuntimeProfileHash()
    private val hostInputSessions = ConcurrentHashMap<String, VesperRelayHostInputSession>()

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

        if (request.headOnly) {
            return VesperRelayFormatAdaptationResult.Stream(
                VesperRelayAdaptedStream(
                    input = ByteArrayInputStream(ByteArray(0)),
                    contentType = request.fallbackFormat.contentType(),
                    headers = profileHeaders(pluginProfileHash),
                    status = 200,
                ),
            )
        }

        val context = appContext
            ?: return VesperRelayFormatAdaptationResult.Failure(
                status = 503,
                diagnostic = VesperRelayDiagnostic(
                    code = "missing_runtime",
                    message = "Android context is required for host-prepared relay remux input.",
                    details = request.baseDiagnosticDetails(),
                ),
            )
        val hostSession =
            try {
                hostInputSession(context, request)
            } catch (error: VesperRelayHostInputException) {
                return VesperRelayFormatAdaptationResult.Failure(
                    status = error.status,
                    diagnostic = error.diagnostic,
                )
            }

        val nativeRequest = request.toNativeJson(hostSession.tracks)
        val opened = runCatching {
            VesperRelayFfmpegNative.open(nativeRequest)
        }.getOrElse { error ->
            hostInputSessions.remove(request.sessionId, hostSession)
            hostSession.close()
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
            val hostDiagnostic = hostSession.failureDiagnostic()
            hostInputSessions.remove(request.sessionId, hostSession)
            hostSession.close()
            if (hostDiagnostic != null) {
                return VesperRelayFormatAdaptationResult.Failure(
                    status = hostDiagnosticStatus(hostDiagnostic),
                    diagnostic = hostDiagnostic,
                )
            }
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
                closeable = hostSession,
            ),
        )
    }

    override fun invalidate(sessionId: String) {
        hostInputSessions.remove(sessionId)?.close()
        runCatching { VesperRelayFfmpegNative.invalidate(sessionId) }
    }

    private fun hostInputSession(
        context: Context,
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayHostInputSession {
        hostInputSessions[request.sessionId]?.let { existing ->
            if (existing.failureDiagnostic() == null) {
                return existing
            }
            hostInputSessions.remove(request.sessionId, existing)
            existing.close()
            runCatching { VesperRelayFfmpegNative.invalidate(request.sessionId) }
        }
        val created = VesperRelayHostInputSession.create(context, request)
        val previous = hostInputSessions.putIfAbsent(request.sessionId, created)
        if (previous != null) {
            created.close()
            return previous
        }
        created.start()
        return created
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

private fun VesperRelayFormatAdaptationRequest.toNativeJson(
    tracks: List<VesperRelayPreparedTrack>,
): String {
    val trackArray = JSONArray()
    tracks.forEach { track ->
        trackArray.put(
            JSONObject()
                .put("kind", track.kind)
                .put("pipePath", track.pipePath)
                .put("mediaId", track.mediaId)
                .put("mimeType", track.mimeType)
                .put("codecs", track.codecs),
        )
    }
    val rangeJson = range?.let {
        JSONObject()
            .put("start", it.start)
            .put("end", it.end)
    }
    return JSONObject()
        .put("sessionId", sessionId)
        .put("inputMode", HOST_PREPARED_DASH_INPUT_MODE)
        .put("tracks", trackArray)
        .put("sourceUriHash", hashForDiagnostic(source.uri))
        .put("sourceLabel", source.label)
        .put("sourceProtocol", source.protocol.name.lowercase())
        .put("fallbackFormat", fallbackFormat.nativeName())
        .put("resourcePath", resourcePath)
        .put("range", rangeJson)
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

private fun profileHeaders(profileHash: String?): Map<String, String> =
    profileHash
        ?.takeIf { it.isNotBlank() }
        ?.let { mapOf("X-Vesper-FFmpeg-Profile-Hash" to it) }
        ?: emptyMap()

private fun hostDiagnosticStatus(diagnostic: VesperRelayDiagnostic): Int =
    when (diagnostic.code) {
        "unsupported_dynamic_dash",
        "unsupported_dash_layout",
        "unsupported_encrypted_dash",
        -> 415
        "host_fetch_timeout" -> 504
        "host_input_cancelled" -> 499
        else -> 502
    }

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
