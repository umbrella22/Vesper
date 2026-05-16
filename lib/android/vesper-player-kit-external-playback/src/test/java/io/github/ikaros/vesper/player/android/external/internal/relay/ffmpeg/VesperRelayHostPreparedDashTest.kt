package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperRelayHostPreparedDashTest {
    @Test
    fun ffmpegInputStreamReadsIntoRequestedOffset() {
        val native = RecordingFfmpegNativeApi(byteArrayOf(1, 2, 3))
        val input = VesperRelayFfmpegInputStream(handle = 7L, native = native)
        val buffer = byteArrayOf(9, 9, 9, 9, 9)

        val read = input.read(buffer, 1, 3)

        assertEquals(3, read)
        assertEquals(listOf(ReadCall(handle = 7L, offset = 1, length = 3)), native.readCalls)
        assertEquals(listOf(9, 1, 2, 3, 9), buffer.map(Byte::toInt))
    }

    @Test
    fun ffmpegInputStreamHandlesZeroLengthClosedAndBoundsContract() {
        val native = RecordingFfmpegNativeApi(byteArrayOf(1))
        val input = VesperRelayFfmpegInputStream(handle = 7L, native = native)

        assertEquals(0, input.read(ByteArray(4), 2, 0))
        input.close()
        assertEquals(0, input.read(ByteArray(4), 2, 0))
        assertEquals(-1, input.read(ByteArray(4), 0, 1))

        assertThrowsIndexOutOfBounds { input.read(ByteArray(4), -1, 1) }
        assertThrowsIndexOutOfBounds { input.read(ByteArray(4), 0, -1) }
        assertThrowsIndexOutOfBounds { input.read(ByteArray(4), 3, 2) }
    }

    @Test
    fun plansStaticSegmentTemplateVideoAndAudioTracks() {
        val plan = planHostPreparedDash(
            manifestText = """
                <MPD type="static" mediaPresentationDuration="PT10S">
                  <BaseURL>https://cdn.example/root/</BaseURL>
                  <Period>
                    <BaseURL>period/</BaseURL>
                    <AdaptationSet mimeType="video/mp4">
                      <BaseURL>video/</BaseURL>
                      <Representation id="v1" codecs="avc1.640028">
                        <SegmentTemplate timescale="1" duration="4" startNumber="1"
                          initialization="init-${'$'}RepresentationID${'$'}.mp4"
                          media="chunk-${'$'}Number%05d${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                    <AdaptationSet mimeType="audio/mp4">
                      <Representation id="a1" codecs="mp4a.40.2">
                        <SegmentTemplate timescale="1" duration="4" startNumber="7"
                          initialization="audio-${'$'}RepresentationID${'$'}-init.mp4"
                          media="audio-${'$'}Number${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                  </Period>
                </MPD>
            """.trimIndent(),
            manifestUri = "https://example.com/video/manifest.mpd",
        )

        assertEquals(listOf("video", "audio"), plan.tracks.map { it.kind })
        val video = plan.tracks.first()
        assertEquals("video0", video.mediaId)
        assertEquals("https://cdn.example/root/period/video/init-v1.mp4", video.initializationUri)
        assertEquals(3, video.segments.size)
        assertEquals("https://cdn.example/root/period/video/chunk-00001.m4s", video.segments.first().uri)

        val audio = plan.tracks.last()
        assertEquals("audio-a1-init.mp4", audio.initializationUri?.substringAfterLast('/'))
        assertEquals("audio-7.m4s", audio.segments.first().uri.substringAfterLast('/'))
    }

    @Test
    fun plansFileDashWithRelativeSegmentsUnderManifestDirectory() {
        val root = Files.createTempDirectory("vesper-dash-plan").toFile()
        val manifest = File(root, "manifest.mpd")
        manifest.writeText("")

        val plan = planHostPreparedDash(
            manifestText = """
                <MPD type="static" mediaPresentationDuration="PT4S">
                  <Period>
                    <AdaptationSet mimeType="video/mp4">
                      <Representation id="v1" codecs="avc1.640028">
                        <SegmentTemplate timescale="1" duration="4" startNumber="1"
                          initialization="init.mp4"
                          media="segments/chunk-${'$'}Number${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                  </Period>
                </MPD>
            """.trimIndent(),
            manifestUri = manifest.toURI().toString(),
            sourceOrigin = VesperRelayDashSourceOrigin(
                kind = "file",
                manifestUri = manifest.toURI().toString(),
                rootUri = root.canonicalFile.toURI().toString(),
            ),
        )

        assertEquals(File(root, "init.mp4").toURI().toString(), plan.tracks.first().initializationUri)
        assertEquals(File(root, "segments/chunk-1.m4s").toURI().toString(), plan.tracks.first().segments.first().uri)
    }

    @Test
    fun rejectsFileDashReferencesOutsideManifestDirectory() {
        val root = Files.createTempDirectory("vesper-dash-plan").toFile()
        val manifest = File(root, "manifest.mpd")
        manifest.writeText("")

        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT4S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <SegmentTemplate timescale="1" duration="4" startNumber="1"
                              initialization="../init.mp4"
                              media="chunk-${'$'}Number${'$'}.m4s" />
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = manifest.toURI().toString(),
                sourceOrigin = VesperRelayDashSourceOrigin(
                    kind = "file",
                    manifestUri = manifest.toURI().toString(),
                    rootUri = root.canonicalFile.toURI().toString(),
                ),
            )
        }

        assertEquals("unsupported_mixed_dash_origin", error.diagnostic.code)
    }

    @Test
    fun plansContentDashWithRelativeSegmentsUnderProviderRoot() {
        val plan = planHostPreparedDash(
            manifestText = """
                <MPD type="static" mediaPresentationDuration="PT4S">
                  <Period>
                    <AdaptationSet mimeType="video/mp4">
                      <Representation id="v1" codecs="avc1.640028">
                        <SegmentTemplate timescale="1" duration="4" startNumber="1"
                          initialization="init.mp4"
                          media="segments/chunk-${'$'}Number${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                  </Period>
                </MPD>
            """.trimIndent(),
            manifestUri = "content://media/video/demo/manifest.mpd",
            sourceOrigin = VesperRelayDashSourceOrigin(
                kind = "content",
                manifestUri = "content://media/video/demo/manifest.mpd",
                rootUri = "content://media/video/demo",
            ),
        )

        assertEquals("content://media/video/demo/init.mp4", plan.tracks.first().initializationUri)
        assertEquals("content://media/video/demo/segments/chunk-1.m4s", plan.tracks.first().segments.first().uri)
    }

    @Test
    fun rejectsRemoteReferenceFromContentDash() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT4S">
                      <BaseURL>https://cdn.example/video/</BaseURL>
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <SegmentTemplate timescale="1" duration="4" startNumber="1"
                              initialization="init.mp4"
                              media="chunk-${'$'}Number${'$'}.m4s" />
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "content://media/video/demo/manifest.mpd",
                sourceOrigin = VesperRelayDashSourceOrigin(
                    kind = "content",
                    manifestUri = "content://media/video/demo/manifest.mpd",
                    rootUri = "content://media/video/demo",
                ),
            )
        }

        assertEquals("unsupported_mixed_dash_origin", error.diagnostic.code)
    }

    @Test
    fun rejectsDynamicDash() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """<MPD type="dynamic" mediaPresentationDuration="PT10S" />""",
                manifestUri = "https://example.com/live.mpd",
            )
        }

        assertEquals("unsupported_dynamic_dash", error.diagnostic.code)
    }

    @Test
    fun rejectsEncryptedDash() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT10S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" />
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://example.com/encrypted.mpd",
            )
        }

        assertEquals("unsupported_encrypted_dash", error.diagnostic.code)
    }

    @Test
    fun rejectsSegmentTimeline() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT10S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1">
                            <SegmentTemplate media="v-${'$'}Time${'$'}.m4s" initialization="init.mp4">
                              <SegmentTimeline><S t="0" d="4" /></SegmentTimeline>
                            </SegmentTemplate>
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://example.com/timeline.mpd",
            )
        }

        assertEquals("unsupported_dash_layout", error.diagnostic.code)
        assertTrue(error.diagnostic.message.contains("SegmentTimeline"))
    }

    @Test
    fun rejectsMissingFiniteDuration() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1">
                            <SegmentTemplate timescale="1" duration="4"
                              initialization="init.mp4" media="v-${'$'}Number${'$'}.m4s" />
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://example.com/no-duration.mpd",
            )
        }

        assertEquals("unsupported_dash_layout", error.diagnostic.code)
        assertTrue(error.diagnostic.message.contains("finite"))
    }

    private fun unsupported(block: () -> Unit): VesperRelayHostInputException =
        try {
            block()
            throw AssertionError("Expected host input exception")
        } catch (error: VesperRelayHostInputException) {
            error
    }
}

private data class ReadCall(
    val handle: Long,
    val offset: Int,
    val length: Int,
)

private class RecordingFfmpegNativeApi(
    private val payload: ByteArray,
) : VesperRelayFfmpegNativeApi {
    val readCalls = mutableListOf<ReadCall>()
    var closedHandle: Long? = null

    override fun read(handle: Long, buffer: ByteArray, offset: Int, length: Int): Int {
        readCalls += ReadCall(handle, offset, length)
        val count = minOf(length, payload.size)
        payload.copyInto(buffer, destinationOffset = offset, endIndex = count)
        return count
    }

    override fun close(handle: Long) {
        closedHandle = handle
    }
}

private fun assertThrowsIndexOutOfBounds(block: () -> Unit) {
    try {
        block()
        fail("expected IndexOutOfBoundsException")
    } catch (_: IndexOutOfBoundsException) {
    }
}
