package io.github.ikaros.vesper.player.android.cast

import android.content.Context
import android.net.Uri
import com.google.android.gms.cast.MediaInfo
import com.google.android.gms.cast.MediaLoadRequestData
import com.google.android.gms.cast.MediaMetadata
import com.google.android.gms.cast.MediaSeekOptions
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.common.images.WebImage
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata

class VesperCastController(context: Context) {
    private val appContext = context.applicationContext

    fun isCastSessionAvailable(): Boolean =
        castContextOrNull()
            ?.sessionManager
            ?.currentCastSession
            ?.remoteMediaClient != null

    fun load(request: VesperCastLoadRequest): VesperCastOperationResult {
        val validationError = request.source.unsupportedCastReason()
        if (validationError != null) {
            return VesperCastOperationResult.Unsupported(validationError)
        }

        val remoteClient =
            castContextOrNull()
                ?.sessionManager
                ?.currentCastSession
                ?.remoteMediaClient
                ?: return VesperCastOperationResult.Unavailable("No active Cast session.")

        val mediaInfo = request.toMediaInfo()
        val loadRequest =
            MediaLoadRequestData.Builder()
                .setMediaInfo(mediaInfo)
                .setAutoplay(request.autoplay)
                .setCurrentTime(request.startPositionMs.coerceAtLeast(0L))
                .build()
        remoteClient.load(loadRequest)
        return VesperCastOperationResult.Success
    }

    fun play(): VesperCastOperationResult =
        withRemoteClient { play() }

    fun pause(): VesperCastOperationResult =
        withRemoteClient { pause() }

    fun stop(): VesperCastOperationResult =
        withRemoteClient { stop() }

    fun seekTo(positionMs: Long): VesperCastOperationResult =
        withRemoteClient {
            seek(
                MediaSeekOptions.Builder()
                    .setPosition(positionMs.coerceAtLeast(0L))
                    .build(),
            )
        }

    private fun withRemoteClient(block: com.google.android.gms.cast.framework.media.RemoteMediaClient.() -> Unit): VesperCastOperationResult {
        val remoteClient =
            castContextOrNull()
                ?.sessionManager
                ?.currentCastSession
                ?.remoteMediaClient
                ?: return VesperCastOperationResult.Unavailable("No active Cast session.")
        remoteClient.block()
        return VesperCastOperationResult.Success
    }

    private fun castContextOrNull(): CastContext? =
        runCatching { CastContext.getSharedInstance(appContext) }.getOrNull()
}

data class VesperCastLoadRequest(
    val source: VesperPlayerSource,
    val metadata: VesperSystemPlaybackMetadata? = null,
    val startPositionMs: Long = 0,
    val autoplay: Boolean = true,
)

sealed class VesperCastOperationResult {
    data object Success : VesperCastOperationResult()
    data class Unavailable(val message: String) : VesperCastOperationResult()
    data class Unsupported(val message: String) : VesperCastOperationResult()
}

private fun VesperCastLoadRequest.toMediaInfo(): MediaInfo {
    val streamType =
        if (metadata?.isLive == true) {
            MediaInfo.STREAM_TYPE_LIVE
        } else {
            MediaInfo.STREAM_TYPE_BUFFERED
        }
    return MediaInfo.Builder(source.uri)
        .setStreamType(streamType)
        .setContentType(source.castContentType())
        .setMetadata(metadata.toCastMetadata(source))
        .build()
}

private fun VesperSystemPlaybackMetadata?.toCastMetadata(
    source: VesperPlayerSource,
): MediaMetadata {
    val metadata = MediaMetadata(MediaMetadata.MEDIA_TYPE_MOVIE)
    metadata.putString(MediaMetadata.KEY_TITLE, this?.title?.takeIf(String::isNotBlank) ?: source.label)
    this?.artist?.takeIf(String::isNotBlank)?.let {
        metadata.putString(MediaMetadata.KEY_ARTIST, it)
    }
    this?.albumTitle?.takeIf(String::isNotBlank)?.let {
        metadata.putString(MediaMetadata.KEY_ALBUM_TITLE, it)
    }
    this?.artworkUri
        ?.takeIf(String::isNotBlank)
        ?.let(Uri::parse)
        ?.let(::WebImage)
        ?.let(metadata::addImage)
    return metadata
}

private fun VesperPlayerSource.castContentType(): String =
    when (protocol) {
        VesperPlayerSourceProtocol.Hls -> "application/x-mpegURL"
        VesperPlayerSourceProtocol.Dash -> "application/dash+xml"
        VesperPlayerSourceProtocol.Progressive -> "video/mp4"
        else -> "application/octet-stream"
    }

private fun VesperPlayerSource.unsupportedCastReason(): String? {
    val parsedUri = runCatching { Uri.parse(uri) }.getOrNull()
    val scheme = parsedUri?.scheme?.lowercase()
    if (kind != VesperPlayerSourceKind.Remote || scheme !in setOf("http", "https")) {
        return "Cast V2 supports only remote http/https sources."
    }
    if (protocol !in setOf(
            VesperPlayerSourceProtocol.Hls,
            VesperPlayerSourceProtocol.Dash,
            VesperPlayerSourceProtocol.Progressive,
            VesperPlayerSourceProtocol.Unknown,
        )
    ) {
        return "Cast V2 does not support ${protocol.name} sources."
    }
    if (headers.isNotEmpty()) {
        return "Cast V2 does not support request headers with the default receiver."
    }
    return null
}
