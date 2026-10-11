package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayAdaptedStream
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayFallbackFormat
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationResult
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayFormatAdapter
import io.github.umbrella22.vesper.player.android.external.internal.relay.contentType
import java.io.ByteArrayInputStream
import java.io.Closeable
import java.io.InterruptedIOException
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

class VesperRelayFfmpegAdapter @JvmOverloads constructor(
    context: Context? = null,
) : VesperRelayFormatAdapter {
    private val appContext = context?.applicationContext
    private val runtimeProfileHash = appContext?.readRuntimeProfileHash()
    private val hostInputSessions = VesperRelaySessionRegistry<PreparedSession>()

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

        val nativeRequest = request.toNativeJson(hostSession.host.tracks, hostSession.lease.handle.get())
        val opened = runCatching {
            VesperRelayFfmpegNative.prewarm(nativeRequest)
        }.getOrElse { error ->
            hostInputSessions.invalidate(request.sessionId)
            return VesperRelayFormatAdaptationResult.Failure(
                status = 503,
                diagnostic = VesperRelayDiagnostic(
                    code = "missing_runtime",
                    message = error.message ?: "Failed to prewarm FFmpeg relay runtime.",
                    details = request.baseDiagnosticDetails(),
                ),
            )
        }

        if (!request.lifetime.isActive || hostSession.lease.handle.get() == 0L) {
            if (opened.handle != 0L) VesperRelayFfmpegNative.close(opened.handle)
            return cancelledFailure(request)
        }
        val errorCode = opened.errorCode
        if (!errorCode.isNullOrBlank()) {
            val hostDiagnostic = hostSession.host.failureDiagnostic()
            if (hostDiagnostic != null || opened.status !in listOf(404, 416)) {
                hostInputSessions.invalidate(request.sessionId)
            }
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

        if (!request.lifetime.isActive) return cancelledFailure(request)
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

        val nativeRequest = request.toNativeJson(hostSession.host.tracks, hostSession.lease.handle.get())
        val opened = runCatching {
            VesperRelayFfmpegNative.open(nativeRequest)
        }.getOrElse { error ->
            hostInputSessions.invalidate(request.sessionId)
            return VesperRelayFormatAdaptationResult.Failure(
                status = 503,
                diagnostic = VesperRelayDiagnostic(
                    code = "missing_runtime",
                    message = error.message ?: "Failed to open FFmpeg relay runtime.",
                    details = request.baseDiagnosticDetails(),
                ),
            )
        }

        if (!request.lifetime.isActive || hostSession.lease.handle.get() == 0L) {
            if (opened.handle != 0L) VesperRelayFfmpegNative.close(opened.handle)
            return cancelledFailure(request)
        }
        val errorCode = opened.errorCode
        if (!errorCode.isNullOrBlank()) {
            val hostDiagnostic = hostSession.host.failureDiagnostic()
            if (hostDiagnostic != null || opened.status !in listOf(404, 416)) {
                hostInputSessions.invalidate(request.sessionId)
            }
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
            ),
        )
    }

    override fun invalidate(sessionId: String) {
        hostInputSessions.invalidate(sessionId)
    }

    private class NativeLease(handle: Long) : Closeable {
        val handle = AtomicLong(handle)
        override fun close() {
            val current = handle.getAndSet(0)
            if (current != 0L) VesperRelayFfmpegNative.retireSession(current)
        }
    }

    private class PreparedSession(val host: VesperRelayHostInputSession, val lease: NativeLease) : Closeable {
        override fun close() {
            lease.close()
            host.close()
        }
    }

    private fun hostInputSession(context: Context, request: VesperRelayFormatAdaptationRequest): PreparedSession {
        try {
            return hostInputSessions.acquire(request.sessionId, request.lifetime) { lifetime ->
                val lease = NativeLease(VesperRelayFfmpegNative.reserveSession(request.sessionId))
                if (lease.handle.get() == 0L || !lifetime.attach(lease)) throw HostInputCancelledException()
                val host = VesperRelayHostInputSession.create(context, request, lifetime = lifetime)
                if (!lifetime.attach(host)) throw HostInputCancelledException()
                host.start()
                lifetime.checkActive()
                PreparedSession(host, lease)
            }
        } catch (error: InterruptedIOException) {
            throw request.hostInputException("host_input_cancelled", 499, "Host-prepared DASH input was cancelled.", emptyMap())
        } catch (error: HostInputCancelledException) {
            throw request.hostInputException("host_input_cancelled", 499, "Host-prepared DASH input was cancelled.", emptyMap())
        }
    }

    private fun cancelledFailure(request: VesperRelayFormatAdaptationRequest) = VesperRelayFormatAdaptationResult.Failure(
        499, VesperRelayDiagnostic("host_input_cancelled", "Host-prepared DASH input was cancelled.", details = request.baseDiagnosticDetails()),
    )
}
