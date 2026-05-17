package io.github.ikaros.vesper.player.android.external.internal.relay

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.io.ByteArrayInputStream
import java.io.Closeable
import java.io.InputStream

enum class VesperRelayFallbackFormat {
    MpegTs,
    Hls,
}

data class VesperRelayFormatAdaptationConfig(
    val enabled: Boolean = false,
    val preferredFallback: VesperRelayFallbackFormat = VesperRelayFallbackFormat.MpegTs,
    val allowHls: Boolean = true,
    val enableRangeCache: Boolean = true,
    val allowRemoteDashMediaReferences: Boolean = false,
    val allowPrivateRemoteDashMediaAddresses: Boolean = false,
    val remoteDashMediaRequestHeaders: Set<String> = DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS,
    val debugDiagnostics: Boolean = false,
)

data class VesperRelayFormatAdaptationRegistration(
    val fallbackFormat: VesperRelayFallbackFormat,
    val config: VesperRelayFormatAdaptationConfig = VesperRelayFormatAdaptationConfig(),
    val routeId: String? = null,
    val routeName: String? = null,
)

data class VesperRelayFormatAdaptationRequest(
    val sessionId: String,
    val source: VesperPlayerSource,
    val fallbackFormat: VesperRelayFallbackFormat,
    val resourcePath: String,
    val range: ByteRangeRequest?,
    val requestHeaders: Map<String, String>,
    val enableRangeCache: Boolean,
    val dashRemoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
    val debugDiagnostics: Boolean,
    val headOnly: Boolean = false,
    val routeId: String? = null,
    val routeName: String? = null,
)

data class VesperRelayDashRemoteMediaPolicy(
    val allowRemoteReferences: Boolean = false,
    val allowPrivateAddresses: Boolean = false,
    val allowedRequestHeaders: Set<String> = DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS,
)

val DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS: Set<String> =
    setOf("User-Agent", "Accept", "Accept-Language")

data class VesperRelayDiagnostic(
    val code: String,
    val message: String,
    val severity: String = "error",
    val details: Map<String, String> = emptyMap(),
)

sealed class VesperRelayFormatAdaptationResult {
    data class Stream(
        val stream: VesperRelayAdaptedStream,
    ) : VesperRelayFormatAdaptationResult()

    data class Failure(
        val status: Int,
        val diagnostic: VesperRelayDiagnostic,
    ) : VesperRelayFormatAdaptationResult()
}

data class VesperRelayAdaptedStream(
    val input: InputStream,
    val contentType: String,
    val contentLength: Long? = null,
    val headers: Map<String, String> = emptyMap(),
    val status: Int = 200,
    val closeable: Closeable? = null,
)

interface VesperRelayFormatAdapter {
    val profileHash: String?
        get() = null

    fun validate(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult.Failure? = null

    fun prewarm(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult.Failure? = null

    fun open(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult

    fun invalidate(sessionId: String) = Unit
}

class VesperUnavailableRelayFormatAdapter(
    private val reason: String = "FFmpeg relay runtime is not packaged.",
) : VesperRelayFormatAdapter {
    override fun prewarm(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult.Failure =
        missingRuntimeFailure(request)

    override fun open(request: VesperRelayFormatAdaptationRequest): VesperRelayFormatAdaptationResult =
        missingRuntimeFailure(request)

    private fun missingRuntimeFailure(
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayFormatAdaptationResult.Failure =
        VesperRelayFormatAdaptationResult.Failure(
            status = 503,
            diagnostic = VesperRelayDiagnostic(
                code = "missing_runtime",
                message = reason,
                details = mapOf(
                    "sessionId" to request.sessionId,
                    "fallbackFormat" to request.fallbackFormat.name,
                ),
            ),
        )
}

fun ByteArray.asRelayAdaptedStream(
    contentType: String,
    status: Int = 200,
    headers: Map<String, String> = emptyMap(),
): VesperRelayAdaptedStream =
    VesperRelayAdaptedStream(
        input = ByteArrayInputStream(this),
        contentType = contentType,
        contentLength = size.toLong(),
        headers = headers,
        status = status,
    )

fun VesperRelayFallbackFormat.contentType(): String =
    when (this) {
        VesperRelayFallbackFormat.MpegTs -> "video/mp2t"
        VesperRelayFallbackFormat.Hls -> "application/x-mpegURL"
    }

fun VesperRelayFallbackFormat.urlExtension(): String =
    when (this) {
        VesperRelayFallbackFormat.MpegTs -> "ts"
        VesperRelayFallbackFormat.Hls -> "m3u8"
    }
