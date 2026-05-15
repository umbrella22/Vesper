package io.github.ikaros.vesper.player.android.external.internal.dlna

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata

object VesperDlnaDidlBuilder {
    fun build(source: VesperPlayerSource, metadata: VesperSystemPlaybackMetadata?): String {
        val title = metadata?.title?.takeIf { it.isNotBlank() } ?: source.label
        val mimeType = source.dlnaMimeType()
        val protocolInfo = mimeType.dlnaProtocolInfo()
        val duration = metadata?.durationMs?.takeIf { it > 0 }?.let(::formatDuration)
        val artwork = metadata?.artworkUri?.takeIf { it.isNotBlank() }
        return buildString {
            append("""<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" """)
            append("""xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" """)
            append("""xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/">""")
            append("""<item id="0" parentID="-1" restricted="1">""")
            append("<dc:title>").append(title.xmlEscaped()).append("</dc:title>")
            append("<upnp:class>").append(mimeType.dlnaUpnpClass()).append("</upnp:class>")
            if (artwork != null) {
                append("<upnp:albumArtURI>").append(artwork.xmlEscaped()).append("</upnp:albumArtURI>")
            }
            append("<res protocolInfo=\"").append(protocolInfo.xmlEscaped()).append("\"")
            if (duration != null) {
                append(" duration=\"").append(duration).append("\"")
            }
            append(">").append(source.uri.xmlEscaped()).append("</res>")
            append("</item></DIDL-Lite>")
        }
    }
}

fun VesperPlayerSource.dlnaMimeType(): String =
    when (protocol) {
        VesperPlayerSourceProtocol.Hls -> "application/x-mpegURL"
        VesperPlayerSourceProtocol.Dash -> "application/dash+xml"
        VesperPlayerSourceProtocol.Progressive,
        VesperPlayerSourceProtocol.File,
        VesperPlayerSourceProtocol.Content,
        VesperPlayerSourceProtocol.Unknown,
        -> {
            listOf(uri, label)
                .firstNotNullOfOrNull { it.mimeTypeFromPath() }
                ?: "video/mp4"
        }
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

private fun String.dlnaUpnpClass(): String =
    when {
        startsWith("image/", ignoreCase = true) -> "object.item.imageItem.photo"
        startsWith("audio/", ignoreCase = true) -> "object.item.audioItem.musicTrack"
        else -> "object.item.videoItem"
    }

private fun String.dlnaProtocolInfo(): String {
    val extras = when {
        equals("image/jpeg", ignoreCase = true) ->
            "DLNA.ORG_PN=JPEG_SM;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=00D00000000000000000000000000000"
        equals("image/png", ignoreCase = true) ->
            "DLNA.ORG_PN=PNG_LRG;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=00D00000000000000000000000000000"
        startsWith("image/", ignoreCase = true) ->
            "DLNA.ORG_CI=0;DLNA.ORG_FLAGS=00D00000000000000000000000000000"
        else ->
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01500000000000000000000000000000"
    }
    return "http-get:*:$this:$extras"
}

internal fun String.xmlEscaped(): String =
    replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")

private fun formatDuration(durationMs: Long): String {
    val totalSeconds = durationMs / 1000
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return "%d:%02d:%02d".format(hours, minutes, seconds)
}
