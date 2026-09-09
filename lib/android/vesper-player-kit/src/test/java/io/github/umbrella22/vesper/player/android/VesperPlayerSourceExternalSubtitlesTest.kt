package io.github.umbrella22.vesper.player.android

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
    fun sourceProtocolWireValuesStayStableAndUnknownValuesFailClosed() {
        val expected =
            listOf(
                VesperPlayerSourceProtocol.Unknown to 0,
                VesperPlayerSourceProtocol.File to 1,
                VesperPlayerSourceProtocol.Content to 2,
                VesperPlayerSourceProtocol.Progressive to 3,
                VesperPlayerSourceProtocol.Hls to 4,
                VesperPlayerSourceProtocol.Dash to 5,
                VesperPlayerSourceProtocol.Rtmp to 6,
                VesperPlayerSourceProtocol.Rtsp to 7,
                VesperPlayerSourceProtocol.Flv to 8,
            )

        expected.forEach { (protocol, wireValue) ->
            assertEquals(wireValue, protocol.wireValue)
            assertEquals(protocol, VesperPlayerSourceProtocol.fromWireValue(wireValue))
        }
        assertEquals(
            VesperPlayerSourceProtocol.Unknown,
            VesperPlayerSourceProtocol.fromWireValue(Int.MAX_VALUE),
        )
    }

    @Test
    fun sourceKindWireValuesStayStableAndUnknownValuesRemainRemote() {
        assertEquals(0, VesperPlayerSourceKind.Local.wireValue)
        assertEquals(1, VesperPlayerSourceKind.Remote.wireValue)
        assertEquals(VesperPlayerSourceKind.Local, VesperPlayerSourceKind.fromWireValue(0))
        assertEquals(VesperPlayerSourceKind.Remote, VesperPlayerSourceKind.fromWireValue(1))
        assertEquals(
            VesperPlayerSourceKind.Remote,
            VesperPlayerSourceKind.fromWireValue(Int.MAX_VALUE),
        )
    }

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
