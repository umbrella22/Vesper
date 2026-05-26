package io.github.ikaros.vesper.player.android

import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import kotlin.io.path.createTempDirectory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperSourceNormalizerLoopbackServerTest {
    @Test
    fun byteRangeParserHandlesOpenEndedAndSuffixRanges() {
        assertEquals(VesperByteRangeRequest(10, 20), parseByteRange("bytes=10-20"))
        assertEquals(VesperByteRangeRequest(10, null), parseByteRange("bytes=10-"))
        assertEquals(VesperByteRangeRequest(null, 8), parseByteRange("bytes=-8"))
        assertEquals(null, parseByteRange("items=0-1"))
        assertEquals(null, parseByteRange("bytes=20-10"))
    }

    @Test
    fun loopbackServerServesPrimaryRange() {
        val directory = createTempDirectory(prefix = "vesper-normalized-loopback").toFile()
        val file = File(directory, "normalized.mp4")
        file.writeBytes("0123456789abcdef".toByteArray())
        val server = VesperSourceNormalizerLoopbackServer()
        try {
            val handle =
                server.register(
                    VesperNormalizedResourceRegistration(
                        outputRoute = "fmp4LocalStream",
                        primaryResourcePath = file.absolutePath,
                        primaryContentType = "video/mp4",
                        sessionReadBufferBytes = 4096,
                    )
                )
            val connection = URL(handle.playbackUri).openConnection() as HttpURLConnection
            connection.setRequestProperty("Range", "bytes=4-7")

            assertEquals(206, connection.responseCode)
            assertEquals("bytes 4-7/16", connection.getHeaderField("Content-Range"))
            assertEquals("4567", connection.inputStream.bufferedReader().readText())
        } finally {
            server.stop()
            directory.deleteRecursively()
        }
    }

    @Test
    fun loopbackServerWaitsForGrowingPrimaryRange() {
        val directory = createTempDirectory(prefix = "vesper-normalized-loopback").toFile()
        val file = File(directory, "normalized.mp4")
        file.writeBytes("0123".toByteArray())
        val server =
            VesperSourceNormalizerLoopbackServer(
                growingReadWaitMillis = 1_000,
                growingReadPollMillis = 10,
            )
        try {
            val handle =
                server.register(
                    VesperNormalizedResourceRegistration(
                        outputRoute = "fmp4LocalStream",
                        primaryResourcePath = file.absolutePath,
                        primaryContentType = "video/mp4",
                        sessionReadBufferBytes = 4096,
                    )
                )
            val writer =
                Thread {
                    Thread.sleep(100)
                    file.appendBytes("4567".toByteArray())
                }
            writer.start()
            val connection = URL(handle.playbackUri).openConnection() as HttpURLConnection
            connection.setRequestProperty("Range", "bytes=4-7")

            assertEquals(206, connection.responseCode)
            assertEquals("bytes 4-7/8", connection.getHeaderField("Content-Range"))
            assertEquals("4567", connection.inputStream.bufferedReader().readText())
            writer.join(1_000)
        } finally {
            server.stop()
            directory.deleteRecursively()
        }
    }

    @Test
    fun loopbackServerStreamsGrowingPrimaryWithoutContentLength() {
        val directory = createTempDirectory(prefix = "vesper-normalized-loopback").toFile()
        val file = File(directory, "normalized.mp4")
        file.writeText("init")
        val server =
            VesperSourceNormalizerLoopbackServer(
                growingReadWaitMillis = 250,
                growingReadPollMillis = 10,
            )
        try {
            val handle =
                server.register(
                    VesperNormalizedResourceRegistration(
                        outputRoute = "fmp4LocalStream",
                        primaryResourcePath = file.absolutePath,
                        primaryContentType = "video/mp4",
                        sessionReadBufferBytes = 4096,
                    )
                )
            val writer =
                Thread {
                    Thread.sleep(50)
                    file.appendText("-fragment")
                }
            writer.start()
            val connection = URL(handle.playbackUri).openConnection() as HttpURLConnection

            assertEquals(200, connection.responseCode)
            assertEquals(null, connection.getHeaderField("Content-Length"))
            assertEquals("close", connection.getHeaderField("Connection"))
            assertEquals("init-fragment", connection.inputStream.bufferedReader().readText())
            writer.join(1_000)
        } finally {
            server.stop()
            directory.deleteRecursively()
        }
    }

    @Test
    fun loopbackServerExpiresTokens() {
        var now = 100L
        val directory = createTempDirectory(prefix = "vesper-normalized-loopback").toFile()
        val file = File(directory, "normalized.mp4")
        file.writeText("media")
        val server =
            VesperSourceNormalizerLoopbackServer(
                tokenTtlMillis = 10L,
                nowMillisProvider = { now },
            )
        try {
            val handle =
                server.register(
                    VesperNormalizedResourceRegistration(
                        outputRoute = "fmp4LocalStream",
                        primaryResourcePath = file.absolutePath,
                        primaryContentType = "video/mp4",
                        sessionReadBufferBytes = 4096,
                    )
                )
            assertNotNull(handle.token)
            now = 111L
            val connection = URL(handle.playbackUri).openConnection() as HttpURLConnection

            assertEquals(404, connection.responseCode)
        } finally {
            server.stop()
            directory.deleteRecursively()
        }
    }

    @Test
    fun loopbackServerServesHlsSegmentsFromSessionDirectory() {
        val directory = createTempDirectory(prefix = "vesper-normalized-loopback").toFile()
        val playlist = File(directory, "index.m3u8")
        val segment = File(directory, "segment_00001.m4s")
        playlist.writeText("#EXTM3U\n#EXTINF:3,\nsegment_00001.m4s\n")
        segment.writeText("segment")
        val server = VesperSourceNormalizerLoopbackServer()
        try {
            val handle =
                server.register(
                    VesperNormalizedResourceRegistration(
                        outputRoute = "hlsShortWindow",
                        primaryResourcePath = playlist.absolutePath,
                        primaryContentType = "application/vnd.apple.mpegurl",
                        sessionReadBufferBytes = 4096,
                    )
                )
            val segmentUrl = handle.playbackUri.replace("index.m3u8", "segment_00001.m4s")
            val connection = URL(segmentUrl).openConnection() as HttpURLConnection

            assertEquals(200, connection.responseCode)
            assertTrue(connection.contentType.startsWith("video/mp4"))
            assertEquals("segment", connection.inputStream.bufferedReader().readText())
        } finally {
            server.stop()
            directory.deleteRecursively()
        }
    }
}
