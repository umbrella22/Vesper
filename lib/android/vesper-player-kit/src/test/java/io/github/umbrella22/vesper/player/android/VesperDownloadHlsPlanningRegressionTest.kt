package io.github.umbrella22.vesper.player.android

import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.file.Files
import java.util.Collections
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.*
import org.junit.Test

class VesperDownloadHlsPlanningRegressionTest {
    @Test
    fun mediaRewritePreservesMapSwitchAndDiscontinuity() {
        val playlist = parseHlsMediaPlaylist("https://fixture.test/media.m3u8", """
            #EXTM3U
            #EXT-X-MAP:URI="init-a.mp4"
            #EXTINF:2,
            one.m4s
            #EXT-X-DISCONTINUITY
            #EXT-X-MAP:URI="init-b.mp4"
            #EXTINF:2,
            two.m4s
            #EXT-X-ENDLIST
        """.trimIndent())
        val mappings = playlist.maps.associate { "${it.uri}:${it.byteRange}" to it.uri.substringAfterLast('/') }
        val rewritten = rewriteHlsMedia("video", playlist, mappings)
        val expected = listOf("#EXT-X-MAP:URI=\"init-a.mp4\"", "video-00001.m4s", "#EXT-X-DISCONTINUITY", "#EXT-X-MAP:URI=\"init-b.mp4\"", "video-00002.m4s")
        var previous = -1
        expected.forEach {
            val position = rewritten.indexOf(it)
            assertTrue("Missing or out-of-order $it in $rewritten", position > previous)
            previous = position
        }
    }

    @Test
    fun sharedInitIsReferencedByEveryMediaPlaylist() = withFixture { executor, base, _ ->
        val playlist = parseHlsMediaPlaylist("$base/media.m3u8", media("seg.m4s", "#EXT-X-MAP:URI=\"init.mp4\"\n"))
        val index = executor.buildHlsMediaAssetIndex("index.m3u8", listOf("video" to playlist, "audio" to playlist), emptyMap())
        val init = index.resources.single { it.uri == "$base/init.mp4" }
        for (id in listOf("video", "audio")) {
            val generated = index.resources.single { it.resourceId == "hls-$id-playlist" }.generatedText!!
            assertTrue(generated.contains("#EXT-X-MAP:URI=\"${init.relativePath}\""))
        }
        assertEquals(1, index.resources.count { it.uri == "$base/init.mp4" })
    }

    @Test
    fun masterWithOneChildKeepsAnActualOfflineChild() = withFixture { executor, base, _ ->
        val index = executor.planHlsMasterAssetIndex("$base/master.m3u8", master(), VesperDownloadProfile(), emptyMap())
        val root = index.resources.single { it.relativePath == "index.m3u8" }.generatedText!!
        val child = root.lineSequence().filter { it.isNotBlank() && !it.startsWith('#') }.single()
        assertEquals("video.m3u8", child)
        assertTrue(index.resources.any { it.relativePath == child && it.generatedText?.contains("#EXTINF:") == true })
    }

    @Test
    fun directMediaRootCanStillBeFlattened() = withFixture { executor, base, _ ->
        val playlist = parseHlsMediaPlaylist("$base/media.m3u8", media("seg.m4s"))
        val index = executor.buildHlsMediaAssetIndex("index.m3u8", listOf("media" to playlist), emptyMap())
        assertEquals(1, index.resources.count { it.relativePath?.endsWith(".m3u8") == true })
        assertTrue(index.resources.single().generatedText!!.contains("#EXTINF:"))
    }

    @Test
    fun audioLanguagePreferenceCannotCrossReferencedGroup() = withFixture { executor, base, requests ->
        val declarations = """
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="wrong",LANGUAGE="zh",DEFAULT=YES,URI="other.m3u8"
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="selected",LANGUAGE="en",DEFAULT=NO,URI="audio.m3u8"
        """.trimIndent()
        val index = executor.planHlsMasterAssetIndex("$base/master.m3u8", master(declarations, "selected"), VesperDownloadProfile(preferredAudioLanguage = "zh"), emptyMap())
        assertTrue(index.segments.any { it.uri == "$base/audio.m4s" })
        assertFalse(index.segments.any { it.uri == "$base/other.m4s" })
        assertFalse(requests.contains("/other.m3u8"))
    }

    @Test
    fun unreferencedAudioGroupIsNotDownloaded() = withFixture { executor, base, requests ->
        val declaration = "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"other\",DEFAULT=YES,URI=\"other.m3u8\""
        val index = executor.planHlsMasterAssetIndex("$base/master.m3u8", master(declaration), VesperDownloadProfile(), emptyMap())
        assertFalse(index.resources.any { it.relativePath == "audio.m3u8" })
        assertFalse(requests.contains("/other.m3u8"))
    }

    @Test
    fun preferredInbandRenditionDoesNotFetchAnExternalFallback() = withFixture { executor, base, requests ->
        val declarations = """
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="selected",LANGUAGE="zh",DEFAULT=YES
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="selected",LANGUAGE="en",DEFAULT=NO,URI="other.m3u8"
        """.trimIndent()
        val index = executor.planHlsMasterAssetIndex("$base/master.m3u8", master(declarations, "selected"), VesperDownloadProfile(preferredAudioLanguage = "zh"), emptyMap())
        assertFalse(index.resources.any { it.relativePath == "audio.m3u8" })
        assertFalse(requests.contains("/other.m3u8"))
    }

