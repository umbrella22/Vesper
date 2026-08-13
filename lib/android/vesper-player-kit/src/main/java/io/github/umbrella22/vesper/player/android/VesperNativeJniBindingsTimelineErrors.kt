package io.github.umbrella22.vesper.player.android

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
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.datasource.cache.CacheDataSource
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

internal fun Int.toRuntimeColorRangeName(): String =
    when (this) {
        Format.NO_VALUE -> "unknown"
        C.COLOR_RANGE_LIMITED -> "limited"
        C.COLOR_RANGE_FULL -> "full"
        else -> "unknown($this)"
    }

internal fun Int.toRuntimeColorTransferName(): String =
    when (this) {
        Format.NO_VALUE -> "unknown"
        C.COLOR_TRANSFER_LINEAR -> "linear"
        C.COLOR_TRANSFER_SDR -> "sdr"
        C.COLOR_TRANSFER_SRGB -> "srgb"
        C.COLOR_TRANSFER_GAMMA_2_2 -> "gamma2.2"
        C.COLOR_TRANSFER_ST2084 -> "st2084"
        C.COLOR_TRANSFER_HLG -> "hlg"
        else -> "unknown($this)"
    }

internal fun buildDataSourceFactory(
    appContext: Context,
    cachePolicy: NativeCachePolicy,
    headers: Map<String, String> = emptyMap(),
): androidx.media3.datasource.DataSource.Factory {
    // Each resource owner receives its own factory. Main-media headers never
    // enter an external-subtitle factory, even when both resources use the
    // same URI.
    val httpFactory =
        DefaultHttpDataSource.Factory().apply {
            if (headers.isNotEmpty()) {
                setDefaultRequestProperties(headers)
            }
        }
    val upstreamFactory = DefaultDataSource.Factory(appContext, httpFactory)
    val resolvedCachePolicy = resolveCachePolicy(cachePolicy)
    val baseFactory =
        if (!resolvedCachePolicy.enabled) {
            upstreamFactory
        } else {
            val cache =
                VesperMediaCacheStore.cache(
                    appContext = appContext,
                    maxDiskBytes = resolvedCachePolicy.maxDiskBytes,
                )
            CacheDataSource.Factory()
                .setCache(cache)
                .setUpstreamDataSourceFactory(upstreamFactory)
                .setFlags(CacheDataSource.FLAG_IGNORE_CACHE_ON_ERROR)
        }
    return baseFactory
}

internal enum class NativeResourceRequestRole {
    Media,
    ExternalSubtitle,
}

internal fun resolveResourceRequestHeaders(
    role: NativeResourceRequestRole,
    mediaHeaders: Map<String, String>,
    subtitleHeaders: Map<String, String>,
): Map<String, String> =
    when (role) {
        NativeResourceRequestRole.Media -> mediaHeaders
        NativeResourceRequestRole.ExternalSubtitle -> subtitleHeaders
    }

internal fun subtitleNativeErrorFromJson(errorJson: String): VesperPlayerUnsupportedOperation {
    val payload = JSONObject(errorJson)
    val code = (payload.opt("code") as? String) ?: "subtitle_selection_mismatch"
    val phase = (payload.opt("phase") as? String) ?: "selection"
    val trackId = payload.opt("trackId") as? String
    val message = (payload.opt("message") as? String) ?: "native subtitle selection failed"
    val commandId = payload.longOrNull("commandId")
    val sourceEpoch = payload.longOrNull("sourceEpoch")
    return subtitleNativeError(
        code = code,
        phase = phase,
        trackId = trackId,
        retriable = payload.optBoolean("retriable", false),
        commandId = commandId,
        sourceEpoch = sourceEpoch,
        message = message,
    )
}

internal fun fixedTrackNativeErrorFromJson(errorJson: String): VesperFixedTrackSelectionException {
    val payload = JSONObject(errorJson)
    val details = jsonObjectToMap(payload)
    val message =
        (payload.opt("message") as? String)
            ?: "native fixed-track selection failed"
    return VesperFixedTrackSelectionException(
        code = (payload.opt("code") as? String) ?: "trackUnsupported",
        trackId = payload.opt("trackId") as? String,
        expectedCatalogRevision = payload.longOrNull("expectedCatalogRevision"),
        actualCatalogRevision = payload.longOrNull("actualCatalogRevision"),
        message = message,
        extraDetails = details - FIXED_TRACK_BASE_DETAIL_KEYS,
    )
}

