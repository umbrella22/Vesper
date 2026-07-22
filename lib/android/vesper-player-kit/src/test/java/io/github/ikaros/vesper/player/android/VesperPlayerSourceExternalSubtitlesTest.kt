package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Test

class VesperPlayerSourceExternalSubtitlesTest {
    private val subtitle =
        VesperExternalSubtitleSource(
            id = "external-en",
            uri = "https://example.com/subtitle.vtt",
            mimeType = VesperExternalSubtitleSource.MIME_WEBVTT,
        )

    @Test
    fun convenienceFactoriesPreserveExternalSubtitles() {
        val expected = listOf(subtitle)
        val sources =
            listOf(
                VesperPlayerSource.local("file:///video.mp4", "local", externalSubtitles = expected),
                VesperPlayerSource.localDash("file:///video.mpd", "local dash", externalSubtitles = expected),
                VesperPlayerSource.remote("https://example.com/video.mp4", "remote", externalSubtitles = expected),
                VesperPlayerSource.hls("https://example.com/master.m3u8", "hls", externalSubtitles = expected),
                VesperPlayerSource.dash("https://example.com/manifest.mpd", "dash", externalSubtitles = expected),
                VesperPlayerSource.rtmp("rtmp://example.com/live", "rtmp", externalSubtitles = expected),
                VesperPlayerSource.rtsp("rtsp://example.com/live", "rtsp", externalSubtitles = expected),
                VesperPlayerSource.flvLive("https://example.com/live.flv", "flv", externalSubtitles = expected),
            )

        sources.forEach { source -> assertEquals(expected, source.externalSubtitles) }
    }
}
