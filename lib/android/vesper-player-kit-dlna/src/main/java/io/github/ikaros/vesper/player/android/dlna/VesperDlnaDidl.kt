package io.github.ikaros.vesper.player.android.dlna

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata

object VesperDlnaDidlBuilder {
    fun build(source: VesperPlayerSource, metadata: VesperSystemPlaybackMetadata?): String {
        val title = metadata?.title?.takeIf { it.isNotBlank() } ?: source.label
        val protocolInfo = "http-get:*:${source.dlnaMimeType()}:*"
        val duration = metadata?.durationMs?.takeIf { it > 0 }?.let(::formatDuration)
        val artwork = metadata?.artworkUri?.takeIf { it.isNotBlank() }
        return buildString {
            append("""<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" """)
            append("""xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" """)
            append("""xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/">""")
            append("""<item id="0" parentID="-1" restricted="1">""")
            append("<dc:title>").append(title.xmlEscaped()).append("</dc:title>")
            append("<upnp:class>object.item.videoItem.movie</upnp:class>")
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
        VesperPlayerSourceProtocol.Hls -> "application/vnd.apple.mpegurl"
        VesperPlayerSourceProtocol.Dash -> "application/dash+xml"
        VesperPlayerSourceProtocol.Progressive,
        VesperPlayerSourceProtocol.File,
        VesperPlayerSourceProtocol.Content,
        VesperPlayerSourceProtocol.Unknown,
        -> {
            val path = uri.substringBefore('?').substringBefore('#').lowercase()
            when {
                path.endsWith(".m3u8") -> "application/vnd.apple.mpegurl"
                path.endsWith(".mpd") -> "application/dash+xml"
                path.endsWith(".mp3") -> "audio/mpeg"
                path.endsWith(".m4a") -> "audio/mp4"
                else -> "video/mp4"
            }
        }
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