internal fun abrPolicyNativeErrorFromJson(errorJson: String): RuntimeException {
    val payload = JSONObject(errorJson)
    if ((payload.opt("domain") as? String) == "fixedTrack") {
        return fixedTrackNativeErrorFromJson(errorJson)
    }
    val message =
        (payload.opt("message") as? String)
            ?: "native ABR policy command failed"
    return VesperPlayerCommandException(
        VesperPlayerErrorState(
            message = message,
            code = VesperPlayerErrorCode.fromWireName(payload.opt("code") as? String),
            category = VesperPlayerErrorCategory.fromWireName(payload.opt("category") as? String),
            retriable = payload.optBoolean("retriable", false),
            details = jsonObjectToMap(payload),
        )
    )
}

private val FIXED_TRACK_BASE_DETAIL_KEYS =
    setOf(
        "domain",
        "code",
        "trackId",
        "expectedCatalogRevision",
        "actualCatalogRevision",
        "message",
    )

internal fun subtitleNativeError(
    code: String,
    phase: String,
    trackId: String?,
    retriable: Boolean,
    commandId: Long?,
    sourceEpoch: Long?,
    message: String,
): VesperPlayerUnsupportedOperation =
    VesperPlayerUnsupportedOperation(
        message,
        mapOf(
            "domain" to "subtitle",
            "code" to code,
            "phase" to phase,
            "trackId" to trackId,
            "retriable" to retriable,
            "commandId" to commandId,
            "sourceEpoch" to sourceEpoch,
        ),
    )

private fun JSONObject.longOrNull(key: String): Long? =
    if (!has(key) || isNull(key)) null else optLong(key)

internal fun resolveCachePolicy(
    cachePolicy: NativeCachePolicy,
): ResolvedCachePolicy {
    val maxDiskBytes = cachePolicy.maxDiskBytes.takeIf { cachePolicy.hasMaxDiskBytes } ?: 0L
    return ResolvedCachePolicy(enabled = maxDiskBytes > 0L, maxDiskBytes = maxDiskBytes)
}

internal fun resolveResiliencePolicy(
    source: VesperPlayerSource,
    resiliencePolicy: VesperPlaybackResiliencePolicy,
): NativeResolvedResiliencePolicy =
    VesperNativeJni.resolveResiliencePolicy(
        sourceKindOrdinal = source.kind.ordinal,
        sourceProtocolOrdinal = source.protocol.ordinal,
        bufferingPolicy = resiliencePolicy.buffering.toNativePayload(),
        retryPolicy = resiliencePolicy.retry.toNativePayload(),
        cachePolicy = resiliencePolicy.cache.toNativePayload(),
    )

internal fun resolveTrackPreferences(
    trackPreferencePolicy: VesperTrackPreferencePolicy,
): VesperTrackPreferencePolicy =
    VesperNativeJni.resolveTrackPreferences(trackPreferencePolicy.toNativePayload())
        .toPublicTrackPreferencePolicy()

internal fun Long.normalizedOptionalMs(): Long? =
    if (this == C.TIME_UNSET || this < 0L) {
        null
    } else {
        this
    }

internal fun Long.normalizedDurationMs(): Long =
    if (this == C.TIME_UNSET || this < 0L) {
        -1L
    } else {
        this
    }

internal data class LiveTimelineWindowCoordinates(
    val startMs: Long,
    val durationMs: Long?,
)

internal fun timelinePositionFromWindowPosition(windowStartMs: Long, windowPositionMs: Long): Long =
    windowStartMs.coerceAtLeast(0L) + windowPositionMs.coerceAtLeast(0L)

internal fun windowPositionFromTimelinePosition(
    timelinePositionMs: Long,
    window: LiveTimelineWindowCoordinates,
): Long {
    val position = (timelinePositionMs - window.startMs).coerceAtLeast(0L)
    return window.durationMs?.let { position.coerceAtMost(it.coerceAtLeast(0L)) } ?: position
}

