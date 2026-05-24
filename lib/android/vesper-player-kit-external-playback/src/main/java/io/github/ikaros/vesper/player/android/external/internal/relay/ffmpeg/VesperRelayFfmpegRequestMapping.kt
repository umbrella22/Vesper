package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFallbackFormat
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import org.json.JSONArray
import org.json.JSONObject

internal fun VesperRelayFormatAdaptationRequest.toNativeJson(
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

internal fun VesperRelayFormatAdaptationRequest.baseDiagnosticDetails(): Map<String, String> =
    mapOf(
        "sessionId" to sessionId,
        "fallbackFormat" to fallbackFormat.name,
        "resourcePath" to resourcePath,
    ) + listOfNotNull(
        routeId?.let { "routeId" to it },
        routeName?.let { "routeName" to it },
    ).toMap()

internal fun profileHeaders(profileHash: String?): Map<String, String> =
    profileHash
        ?.takeIf { it.isNotBlank() }
        ?.let { mapOf("X-Vesper-FFmpeg-Profile-Hash" to it) }
        ?: emptyMap()

internal fun hostDiagnosticStatus(diagnostic: VesperRelayDiagnostic): Int =
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

internal fun Context.readRuntimeProfileHash(): String? =
    runCatching {
        assets.open("vesper-ffmpeg-runtime/profile-hash.txt").bufferedReader().use { reader ->
            reader.readText().trim().takeIf { it.isNotBlank() }
        }
    }.getOrNull()