    @Test
    fun encryptedMediaIsRejectedEvenWhenLaterKeyResetsToNone() {
        val failure = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            parseHlsMediaPlaylist("https://fixture.test/media.m3u8", media("encrypted.ts", "#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n") + "#EXT-X-KEY:METHOD=NONE\n")
        }
        assertEquals("hlsEncryptionUnsupported", failure.details["reason"])
    }

    @Test
    fun encryptedSessionKeyIsRejectedBeforeFetchingChildren() = withFixture { executor, base, requests ->
        var failure: VesperPlayerUnsupportedOperation? = null
        try {
            executor.planHlsMasterAssetIndex("$base/master.m3u8", master("#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"key\""), VesperDownloadProfile(), emptyMap())
        } catch (error: VesperPlayerUnsupportedOperation) {
            failure = error
        }
        assertNotNull(failure)
        assertEquals("hlsEncryptionUnsupported", failure!!.details["reason"])
        assertFalse(requests.contains("/video.m3u8"))
    }

    @Test
    fun methodNoneRetainsClearPlaybackPlanning() {
        assertEquals(1, parseHlsMediaPlaylist("https://fixture.test/media.m3u8", media("clear.ts", "#EXT-X-KEY:METHOD=NONE\n")).segments.size)
        assertEquals(1, parseHlsMasterPlaylist("https://fixture.test/master.m3u8", master("#EXT-X-SESSION-KEY:METHOD=NONE")).variants.size)
    }

    @Test
    fun publicPreparationReportsTypedUnsupportedWithoutPublishingFiles() = withFixture { executor, base, _ ->
        val completed = CompletableDeferred<VesperDownloadError?>()
        val task = VesperDownloadTaskSnapshot(taskId = 1, assetId = "fixture", source = VesperDownloadSource(
            VesperPlayerSource(uri = "$base/encrypted.m3u8", label = "Fixture", kind = VesperPlayerSourceKind.Remote, protocol = VesperPlayerSourceProtocol.Hls)),
            profile = VesperDownloadProfile(), state = VesperDownloadState.Preparing,
            progress = VesperDownloadProgressSnapshot(), assetIndex = VesperDownloadAssetIndex())
        executor.prepare(task, object : VesperDownloadExecutionReporter {
            override fun completePreparation(taskId: VesperDownloadTaskId, assetIndex: VesperDownloadAssetIndex) { completed.complete(null) }
            override fun updateProgress(taskId: VesperDownloadTaskId, receivedBytes: Long, receivedSegments: Int) = Unit
            override fun complete(taskId: VesperDownloadTaskId, completedPath: String?) = Unit
            override fun fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) { completed.complete(error) }
        })
        val failure = withTimeout(5_000) { completed.await() }
        assertNotNull(failure)
        assertEquals(VesperPlayerErrorCode.Unsupported, failure!!.code)
        assertEquals(VesperPlayerErrorCategory.Capability, failure.category)
        assertFalse(executor.baseDirectory!!.walkTopDown().any { it.isFile })
    }

    private fun master(declarations: String = "", audioGroup: String? = null): String =
        "#EXTM3U\n$declarations\n#EXT-X-STREAM-INF:BANDWIDTH=1000${audioGroup?.let { ",AUDIO=\"$it\"" } ?: ""}\nvideo.m3u8\n"

    private fun media(segment: String, declarations: String = ""): String =
        "#EXTM3U\n#EXT-X-TARGETDURATION:2\n$declarations#EXTINF:2,\n$segment\n#EXT-X-ENDLIST\n"

    private fun withFixture(block: suspend (VesperForegroundDownloadExecutor, String, MutableList<String>) -> Unit) = runBlocking {
        val directory = Files.createTempDirectory("vesper-hls-planning").toFile()
        val executor = VesperForegroundDownloadExecutor(null, directory)
        val requests = Collections.synchronizedList(mutableListOf<String>())
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/") { exchange ->
            try {
                val path = exchange.requestURI.path
                requests += path
                val body = when (path) {
                    "/video.m3u8" -> media("video.m4s")
                    "/audio.m3u8" -> media("audio.m4s")
                    "/other.m3u8" -> media("other.m4s")
                    "/encrypted.m3u8" -> media("encrypted.ts", "#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n")
                    else -> "fixture-bytes"
                }.toByteArray()
                exchange.responseHeaders.set("Content-Length", body.size.toString())
                if (exchange.requestMethod == "HEAD") exchange.sendResponseHeaders(200, -1)
                else { exchange.sendResponseHeaders(200, body.size.toLong()); exchange.responseBody.write(body) }
            } finally { exchange.close() }
        }
        server.start()
        try { withTimeout(10_000) { block(executor, "http://127.0.0.1:${server.address.port}", requests) } }
        finally { executor.dispose(); server.stop(0); directory.deleteRecursively() }
    }
}