internal fun ExoPlayer.currentLiveTimelineWindow(): LiveTimelineWindowCoordinates? {
    val timeline = currentTimeline
    if (timeline.isEmpty) {
        return null
    }

    val window = Timeline.Window()
    timeline.getWindow(currentMediaItemIndex, window)
    return LiveTimelineWindowCoordinates(
        startMs = window.getPositionInFirstPeriodMs().coerceAtLeast(0L),
        durationMs = window.getDurationMs().normalizedOptionalMs(),
    )
}

internal fun ExoPlayer.timelinePositionForWindowPosition(windowPositionMs: Long): Long {
    val window = if (isCurrentMediaItemLive) currentLiveTimelineWindow() else null
    return timelinePositionFromWindowPosition(window?.startMs ?: 0L, windowPositionMs)
}

internal fun ExoPlayer.windowPositionForTimelinePosition(timelinePositionMs: Long): Long {
    val window = if (isCurrentMediaItemLive) currentLiveTimelineWindow() else null
    return if (window != null) {
        windowPositionFromTimelinePosition(timelinePositionMs, window)
    } else {
        timelinePositionMs.coerceAtLeast(0L)
    }
}

internal data class NativePlaybackError(
    val codeOrdinal: Int,
    val categoryOrdinal: Int,
    val retriable: Boolean,
    val likelyCapabilityIssue: Boolean = false,
    val capabilityFailureCause: AndroidCapabilityFailureCause? = null,
    val capabilityFailureAxis: AndroidCapabilityFailureAxis? = null,
    val causeEvidence: AndroidPlaybackFailureCauseEvidence? = null,
)

internal data class AndroidPlaybackFailureCauseEvidence(
    val causeClass: String?,
    val causeMessage: String?,
    val rootCauseClass: String?,
    val rootCauseMessage: String?,
    val rendererName: String? = null,
    val rendererIndex: Int? = null,
    val rendererFormatSupport: String? = null,
    val rendererFormatSampleMimeType: String? = null,
    val rendererFormatCodecs: String? = null,
    val rendererFormatWidth: Int? = null,
    val rendererFormatHeight: Int? = null,
    val rendererFormatFrameRate: Float? = null,
) {
    fun diagnostics(): Map<String, String> =
        linkedMapOf<String, String>().also { output ->
            causeClass?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureCauseClass"] = it
            }
            causeMessage?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureCauseMessage"] = it
            }
            rootCauseClass?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureRootCauseClass"] = it
            }
            rootCauseMessage?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureRootCauseMessage"] = it
            }
            rendererName?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureRendererName"] = it
            }
            rendererIndex?.let {
                output["playbackFailureRendererIndex"] = it.toString()
            }
            rendererFormatSupport?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureRendererFormatSupport"] = it
            }
            rendererFormatSampleMimeType?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureRendererFormatSampleMimeType"] = it
            }
            rendererFormatCodecs?.takeIf { it.isNotBlank() }?.let {
                output["playbackFailureRendererFormatCodecs"] = it
            }
            rendererFormatWidth?.takeIf { it > 0 }?.let {
                output["playbackFailureRendererFormatWidth"] = it.toString()
            }
            rendererFormatHeight?.takeIf { it > 0 }?.let {
                output["playbackFailureRendererFormatHeight"] = it.toString()
            }
            rendererFormatFrameRate?.takeIf { it > 0f }?.let {
                output["playbackFailureRendererFormatFrameRate"] = it.toString()
            }
        }
}

