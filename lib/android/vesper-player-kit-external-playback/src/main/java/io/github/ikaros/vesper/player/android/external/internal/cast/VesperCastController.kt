package io.github.ikaros.vesper.player.android.external.internal.cast

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
    val contentType = source.castContentType()
    return MediaInfo.Builder(source.uri)
        .setStreamType(streamType)
        .setContentType(contentType)
        .setMetadata(metadata.toCastMetadata(source, contentType))
        .build()
}

private fun VesperSystemPlaybackMetadata?.toCastMetadata(
    source: VesperPlayerSource,
    contentType: String,
): MediaMetadata {
    val metadata = MediaMetadata(contentType.castMetadataType())
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
    listOf(uri, label)
        .firstNotNullOfOrNull { it.mimeTypeFromPath() }
        ?: when (protocol) {
            VesperPlayerSourceProtocol.Hls -> "application/x-mpegURL"
            VesperPlayerSourceProtocol.Dash -> "application/dash+xml"
            VesperPlayerSourceProtocol.Progressive -> "video/mp4"
            else -> "application/octet-stream"
        }

private fun String.castMetadataType(): Int =
    when {
        startsWith("audio/", ignoreCase = true) -> MediaMetadata.MEDIA_TYPE_MUSIC_TRACK
        startsWith("image/", ignoreCase = true) -> MediaMetadata.MEDIA_TYPE_PHOTO
        else -> MediaMetadata.MEDIA_TYPE_MOVIE
    }

private fun String.mimeTypeFromPath(): String? {
    val path = substringBefore('?').substringBefore('#').lowercase()
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
