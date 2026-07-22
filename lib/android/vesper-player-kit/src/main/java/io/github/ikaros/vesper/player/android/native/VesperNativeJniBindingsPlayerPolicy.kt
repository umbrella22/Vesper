package io.github.ikaros.vesper.player.android

import android.content.Context
import android.net.Uri
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
import androidx.media3.exoplayer.source.MediaSource
import androidx.media3.exoplayer.source.SingleSampleMediaSource
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy
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

internal val NativeVideoSurfaceKind.nativeFramePresenterProfileWireName: String
    get() =
        when (this) {
            NativeVideoSurfaceKind.SurfaceView -> "SurfaceView"
            NativeVideoSurfaceKind.TextureView -> "SurfaceTexture"
        }

internal fun buildLoadControl(
    bufferingPolicy: NativeBufferingPolicy,
): DefaultLoadControl {
    val builder = DefaultLoadControl.Builder()
    val resolved = resolveBufferingPolicy(bufferingPolicy) ?: return builder.build()
    return builder
        .setBufferDurationsMs(
            resolved.minBufferMs,
            resolved.maxBufferMs,
            resolved.bufferForPlaybackMs,
            resolved.bufferForPlaybackAfterRebufferMs,
        )
        .build()
}

internal fun buildLoadErrorHandlingPolicy(
    source: VesperPlayerSource,
    retryPolicy: NativeRetryPolicy,
    onRetryScheduled: (attempt: Int, delayMs: Long) -> Unit,
): DefaultLoadErrorHandlingPolicy =
    when (source.kind) {
        VesperPlayerSourceKind.Local -> DefaultLoadErrorHandlingPolicy(0)
        VesperPlayerSourceKind.Remote -> VesperLoadErrorHandlingPolicy(retryPolicy, onRetryScheduled)
    }

internal fun resolveBufferingPolicy(
    bufferingPolicy: NativeBufferingPolicy,
): ResolvedBufferingPolicy? {
    val minBufferMs = bufferingPolicy.minBufferMs.takeIf { bufferingPolicy.hasMinBufferMs }
    val maxBufferMs = bufferingPolicy.maxBufferMs.takeIf { bufferingPolicy.hasMaxBufferMs }
    val bufferForPlaybackMs =
        bufferingPolicy.bufferForPlaybackMs.takeIf { bufferingPolicy.hasBufferForPlaybackMs }
    val bufferForPlaybackAfterRebufferMs =
        bufferingPolicy.bufferForPlaybackAfterRebufferMs.takeIf {
            bufferingPolicy.hasBufferForPlaybackAfterRebufferMs
        }

    if (
        minBufferMs == null ||
        maxBufferMs == null ||
        bufferForPlaybackMs == null ||
        bufferForPlaybackAfterRebufferMs == null
    ) {
        return null
    }

    return ResolvedBufferingPolicy(
        minBufferMs = minBufferMs.coerceAtLeast(0),
        maxBufferMs = maxBufferMs.coerceAtLeast(minBufferMs),
        bufferForPlaybackMs = bufferForPlaybackMs.coerceAtLeast(0),
        bufferForPlaybackAfterRebufferMs = bufferForPlaybackAfterRebufferMs.coerceAtLeast(0),
    )
}

internal data class ResolvedBufferingPolicy(
    val minBufferMs: Int,
    val maxBufferMs: Int,
    val bufferForPlaybackMs: Int,
    val bufferForPlaybackAfterRebufferMs: Int,
)

internal fun media3MinimumRetryCount(retryPolicy: NativeRetryPolicy): Int {
    val maxAttempts = retryPolicy.resolvedMaxAttempts()
    return when {
        maxAttempts == null -> Int.MAX_VALUE
        maxAttempts <= 0 -> 0
        else -> maxAttempts
    }
}

internal fun NativeRetryPolicy.resolvedMaxAttempts(): Int? =
    when {
        usesDefaultMaxAttempts -> 3
        hasMaxAttempts -> maxAttempts
        else -> null
    }