internal fun AndroidPlaybackFailureCauseEvidence?.runtimeFormatConvergenceDiagnostics(
    runtimeDiagnostics: Map<String, Any?>,
): Map<String, String> {
    this ?: return emptyMap()
    val rendererMimeType = rendererFormatSampleMimeType?.takeIf(String::isNotBlank)
    val runtimeMimeType = runtimeDiagnostics.stringValue("runtimeFormatSampleMimeType")
    val rendererCodecs = rendererFormatCodecs?.takeIf(String::isNotBlank)
    val runtimeCodecs = runtimeDiagnostics.stringValue("runtimeFormatCodecs")
    val rendererWidth = rendererFormatWidth
    val runtimeWidth = runtimeDiagnostics.intValue("runtimeFormatWidth")
    val rendererHeight = rendererFormatHeight
    val runtimeHeight = runtimeDiagnostics.intValue("runtimeFormatHeight")
    val rendererFrameRate = rendererFormatFrameRate
    val runtimeFrameRate = runtimeDiagnostics.floatValue("runtimeFormatFrameRate")
    return linkedMapOf<String, String>().also { output ->
        rendererFormatSupport?.takeIf(String::isNotBlank)?.let {
            output["playbackFailureRendererFormatSupported"] = (it == "handled").toString()
        }
        if (rendererMimeType != null && runtimeMimeType != null) {
            output["playbackFailureRendererFormatMimeMatchesRuntime"] =
                (rendererMimeType == runtimeMimeType).toString()
        }
        if (rendererCodecs != null && runtimeCodecs != null) {
            output["playbackFailureRendererFormatCodecsMatchRuntime"] =
                (rendererCodecs == runtimeCodecs).toString()
        }
        if (rendererWidth != null && runtimeWidth != null && rendererHeight != null && runtimeHeight != null) {
            output["playbackFailureRendererFormatSizeMatchesRuntime"] =
                (rendererWidth == runtimeWidth && rendererHeight == runtimeHeight).toString()
        }
        if (rendererFrameRate != null && runtimeFrameRate != null) {
            output["playbackFailureRendererFormatFrameRateMatchesRuntime"] =
                rendererFrameRate.nearlyEquals(runtimeFrameRate).toString()
        }
    }
}

internal enum class AndroidCapabilityFailureCause {
    ContainerUnsupported,
    ManifestUnsupported,
    DecoderInit,
    DecoderQuery,
    DecodeFailed,
    FormatUnsupported,
    FormatExceedsCapabilities,
}

internal val AndroidCapabilityFailureCause.wireName: String
    get() =
        when (this) {
            AndroidCapabilityFailureCause.ContainerUnsupported -> "containerUnsupported"
            AndroidCapabilityFailureCause.ManifestUnsupported -> "manifestUnsupported"
            AndroidCapabilityFailureCause.DecoderInit -> "decoderInit"
            AndroidCapabilityFailureCause.DecoderQuery -> "decoderQuery"
            AndroidCapabilityFailureCause.DecodeFailed -> "decodeFailed"
            AndroidCapabilityFailureCause.FormatUnsupported -> "formatUnsupported"
            AndroidCapabilityFailureCause.FormatExceedsCapabilities -> "formatExceedsCapabilities"
        }

internal enum class AndroidCapabilityFailureAxis {
    Container,
    Manifest,
    Decoder,
    Renderer,
    DisplaySurface,
}

internal val AndroidCapabilityFailureAxis.wireName: String
    get() =
        when (this) {
            AndroidCapabilityFailureAxis.Container -> "container"
            AndroidCapabilityFailureAxis.Manifest -> "manifest"
            AndroidCapabilityFailureAxis.Decoder -> "decoder"
            AndroidCapabilityFailureAxis.Renderer -> "renderer"
            AndroidCapabilityFailureAxis.DisplaySurface -> "displaySurface"
        }

internal data class ResolvedCachePolicy(
    val enabled: Boolean,
    val maxDiskBytes: Long,
)

internal object VesperMediaCacheStore {
    private val owner = VesperSingleFlightOwner<Long, VesperSimpleCacheResource> { resource ->
        resource.close()
    }

    fun cache(
        appContext: Context,
        maxDiskBytes: Long,
    ): androidx.media3.datasource.cache.SimpleCache =
        owner.get(maxDiskBytes) {
            VesperSimpleCacheResource.create(
                appContext = appContext,
                cacheDir = java.io.File(appContext.cacheDir, "vesper-media-cache/$maxDiskBytes"),
                maxDiskBytes = maxDiskBytes,
            )
        }.cache
}

