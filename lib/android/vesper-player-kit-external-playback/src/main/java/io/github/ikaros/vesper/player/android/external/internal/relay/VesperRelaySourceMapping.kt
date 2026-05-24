package io.github.ikaros.vesper.player.android.external.internal.relay

import android.net.Uri
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import java.io.File
import java.net.URI
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
import java.util.Locale

internal fun VesperPlayerSource.toFormatAdaptationRequest(
    token: String,
    adaptation: VesperRelayFormatAdaptationRegistration,
    resourcePath: String,
    headOnly: Boolean,
    range: ByteRangeRequest?,
    requestHeaders: Map<String, String>,
): VesperRelayFormatAdaptationRequest =
    VesperRelayFormatAdaptationRequest(
        sessionId = token,
        source = this,
        fallbackFormat = adaptation.fallbackFormat,
        resourcePath = resourcePath,
        range = range,
        requestHeaders = requestHeaders,
        enableRangeCache = adaptation.config.enableRangeCache,
        dashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(
            allowRemoteReferences = adaptation.config.allowRemoteDashMediaReferences,
            allowPrivateAddresses = adaptation.config.allowPrivateRemoteDashMediaAddresses,
            allowedRequestHeaders = adaptation.config.remoteDashMediaRequestHeaders,
        ),
        debugDiagnostics = adaptation.config.debugDiagnostics,
        headOnly = headOnly,
        routeId = adaptation.routeId,
        routeName = adaptation.routeName,
    )

internal fun VesperRelayDiagnostic.withHttpStatus(status: Int): VesperRelayDiagnostic =
    copy(details = details + ("httpStatus" to status.toString()))

internal fun String.isHopByHopHeader(): Boolean =
    lowercase(Locale.US) in setOf(
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        "range",
    )

internal fun VesperPlayerSource.relayPath(
    token: String,
    adaptation: VesperRelayFormatAdaptationRegistration?,
): String {
    if (adaptation != null) {
        val rawBaseName = listOfNotNull(label, uri.fileNameFromUri())
            .firstOrNull { it.isNotBlank() }
            ?: "media"
        val baseName = rawBaseName
            .substringBeforeLast('.', missingDelimiterValue = rawBaseName)
            .takeIf { it.isNotBlank() }
            ?: "media"
        return "/media/$token/${baseName.urlPathSegmentEncoded()}.${adaptation.fallbackFormat.urlExtension()}"
    }
    val fileName = listOfNotNull(uri.fileNameFromUri(), label)
        .firstOrNull { it.contentTypeFromPath() != null }
        ?.urlPathSegmentEncoded()
    return if (fileName == null) {
        "/media/$token"
    } else {
        "/media/$token/$fileName"
    }
}

private fun String.fileNameFromUri(): String? {
    val javaUriPath = runCatching { URI(this).path }.getOrNull()
    val androidUriPath = runCatching { Uri.parse(this).lastPathSegment }.getOrNull()
    return (javaUriPath ?: androidUriPath)
        ?.substringAfterLast('/')
        ?.takeIf { it.isNotBlank() }
}

private fun String.urlPathSegmentEncoded(): String =
    URLEncoder.encode(this, StandardCharsets.UTF_8.name())
        .replace("+", "%20")

internal fun String.toFile(): File =
    if (startsWith("file://", ignoreCase = true)) {
        File(Uri.parse(this).path ?: "")
    } else {
        File(this)
    }

internal fun VesperPlayerSource.contentTypeGuess(): String {
    return listOf(uri, label)
        .firstNotNullOfOrNull { it.contentTypeFromPath() }
        ?: when (protocol) {
            VesperPlayerSourceProtocol.Hls -> "application/x-mpegURL"
            VesperPlayerSourceProtocol.Dash -> "application/dash+xml"
            VesperPlayerSourceProtocol.Progressive -> "video/mp4"
            else -> "application/octet-stream"
        }
}

private fun String.contentTypeFromPath(): String? {
    val path = substringBefore('?').substringBefore('#').lowercase(Locale.US)
    return when {
        path.endsWith(".m3u8") -> "application/x-mpegURL"
        path.endsWith(".m3u") -> "audio/mpegurl"
        path.endsWith(".mpd") -> "application/dash+xml"
        path.endsWith(".mp4") || path.endsWith(".m4v") -> "video/mp4"
        path.endsWith(".mkv") -> "video/x-matroska"
        path.endsWith(".webm") -> "video/webm"
        path.endsWith(".mov") -> "video/quicktime"
        path.endsWith(".avi") -> "video/x-msvideo"
        path.endsWith(".3gp") -> "video/3gpp"
        path.endsWith(".mts") || path.endsWith(".ts") -> "video/mp2t"
        path.endsWith(".mp3") -> "audio/mpeg"
        path.endsWith(".m4a") -> "audio/mp4"
        path.endsWith(".aac") -> "audio/aac"
        path.endsWith(".ogg") -> "audio/ogg"
        path.endsWith(".opus") -> "audio/opus"
        path.endsWith(".wav") -> "audio/wav"
        path.endsWith(".flac") -> "audio/flac"
        path.endsWith(".wma") -> "audio/x-ms-wma"
        path.endsWith(".jpg") || path.endsWith(".jpeg") -> "image/jpeg"
        path.endsWith(".png") -> "image/png"
        path.endsWith(".gif") -> "image/gif"
        path.endsWith(".bmp") -> "image/bmp"
        path.endsWith(".webp") -> "image/webp"
        path.endsWith(".tif") || path.endsWith(".tiff") -> "image/tiff"
        else -> null
    }
}
