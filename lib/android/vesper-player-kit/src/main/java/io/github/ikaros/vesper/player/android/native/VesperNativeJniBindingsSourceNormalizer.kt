package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import android.view.ViewGroup
import androidx.media3.common.C
import androidx.media3.common.ColorInfo
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.Timeline
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.util.UnstableApi
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.ExoPlaybackException
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistTracker
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo
import java.io.File
import java.net.URI
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.absoluteValue
import kotlin.math.pow
import kotlin.math.roundToLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject

internal data class NativeFramePacketSource(
    val source: VesperPlayerSource,
) {
    fun close() = Unit
}

internal data class NativeSourceNormalizerResourceOpenOutcome(
    val resource: NativeSourceNormalizerResource? = null,
    val diagnostics: List<Map<String, Any?>> = emptyList(),
)

internal data class NativeSourceNormalizerResource(
    val handle: Long,
    val outputRoute: String,
    val loopbackToken: String?,
    val playbackSource: VesperPlayerSource,
    val diagnostics: List<Map<String, Any?>>,
) {
    val subtitle: String
        get() = "SourceNormalizer $outputRoute"
}

internal fun VesperSourceNormalizerConfiguration.shouldOpenNormalizedResourceForPlayback(
    source: VesperPlayerSource,
): Boolean {
    if (
        mode != VesperSourceNormalizerMode.PreferNormalized &&
            mode != VesperSourceNormalizerMode.RequireNormalized
    ) {
        return false
    }
    if (source.isHostHandledNetworkSource) {
        Log.i(
            NATIVE_JNI_BINDINGS_TAG,
            "source normalizer resource playback skipped for host-handled network source=${source.uri}",
        )
        return false
    }
    return true
}

internal val VesperPlayerSource.isHostHandledNetworkSource: Boolean
    get() =
        kind == VesperPlayerSourceKind.Remote &&
            (
                protocol == VesperPlayerSourceProtocol.Progressive ||
                    protocol == VesperPlayerSourceProtocol.Hls ||
                    protocol == VesperPlayerSourceProtocol.Dash ||
                    uri.startsWith("http://", ignoreCase = true) ||
                    uri.startsWith("https://", ignoreCase = true)
            )

internal const val DEFAULT_NORMALIZED_READ_BUFFER_BYTES = 4L * 1024L * 1024L

internal fun parseSourceNormalizerResource(
    json: String,
    originalSource: VesperPlayerSource,
    loopbackServer: VesperSourceNormalizerLoopbackServer,
): NativeSourceNormalizerResource? =
    runCatching {
        val value = JSONObject(json)
        val handle = value.optLong("handle", 0L)
        val route = value.optString("outputRoute").takeIf(String::isNotBlank) ?: return null
        val primaryPath =
            value.optString("primaryResourcePath").takeIf(String::isNotBlank) ?: return null
        if (handle == 0L) {
            return null
        }
        val cachePolicy = value.optJSONObject("cachePolicy")
        val loopbackHandle =
            loopbackServer.register(
                VesperNormalizedResourceRegistration(
                    outputRoute = route,
                    primaryResourcePath = primaryPath,
                    primaryContentType = value.optString("primaryContentType").takeIf(String::isNotBlank),
                    sessionReadBufferBytes =
                        cachePolicy?.optLong("sessionReadBufferBytes", DEFAULT_NORMALIZED_READ_BUFFER_BYTES)
                            ?: DEFAULT_NORMALIZED_READ_BUFFER_BYTES,
                )
            )
        val playbackProtocol =
            when (route) {
                "hlsShortWindow" -> VesperPlayerSourceProtocol.Hls
                "fmp4LocalStream" -> VesperPlayerSourceProtocol.Progressive
                else -> return null
            }
        NativeSourceNormalizerResource(
            handle = handle,
            outputRoute = route,
            loopbackToken = loopbackHandle.token,
            playbackSource =
                VesperPlayerSource(
                    uri = loopbackHandle.playbackUri,
                    label = originalSource.label,
                    kind = VesperPlayerSourceKind.Remote,
                    protocol = playbackProtocol,
                ),
            diagnostics = value.optJSONArray("diagnostics")?.let { array ->
                List(array.length()) { index ->
                    val diagnostic = jsonObjectToMap(array.getJSONObject(index)).toMutableMap()
                    diagnostic["outputRoute"] = route
                    value.optString("selectedProfile").takeIf(String::isNotBlank)?.let {
                        diagnostic["selectedProfile"] = it
                    }
                    value.optString("primaryContentType").takeIf(String::isNotBlank)?.let {
                        diagnostic["contentType"] = it
                    }
                    diagnostic["primaryResource"] = primaryPath
                    if (value.has("diskBytesUsed")) {
                        diagnostic["diskBytesUsed"] = value.optLong("diskBytesUsed")
                    }
                    value.optJSONObject("cachePolicy")?.let {
                        diagnostic["cachePolicy"] = jsonObjectToMap(it)
                    }
                    if (value.has("cacheQuota")) {
                        diagnostic["cacheQuota"] = value.optLong("cacheQuota")
                    }
                    value.optString("fallbackReason").takeIf(String::isNotBlank)?.let {
                        diagnostic["fallbackReason"] = it
                    }
                    value.optString("route").takeIf(String::isNotBlank)?.let {
                        diagnostic["route"] = it
                    }
                    diagnostic["playbackUri"] = loopbackHandle.playbackUri
                    diagnostic["participation"] = "participated"
                    diagnostic
                }
            } ?: emptyList(),
        )
    }.onFailure { error ->
        Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to parse source normalizer resource open result", error)
    }.getOrNull()

internal fun parseSourceNormalizerBypassDiagnostics(json: String): List<Map<String, Any?>>? =
    runCatching {
        if (!json.trimStart().startsWith("[")) {
            return null
        }
        val array = JSONArray(json)
        List(array.length()) { index ->
            jsonObjectToMap(array.getJSONObject(index))
        }
    }.getOrNull()?.takeIf(List<Map<String, Any?>>::isNotEmpty)

internal fun sourceNormalizerBypassReason(diagnostics: List<Map<String, Any?>>): String {
    val messages = diagnostics.mapNotNull { it["message"] as? String }
    if (messages.any { it.contains("HdrResourceMetadataNotPreserved") }) {
        return "sourceNormalizerResourceBypassedForHdr"
    }
    return messages.firstOrNull() ?: "sourceNormalizerResourceBypassed"
}

internal fun parseNativeFramePipelineJson(json: String): Map<String, Any?>? =
    runCatching {
        jsonObjectToMap(JSONObject(json))
    }.onFailure { error ->
        Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to parse native-frame pipeline result", error)
    }.getOrNull()

