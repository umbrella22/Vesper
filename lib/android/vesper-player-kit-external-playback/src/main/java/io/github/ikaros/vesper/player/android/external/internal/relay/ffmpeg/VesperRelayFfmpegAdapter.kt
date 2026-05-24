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
import java.util.concurrent.ConcurrentHashMap
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

    override fun validate(
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayFormatAdaptationResult.Failure? {
        val context = appContext
            ?: return VesperRelayFormatAdaptationResult.Failure(
                status = 503,
                diagnostic = VesperRelayDiagnostic(
                    code = "missing_runtime",
                    message = "Android context is required for host-prepared relay remux input.",
                    details = request.baseDiagnosticDetails(),
                ),
            )
        return try {
            VesperRelayHostInputSession.validate(context, request)
            null
        } catch (error: VesperRelayHostInputException) {
            VesperRelayFormatAdaptationResult.Failure(
                status = error.status,
                diagnostic = error.diagnostic,
            )
        }
    }

    override fun prewarm(
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayFormatAdaptationResult.Failure? {
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
            VesperRelayFfmpegNative.prewarm(nativeRequest)
        }.getOrElse { error ->
            hostInputSessions.remove(request.sessionId, hostSession)
            hostSession.close()
            return VesperRelayFormatAdaptationResult.Failure(
                status = 503,
                diagnostic = VesperRelayDiagnostic(
                    code = "missing_runtime",
                    message = error.message ?: "Failed to prewarm FFmpeg relay runtime.",
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
                    message = opened.errorMessage ?: "FFmpeg relay prewarm failed.",
                    details = request.baseDiagnosticDetails() + opened.errorDetails,
                ),
            )
        }

        return null
    }

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