internal class VesperLoadErrorHandlingPolicy(
    private val retryPolicy: NativeRetryPolicy,
    private val onRetryScheduled: (attempt: Int, delayMs: Long) -> Unit,
) : DefaultLoadErrorHandlingPolicy(media3MinimumRetryCount(retryPolicy)) {
    override fun getRetryDelayMsFor(loadErrorInfo: LoadErrorInfo): Long {
        val superDelayMs = super.getRetryDelayMsFor(loadErrorInfo)
        if (superDelayMs == C.TIME_UNSET) {
            return C.TIME_UNSET
        }

        val maxAttempts = retryPolicy.resolvedMaxAttempts()
        if (maxAttempts != null && loadErrorInfo.errorCount > maxAttempts) {
            return C.TIME_UNSET
        }

        val backoff =
            if (retryPolicy.hasBackoff) {
                VesperRetryBackoff.entries.getOrElse(retryPolicy.backoffOrdinal) {
                    VesperRetryBackoff.Linear
                }
            } else {
                VesperRetryBackoff.Linear
            }
        val step = when (backoff) {
            VesperRetryBackoff.Fixed -> 1.0
            VesperRetryBackoff.Linear -> loadErrorInfo.errorCount.toDouble()
            VesperRetryBackoff.Exponential ->
                2.0.pow((loadErrorInfo.errorCount - 1).coerceAtLeast(0).toDouble())
        }
        val baseDelayMs = retryPolicy.baseDelayMs.takeIf { retryPolicy.hasBaseDelayMs } ?: 1_000L
        val maxDelayMs = retryPolicy.maxDelayMs.takeIf { retryPolicy.hasMaxDelayMs } ?: 5_000L
        val computedDelay = (baseDelayMs.toDouble() * step).roundToLong()
        val resolvedDelay = computedDelay.coerceAtMost(maxDelayMs).coerceAtLeast(0L)
        onRetryScheduled(loadErrorInfo.errorCount, resolvedDelay)
        return resolvedDelay
    }
}

internal fun VideoSize.toNativeVideoLayoutInfo(): NativeVideoLayoutInfo? {
    if (width <= 0 || height <= 0) {
        return null
    }

    return NativeVideoLayoutInfo(
        width = width,
        height = height,
        pixelWidthHeightRatio = pixelWidthHeightRatio.takeIf { it > 0f } ?: 1.0f,
    )
}

internal fun exoPlaybackStateOrdinal(playbackState: Int): Int =
    when (playbackState) {
        Player.STATE_BUFFERING -> 1
        Player.STATE_READY -> 2
        Player.STATE_ENDED -> 3
        else -> 0
    }