internal fun classifyPlaybackException(error: PlaybackException): NativePlaybackError {
    val causeEvidence = error.playbackFailureCauseEvidence()
    val classified =
        if (error.hasCause(HlsPlaylistTracker.PlaylistStuckException::class.java)) {
        NativePlaybackError(
            codeOrdinal = BACKEND_FAILURE_ORDINAL,
            categoryOrdinal = NETWORK_CATEGORY_ORDINAL,
            retriable = true,
        )
    } else {
        when (error.errorCode) {
            PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED,
            PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
            PlaybackException.ERROR_CODE_IO_INVALID_HTTP_CONTENT_TYPE,
            PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS,
            -> NativePlaybackError(
                codeOrdinal = BACKEND_FAILURE_ORDINAL,
                categoryOrdinal = NETWORK_CATEGORY_ORDINAL,
                retriable = true,
            )

            PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND,
            PlaybackException.ERROR_CODE_IO_READ_POSITION_OUT_OF_RANGE,
            -> NativePlaybackError(
                codeOrdinal = INVALID_SOURCE_ORDINAL,
                categoryOrdinal = SOURCE_CATEGORY_ORDINAL,
                retriable = false,
            )

            PlaybackException.ERROR_CODE_DRM_PROVISIONING_FAILED,
            PlaybackException.ERROR_CODE_DRM_LICENSE_ACQUISITION_FAILED,
            PlaybackException.ERROR_CODE_DRM_SYSTEM_ERROR,
            PlaybackException.ERROR_CODE_DRM_LICENSE_EXPIRED,
            PlaybackException.ERROR_CODE_DRM_UNSPECIFIED,
            PlaybackException.ERROR_CODE_DRM_CONTENT_ERROR,
            -> NativePlaybackError(
                codeOrdinal = BACKEND_FAILURE_ORDINAL,
                categoryOrdinal = NETWORK_CATEGORY_ORDINAL,
                retriable = true,
            )

            PlaybackException.ERROR_CODE_IO_NO_PERMISSION,
            PlaybackException.ERROR_CODE_IO_CLEARTEXT_NOT_PERMITTED,
            PlaybackException.ERROR_CODE_DRM_SCHEME_UNSUPPORTED,
            PlaybackException.ERROR_CODE_DRM_DISALLOWED_OPERATION,
            PlaybackException.ERROR_CODE_DRM_DEVICE_REVOKED,
            -> NativePlaybackError(
                codeOrdinal = UNSUPPORTED_ORDINAL,
                categoryOrdinal = CAPABILITY_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
            )

            PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED,
            -> NativePlaybackError(
                codeOrdinal = UNSUPPORTED_ORDINAL,
                categoryOrdinal = CAPABILITY_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.ContainerUnsupported,
                capabilityFailureAxis = AndroidCapabilityFailureAxis.Container,
            )

            PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED,
            -> NativePlaybackError(
                codeOrdinal = UNSUPPORTED_ORDINAL,
                categoryOrdinal = CAPABILITY_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.ManifestUnsupported,
                capabilityFailureAxis = AndroidCapabilityFailureAxis.Manifest,
            )

            PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED,
            PlaybackException.ERROR_CODE_PARSING_MANIFEST_MALFORMED,
            -> NativePlaybackError(
                codeOrdinal = INVALID_SOURCE_ORDINAL,
                categoryOrdinal = SOURCE_CATEGORY_ORDINAL,
                retriable = false,
            )

            PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
            -> NativePlaybackError(
                codeOrdinal = DECODE_FAILURE_ORDINAL,
                categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.DecoderInit,
                capabilityFailureAxis = causeEvidence.runtimeFailureAxis()
                    ?: AndroidCapabilityFailureAxis.Decoder,
            )

            PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED,
            -> NativePlaybackError(
                codeOrdinal = DECODE_FAILURE_ORDINAL,
                categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.DecoderQuery,
                capabilityFailureAxis = AndroidCapabilityFailureAxis.Decoder,
            )

            PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED,
            -> NativePlaybackError(
                codeOrdinal = UNSUPPORTED_ORDINAL,
                categoryOrdinal = CAPABILITY_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.FormatUnsupported,
                capabilityFailureAxis = AndroidCapabilityFailureAxis.Decoder,
            )

            PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES,
            -> NativePlaybackError(
                codeOrdinal = UNSUPPORTED_ORDINAL,
                categoryOrdinal = CAPABILITY_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.FormatExceedsCapabilities,
                capabilityFailureAxis = AndroidCapabilityFailureAxis.Decoder,
            )

            PlaybackException.ERROR_CODE_DECODING_FAILED,
            -> NativePlaybackError(
                codeOrdinal = DECODE_FAILURE_ORDINAL,
                categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                retriable = false,
                likelyCapabilityIssue = true,
                capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                capabilityFailureAxis = causeEvidence.runtimeFailureAxis()
                    ?: AndroidCapabilityFailureAxis.Decoder,
            )

            PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED,
            PlaybackException.ERROR_CODE_AUDIO_TRACK_WRITE_FAILED,
            PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_INIT_FAILED,
            PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_WRITE_FAILED,
            -> NativePlaybackError(
                codeOrdinal = AUDIO_OUTPUT_UNAVAILABLE_ORDINAL,
                categoryOrdinal = AUDIO_OUTPUT_CATEGORY_ORDINAL,
                retriable = false,
            )

            else ->
                NativePlaybackError(
                    codeOrdinal = BACKEND_FAILURE_ORDINAL,
                    categoryOrdinal = PLATFORM_CATEGORY_ORDINAL,
                    retriable = false,
                )
        }
    }
    return classified.copy(causeEvidence = causeEvidence)
}

internal fun AndroidPlaybackFailureCauseEvidence?.runtimeFailureAxis(): AndroidCapabilityFailureAxis? {
    this ?: return null
    rendererFormatSupport?.lowercase()?.let { support ->
        if (support != "handled") {
            return AndroidCapabilityFailureAxis.Decoder
        }
    }
    rendererName?.lowercase()?.let { name ->
        if (name.contains("video")) {
            return AndroidCapabilityFailureAxis.Renderer
        }
    }
    val haystack =
        listOfNotNull(
            causeClass,
            causeMessage,
            rootCauseClass,
            rootCauseMessage,
        ).joinToString(separator = " ").lowercase()
    return when {
        haystack.contains("surface") ||
            haystack.contains("egl") ||
            haystack.contains("glsurface") ||
            haystack.contains("display") -> AndroidCapabilityFailureAxis.DisplaySurface
        haystack.contains("renderer") ||
            haystack.contains("mediacodecvideorenderer") -> AndroidCapabilityFailureAxis.Renderer
        haystack.contains("mediacodec") ||
            haystack.contains("decoder") ||
            haystack.contains("codec") -> AndroidCapabilityFailureAxis.Decoder
        else -> null
    }
}

internal fun Throwable.hasCause(type: Class<out Throwable>): Boolean {
    var current: Throwable? = this
    while (current != null) {
        if (type.isInstance(current)) {
            return true
        }
        current = current.cause
    }
    return false
}

internal fun PlaybackException.playbackFailureCauseEvidence(): AndroidPlaybackFailureCauseEvidence? {
    val directCause = cause ?: return null
    val rootCause = directCause.rootCause()
    val exoError = this as? ExoPlaybackException
    val rendererFormat = exoError?.rendererFormat
    return AndroidPlaybackFailureCauseEvidence(
        causeClass = directCause.javaClass.name,
        causeMessage = directCause.message?.boundedFailureMessage(),
        rootCauseClass = if (rootCause !== directCause) rootCause.javaClass.name else null,
        rootCauseMessage = if (rootCause !== directCause) {
            rootCause.message?.boundedFailureMessage()
        } else {
            null
        },
        rendererName = exoError?.rendererName,
        rendererIndex = exoError?.rendererIndex?.takeIf { it >= 0 },
        rendererFormatSupport = exoError?.rendererFormatSupport?.formatSupportName(),
        rendererFormatSampleMimeType = rendererFormat?.sampleMimeType,
        rendererFormatCodecs = rendererFormat?.codecs,
        rendererFormatWidth = rendererFormat?.width?.takeIf { it > 0 },
        rendererFormatHeight = rendererFormat?.height?.takeIf { it > 0 },
        rendererFormatFrameRate = rendererFormat?.frameRate?.takeIf { it > 0f },
    )
}