internal fun buildMediaItem(source: VesperPlayerSource): MediaItem {
    val builder = MediaItem.Builder()
        .setUri(source.uri)

    when (source.protocol) {
        VesperPlayerSourceProtocol.Hls -> builder.setMimeType(MimeTypes.APPLICATION_M3U8)
        VesperPlayerSourceProtocol.Dash -> builder.setMimeType(MimeTypes.APPLICATION_MPD)
        // FLV streams are handled by the built-in FlvExtractor (media3-extractor)
        // via ProgressiveMediaSource. ExoPlayer auto-detects the container from the
        // `.flv` extension, so no explicit MIME type is needed; setting it to null
        // keeps the auto-detection path intact.
        VesperPlayerSourceProtocol.Flv -> Unit
        // RTMP requires the optional media3-exoplayer-rtmp extension. When the host
        // has not bundled it, reject explicitly instead of letting ExoPlayer fall
        // through to an unsupported-source error deep inside the pipeline.
        VesperPlayerSourceProtocol.Rtmp -> throw VesperPlayerUnsupportedOperation(
            "RTMP playback is not implemented by the stable Android host kit.",
            mapOf(
                "reason" to "rtmpUnsupported",
                "route" to "direct",
                "protocol" to "rtmp",
            ),
        )
        VesperPlayerSourceProtocol.Rtsp -> throw VesperPlayerUnsupportedOperation(
            "RTSP playback requires the optional media3-exoplayer-rtsp extension. " +
                "Add it to your app's dependencies; Vesper does not bundle it by default.",
            mapOf(
                "reason" to "rtspExtensionRequired",
                "route" to "direct",
                "protocol" to "rtsp",
            ),
        )
        else -> Unit
    }

    buildWidevineDrmConfiguration(source)?.let(builder::setDrmConfiguration)

    // Side-loaded external subtitles (SRT/ASS/WebVTT). ExoPlayer's TextRenderer
    // parses and renders them; Vesper only forwards the URIs and MIME types.
    if (source.externalSubtitles.isNotEmpty()) {
        val ids = source.externalSubtitles.map { it.id }
        if (ids.any(String::isBlank) || ids.toSet().size != ids.size) {
            throw VesperPlayerUnsupportedOperation(
                "External subtitle ids must be non-empty and unique within a source.",
                mapOf(
                    "domain" to "subtitle",
                    "code" to "subtitle_track_identity_ambiguous",
                    "phase" to "identity",
                    "trackId" to null,
                    "retriable" to false,
                    "message" to "external subtitle ids must be non-empty and unique within a source",
                ),
            )
        }
        if (source.externalSubtitles.count { it.isDefault } > 1) {
            throw VesperPlayerUnsupportedOperation(
                "A subtitle group may contain at most one default track.",
                mapOf(
                    "domain" to "subtitle",
                    "code" to "subtitle_default_track_ambiguous",
                    "phase" to "identity",
                    "trackId" to null,
                    "retriable" to false,
                    "message" to "a subtitle group may contain at most one default track",
                ),
            )
        }
        builder.setSubtitleConfigurations(source.externalSubtitles.map(::buildExternalSubtitleConfiguration))
    }

    return builder.build()
}

internal fun buildExternalSubtitleConfiguration(
    source: VesperExternalSubtitleSource,
): MediaItem.SubtitleConfiguration =
    MediaItem.SubtitleConfiguration.Builder(Uri.parse(source.uri))
        .setId(source.id)
        .setMimeType(source.mimeType)
        .apply {
            var flags = 0
            if (source.isDefault) flags = flags or C.SELECTION_FLAG_DEFAULT
            if (source.isForced) flags = flags or C.SELECTION_FLAG_FORCED
            setSelectionFlags(flags)
            source.language?.let(::setLanguage)
            source.label?.let(::setLabel)
        }
        .build()

internal data class PreparedExternalSubtitleMediaSources(
    val mediaSources: List<MediaSource>,
    val activeSources: List<VesperExternalSubtitleSource>,
    val failures: List<NativeTrackSelectionFailure>,
)

internal fun prepareExternalSubtitleMediaSources(
    appContext: Context,
    cachePolicy: NativeCachePolicy,
    sources: List<VesperExternalSubtitleSource>,
    loadErrorHandlingPolicy: LoadErrorHandlingPolicy,
    primaryUri: String? = null,
    primaryHeaders: Map<String, String> = emptyMap(),
): PreparedExternalSubtitleMediaSources {
    if (sources.isEmpty()) return PreparedExternalSubtitleMediaSources(emptyList(), emptyList(), emptyList())

    val failures = mutableListOf<NativeTrackSelectionFailure>()
    val ids = sources.map { it.id }
    if (ids.any(String::isBlank) || ids.toSet().size != ids.size) {
        failures += NativeTrackSelectionFailure(
            kind = NativeTrackKind.Subtitle,
            trackId = ids.firstOrNull { id -> id.isBlank() || ids.count { it == id } > 1 },
            code = "subtitle_track_identity_ambiguous",
            phase = "identity",
            message = "external subtitle ids must be non-blank and unique within a source",
            advertisedTrackCount = sources.size,
        )
        return PreparedExternalSubtitleMediaSources(emptyList(), emptyList(), failures)
    }
    if (sources.count { it.isDefault } > 1) {
        failures += NativeTrackSelectionFailure(
            kind = NativeTrackKind.Subtitle,
            trackId = null,
            code = "subtitle_default_track_ambiguous",
            phase = "identity",
            message = "a subtitle group may contain at most one default track",
            advertisedTrackCount = sources.size,
        )
        return PreparedExternalSubtitleMediaSources(emptyList(), emptyList(), failures)
    }

    val conflictingUris =
        sources
            .groupBy { it.uri }
            .filterValues { group -> group.map { it.headers }.distinct().size > 1 }
            .keys
    val activeSources = mutableListOf<VesperExternalSubtitleSource>()
    val mediaSources = mutableListOf<MediaSource>()
    for (source in sources) {
        val conflictsWithPrimaryRequest =
            primaryUri != null &&
                source.uri == primaryUri &&
                source.headers != primaryHeaders
        if (source.uri in conflictingUris || conflictsWithPrimaryRequest) {
            failures += NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = source.id,
                code = "subtitle_request_identity_ambiguous",
                phase = "resource",
                message = "resources sharing a URI must use identical request headers",
                advertisedTrackCount = sources.size,
            )
            continue
        }
        val uri = Uri.parse(source.uri)
        val supportedScheme = uri.scheme?.lowercase() in setOf("file", "content", "http", "https", "android.resource")
        if (source.uri.isBlank() || !supportedScheme) {
            failures += NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = source.id,
                code = "subtitle_uri_invalid",
                phase = "resource",
                message = "external subtitle URI is invalid or unsupported",
                advertisedTrackCount = sources.size,
            )
            continue
        }
        try {
            val dataSourceFactory = buildDataSourceFactory(appContext, cachePolicy, source.headers)
            mediaSources +=
                SingleSampleMediaSource.Factory(dataSourceFactory)
                    .setTrackId(source.id)
                    .setLoadErrorHandlingPolicy(loadErrorHandlingPolicy)
                    .setTreatLoadErrorsAsEndOfStream(true)
                    .createMediaSource(buildExternalSubtitleConfiguration(source), C.TIME_UNSET)
            activeSources += source
        } catch (_: RuntimeException) {
            failures += NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = source.id,
                code = "subtitle_resource_failed",
                phase = "resource",
                message = "external subtitle media source could not be prepared",
                advertisedTrackCount = sources.size,
            )
        }
    }
    return PreparedExternalSubtitleMediaSources(mediaSources, activeSources, failures)
}

internal fun buildWidevineDrmConfiguration(source: VesperPlayerSource): MediaItem.DrmConfiguration? {
    val drmConfiguration = source.drmConfiguration ?: return null
    if (!drmConfiguration.keySystem.equals("widevine", ignoreCase = true)) {
        return null
    }
    val licenseUriText =
        drmConfiguration.licenseUri.takeIf { it.isNotBlank() }
            ?: throw VesperPlayerUnsupportedOperation(
                "Widevine DRM requires a non-empty license URI.",
                drmUnsupportedRouteDetails(source, route = "direct", reason = "drmLicenseUriMissing"),
            )
    val builder = MediaItem.DrmConfiguration.Builder(C.WIDEVINE_UUID)
        .setLicenseRequestHeaders(drmConfiguration.licenseHeaders)
        .setMultiSession(drmConfiguration.multiSession)
    val parsedLicenseUri = parseAndroidUriForDrm(licenseUriText)
    if (parsedLicenseUri != null) {
        builder
            .setLicenseUri(parsedLicenseUri)
            .setForceDefaultLicenseUri(true)
    } else {
        // Local JVM tests use Android stubs where Uri.parse may be unavailable.
        builder.setLicenseUri(licenseUriText)
    }
    return builder.build()
}

internal fun parseAndroidUriForDrm(uri: String): Uri? =
    runCatching { Uri.parse(uri) }.getOrNull()