internal fun Int.formatSupportName(): String =
    when (this) {
        C.FORMAT_HANDLED -> "handled"
        C.FORMAT_EXCEEDS_CAPABILITIES -> "exceedsCapabilities"
        C.FORMAT_UNSUPPORTED_DRM -> "unsupportedDrm"
        C.FORMAT_UNSUPPORTED_SUBTYPE -> "unsupportedSubtype"
        C.FORMAT_UNSUPPORTED_TYPE -> "unsupportedType"
        else -> toString()
    }

internal fun Throwable.rootCause(): Throwable {
    var current = this
    while (current.cause != null && current.cause !== current) {
        current = current.cause ?: break
    }
    return current
}

internal fun String.boundedFailureMessage(): String =
    if (length <= MAX_PLAYBACK_FAILURE_CAUSE_MESSAGE_CHARS) {
        this
    } else {
        take(MAX_PLAYBACK_FAILURE_CAUSE_MESSAGE_CHARS - 3) + "..."
    }

internal const val MAX_PLAYBACK_FAILURE_CAUSE_MESSAGE_CHARS = 256

internal const val INVALID_SOURCE_ORDINAL = 2
internal const val BACKEND_FAILURE_ORDINAL = 3
internal const val AUDIO_OUTPUT_UNAVAILABLE_ORDINAL = 4
internal const val DECODE_FAILURE_ORDINAL = 5
internal const val UNSUPPORTED_ORDINAL = 7
internal const val SOURCE_CATEGORY_ORDINAL = 1
internal const val NETWORK_CATEGORY_ORDINAL = 2
internal const val DECODE_CATEGORY_ORDINAL = 3
internal const val AUDIO_OUTPUT_CATEGORY_ORDINAL = 4
internal const val CAPABILITY_CATEGORY_ORDINAL = 6
internal const val PLATFORM_CATEGORY_ORDINAL = 7
internal const val NATIVE_JNI_BINDINGS_TAG = "VesperPlayerAndroidHost"
internal val FORMAT_NO_VALUE_FLOAT = Format.NO_VALUE.toFloat()

internal fun exoPlaybackStateName(playbackState: Int): String =
    when (playbackState) {
        Player.STATE_IDLE -> "IDLE"
        Player.STATE_BUFFERING -> "BUFFERING"
        Player.STATE_READY -> "READY"
        Player.STATE_ENDED -> "ENDED"
        else -> "UNKNOWN($playbackState)"
    }

internal fun inferSourceKind(uri: String): VesperPlayerSourceKind =
    if (
        uri.startsWith("file://", ignoreCase = true) ||
            uri.startsWith("content://", ignoreCase = true) ||
            uri.startsWith("/") ||
            (!uri.contains("://") && !uri.startsWith("content:", ignoreCase = true))
    ) {
        VesperPlayerSourceKind.Local
    } else {
        VesperPlayerSourceKind.Remote
    }

internal fun inferSourceProtocol(uri: String): VesperPlayerSourceProtocol {
    val normalized = uri.lowercase()
    val normalizedPath = normalized.substringBefore('#').substringBefore('?')
    return when {
        normalized.startsWith("file://") || uri.startsWith("/") -> VesperPlayerSourceProtocol.File
        normalized.startsWith("content://") -> VesperPlayerSourceProtocol.Content
        normalizedPath.endsWith(".m3u8") -> VesperPlayerSourceProtocol.Hls
        normalizedPath.endsWith(".mpd") -> VesperPlayerSourceProtocol.Dash
        normalized.startsWith("http://") || normalized.startsWith("https://") -> VesperPlayerSourceProtocol.Progressive
        else -> VesperPlayerSourceProtocol.Unknown
    }
}

internal const val DEFAULT_PRELOAD_WARMUP_READ_BYTES = 32 * 1024
