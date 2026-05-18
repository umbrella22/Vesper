package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDashRemoteMediaPolicy
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.file.Files
import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean
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
    fun plansSegmentTemplateWithRepresentationBaseUrlInheritance() {
        val plan = planHostPreparedDash(
            manifestText = """
                <MPD type="static" mediaPresentationDuration="PT4S">
                  <BaseURL>https://cdn.example/root/</BaseURL>
                  <Period>
                    <BaseURL>period/</BaseURL>
                    <AdaptationSet mimeType="video/mp4">
                      <BaseURL>adaptation/</BaseURL>
                      <Representation id="v1" codecs="avc1.640028">
                        <BaseURL>representation/</BaseURL>
                        <SegmentTemplate timescale="1" duration="4" startNumber="3"
                          initialization="init-${'$'}RepresentationID${'$'}.mp4"
                          media="chunk-${'$'}Number%02d${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                  </Period>
                </MPD>
            """.trimIndent(),
            manifestUri = "https://example.com/video/manifest.mpd",
        )

        val track = plan.tracks.single()
        assertEquals("https://cdn.example/root/period/adaptation/representation/init-v1.mp4", track.initializationUri)
        assertEquals("https://cdn.example/root/period/adaptation/representation/chunk-03.m4s", track.segments.single().uri)
    }

    @Test
    fun plansStaticSegmentBaseVideoTrackWithSidxRanges() {
        withBridgeApi(FakeSegmentBaseBridgeApi) {
            val resolver = ByteArrayRangeResolver(
                uri = "https://cdn.example/video/main.mp4",
                payload = byteArrayOf(9, 8, 7),
            )

            val plan = planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT10S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <BaseURL>main.mp4</BaseURL>
                            <SegmentBase indexRange="100-199">
                              <Initialization range="0-99" />
                            </SegmentBase>
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://cdn.example/video/manifest.mpd",
                resolver = resolver,
            )

            assertEquals(listOf("https://cdn.example/video/main.mp4" to VesperRelayDashByteRange(100, 199)), resolver.reads)
            val track = plan.tracks.single()
            assertEquals("video", track.kind)
            assertEquals("https://cdn.example/video/main.mp4", track.initializationUri)
            assertEquals(VesperRelayDashByteRange(0, 99), track.initializationRange)
            assertEquals(2, track.segments.size)
            assertEquals(VesperRelayDashByteRange(200, 299), track.segments[0].byteRange)
            assertEquals(VesperRelayDashByteRange(300, 449), track.segments[1].byteRange)
        }
    }

    @Test
    fun plansSegmentBaseWithRepresentationBaseUrlInheritance() {
        withBridgeApi(FakeSegmentBaseBridgeApi) {
            val mediaUri = "https://cdn.example/root/period/adaptation/representation/main.mp4"
            val resolver = ByteArrayRangeResolver(
                uri = mediaUri,
                payload = byteArrayOf(9, 8, 7),
            )

            val plan = planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT10S">
                      <BaseURL>https://cdn.example/root/</BaseURL>
                      <Period>
                        <BaseURL>period/</BaseURL>
                        <AdaptationSet mimeType="video/mp4">
                          <BaseURL>adaptation/</BaseURL>
                          <Representation id="v1" codecs="avc1.640028">
                            <BaseURL>representation/main.mp4</BaseURL>
                            <SegmentBase indexRange="100-199">
                              <Initialization range="0-99" />
                            </SegmentBase>
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://example.com/video/manifest.mpd",
                resolver = resolver,
            )

            assertEquals(listOf(mediaUri to VesperRelayDashByteRange(100, 199)), resolver.reads)
            val track = plan.tracks.single()
            assertEquals(mediaUri, track.initializationUri)
            assertEquals(mediaUri, track.segments.first().uri)
            assertEquals(VesperRelayDashByteRange(200, 299), track.segments.first().byteRange)
        }
    }

    @Test
    fun rejectsSegmentBaseWithoutSidxRange() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT10S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <BaseURL>main.mp4</BaseURL>
                            <SegmentBase>
                              <Initialization range="0-99" />
                            </SegmentBase>
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://cdn.example/video/manifest.mpd",
            )
        }

        assertEquals("unsupported_dash_layout", error.diagnostic.code)
        assertTrue(error.diagnostic.message.contains("indexRange"))
    }

    @Test
    fun rejectsInvalidSegmentBaseRange() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="static" mediaPresentationDuration="PT10S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <BaseURL>main.mp4</BaseURL>
                            <SegmentBase indexRange="200-100">
                              <Initialization range="0-99" />
                            </SegmentBase>
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://cdn.example/video/manifest.mpd",
            )
        }

        assertEquals("unsupported_dash_layout", error.diagnostic.code)
        assertTrue(error.diagnostic.message.contains("indexRange"))
    }

    @Test
    fun fallsBackToSegmentTemplateForDynamicDashWhenAvailable() {
        val plan = planHostPreparedDash(
            manifestText = """
                <MPD type="dynamic" mediaPresentationDuration="PT8S">
                  <Period>
                    <AdaptationSet mimeType="video/mp4">
                      <Representation id="sb" codecs="avc1.640028">
                        <BaseURL>main.mp4</BaseURL>
                        <SegmentBase indexRange="100-199">
                          <Initialization range="0-99" />
                        </SegmentBase>
                      </Representation>
                      <Representation id="tmpl" codecs="avc1.640028">
                        <SegmentTemplate timescale="1" duration="4" startNumber="1"
                          initialization="init-${'$'}RepresentationID${'$'}.mp4"
                          media="chunk-${'$'}Number${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                  </Period>
                </MPD>
            """.trimIndent(),
            manifestUri = "https://example.com/video/manifest.mpd",
        )

        val track = plan.tracks.single()
        assertEquals("https://example.com/video/init-tmpl.mp4", track.initializationUri)
        assertEquals("https://example.com/video/chunk-1.m4s", track.segments.first().uri)
    }

    @Test
    fun dynamicDashSkipsSegmentBaseAudioWhenTemplateVideoIsAvailable() {
        val plan = planHostPreparedDash(
            manifestText = """
                <MPD type="dynamic" mediaPresentationDuration="PT8S">
                  <Period>
                    <AdaptationSet mimeType="video/mp4">
                      <Representation id="tmpl" codecs="avc1.640028">
                        <SegmentTemplate timescale="1" duration="4" startNumber="1"
                          initialization="init-${'$'}RepresentationID${'$'}.mp4"
                          media="chunk-${'$'}Number${'$'}.m4s" />
                      </Representation>
                    </AdaptationSet>
                    <AdaptationSet mimeType="audio/mp4">
                      <Representation id="a1" codecs="mp4a.40.2">
                        <BaseURL>audio.mp4</BaseURL>
                        <SegmentBase indexRange="100-199">
                          <Initialization range="0-99" />
                        </SegmentBase>
                      </Representation>
                    </AdaptationSet>
                  </Period>
                </MPD>
            """.trimIndent(),
            manifestUri = "https://example.com/video/manifest.mpd",
        )

        assertEquals(listOf("video"), plan.tracks.map { it.kind })
        assertEquals("https://example.com/video/chunk-1.m4s", plan.tracks.single().segments.first().uri)
    }

    @Test
    fun rejectsDynamicSegmentBaseWithoutTemplateFallback() {
        val error = unsupported {
            planHostPreparedDash(
                manifestText = """
                    <MPD type="dynamic" mediaPresentationDuration="PT8S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <BaseURL>main.mp4</BaseURL>
                            <SegmentBase indexRange="100-199">
                              <Initialization range="0-99" />
                            </SegmentBase>
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                """.trimIndent(),
                manifestUri = "https://example.com/live.mpd",
            )
        }

        assertEquals("unsupported_dash_layout", error.diagnostic.code)
        assertTrue(error.diagnostic.message.contains("Dynamic DASH SegmentBase"))
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
    fun rejectsHybridFileDashWithRemoteBaseUrlByDefault() {
        val root = Files.createTempDirectory("vesper-dash-hybrid-plan").toFile()
        val manifest = File(root, "manifest.mpd")
        manifest.writeText("")

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
    fun plansHybridFileDashWithRemoteBaseUrl() {
        val root = Files.createTempDirectory("vesper-dash-hybrid-plan").toFile()
        val manifest = File(root, "manifest.mpd")
        manifest.writeText("")

        val plan = planHostPreparedDash(
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
            manifestUri = manifest.toURI().toString(),
            sourceOrigin = VesperRelayDashSourceOrigin(
                kind = "file",
                manifestUri = manifest.toURI().toString(),
                rootUri = root.canonicalFile.toURI().toString(),
                allowRemoteMediaReferences = true,
            ),
        )

        assertEquals("https://cdn.example/video/init.mp4", plan.tracks.first().initializationUri)
        assertEquals("https://cdn.example/video/chunk-1.m4s", plan.tracks.first().segments.first().uri)
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
                    allowRemoteMediaReferences = true,
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
    fun plansHybridContentDashWithRemoteBaseUrl() {
        val plan = planHostPreparedDash(
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
                allowRemoteMediaReferences = true,
            ),
        )

        assertEquals("https://cdn.example/video/init.mp4", plan.tracks.first().initializationUri)
        assertEquals("https://cdn.example/video/chunk-1.m4s", plan.tracks.first().segments.first().uri)
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

    @Test
    fun hybridFileResolverFetchesRemoteMediaWithHeaders() {
        val root = Files.createTempDirectory("vesper-dash-hybrid-range").toFile()
        val manifest = File(root, "manifest.mpd").apply { writeText("<MPD />") }
        val requests = Collections.synchronizedList(mutableListOf<RecordedRequest>())
        val server = RangeHttpServer(
            body = "abcdefghij".toByteArray(),
            requests = requests,
        )
        server.start()
        try {
            val resolver = VesperRelayFileDashResourceResolver(
                origin = VesperRelayDashSourceOrigin(
                    kind = "file",
                    manifestUri = manifest.toURI().toString(),
                    rootUri = root.canonicalFile.toURI().toString(),
                    allowRemoteMediaReferences = true,
                ),
                remoteHeaders = mapOf(
                    "Cookie" to "source-cookie",
                    "Authorization" to "Bearer secret",
                    "Range" to "bytes=0-1",
                    "Referer" to "https://app.example/player",
                    "User-Agent" to "VesperRelayTest",
                ),
                remoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(
                    allowRemoteReferences = true,
                    allowPrivateAddresses = true,
                    allowedRequestHeaders = setOf("User-Agent", "Referer"),
                ),
            )
            val output = ByteArrayOutputStream()

            resolver.copyRangeTo(
                uri = "http://${server.address.hostAddress}:${server.port}/media.mp4",
                range = VesperRelayDashByteRange(2, 5),
                output = output,
                cancellation = AtomicBoolean(false),
            )

            val headers = requests.last().headers
            assertEquals("cdef", output.toByteArray().toString(Charsets.UTF_8))
            assertEquals(null, headers.valueFor("Cookie"))
            assertEquals(null, headers.valueFor("Authorization"))
            assertEquals("VesperRelayTest", headers.valueFor("User-Agent"))
            assertEquals("https://app.example/player", headers.valueFor("Referer"))
            assertEquals("bytes=2-5", headers.valueFor("Range"))
        } finally {
            server.stop()
        }
    }

    @Test
    fun hybridFileResolverRejectsPrivateRemoteMediaAddressesByDefault() {
        val root = Files.createTempDirectory("vesper-dash-hybrid-range").toFile()
        val manifest = File(root, "manifest.mpd").apply { writeText("<MPD />") }
        val resolver = VesperRelayFileDashResourceResolver(
            origin = VesperRelayDashSourceOrigin(
                kind = "file",
                manifestUri = manifest.toURI().toString(),
                rootUri = root.canonicalFile.toURI().toString(),
                allowRemoteMediaReferences = true,
            ),
            remoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(
                allowRemoteReferences = true,
            ),
        )

        val error = try {
            resolver.copyRangeTo(
                uri = "http://127.0.0.1:9/media.mp4",
                range = VesperRelayDashByteRange(2, 5),
                output = ByteArrayOutputStream(),
                cancellation = AtomicBoolean(false),
            )
            fail("expected private address rejection")
            return
        } catch (error: IOException) {
            error
        }

        assertTrue(error.message.orEmpty().contains("private or local address"))
    }

    @Test
    fun hybridFileResolverValidatesRedirectTargets() {
        val root = Files.createTempDirectory("vesper-dash-hybrid-redirect").toFile()
        val manifest = File(root, "manifest.mpd").apply { writeText("<MPD />") }
        val requests = Collections.synchronizedList(mutableListOf<RecordedRequest>())
        val server = RangeHttpServer(
            body = "abcdefghij".toByteArray(),
            requests = requests,
            redirectLocation = "file:///private.mp4",
        )
        server.start()
        try {
            val resolver = VesperRelayFileDashResourceResolver(
                origin = VesperRelayDashSourceOrigin(
                    kind = "file",
                    manifestUri = manifest.toURI().toString(),
                    rootUri = root.canonicalFile.toURI().toString(),
                    allowRemoteMediaReferences = true,
                ),
                remoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(
                    allowRemoteReferences = true,
                    allowPrivateAddresses = true,
                ),
            )

            val error = try {
                resolver.copyRangeTo(
                    uri = "http://${server.address.hostAddress}:${server.port}/redirect.mp4",
                    range = VesperRelayDashByteRange(2, 5),
                    output = ByteArrayOutputStream(),
                    cancellation = AtomicBoolean(false),
                )
                fail("expected redirect target rejection")
                return
            } catch (error: IOException) {
                error
            }

            assertTrue(error.message.orEmpty().contains("http or https"))
        } finally {
            server.stop()
        }
    }

    @Test
    fun httpRangeResolverSendsRangeHeaderAndCopiesPartialBody() {
        val requests = Collections.synchronizedList(mutableListOf<RecordedRequest>())
        val server = RangeHttpServer(
            body = "abcdefghij".toByteArray(),
            requests = requests,
        )
        server.start()
        try {
            val resolver = VesperRelayRemoteDashResourceClient(
                headers = emptyMap(),
                allowPrivateAddresses = true,
            )
            val output = ByteArrayOutputStream()

            resolver.copyRangeTo(
                uri = "http://${server.address.hostAddress}:${server.port}/media.mp4",
                range = VesperRelayDashByteRange(2, 5),
                output = output,
                cancellation = AtomicBoolean(false),
            )

            assertEquals("cdef", output.toByteArray().toString(Charsets.UTF_8))
            assertEquals("bytes=2-5", requests.last().headers.valueFor("Range"))
        } finally {
            server.stop()
        }
    }

    @Test
    fun httpRangeResolverRejectsMismatchedContentRange() {
        val requests = Collections.synchronizedList(mutableListOf<RecordedRequest>())
        val server = RangeHttpServer(
            body = "abcdefghij".toByteArray(),
            requests = requests,
            contentRange = "bytes 0-3/10",
        )
        server.start()
        try {
            val resolver = VesperRelayRemoteDashResourceClient(
                headers = emptyMap(),
                allowPrivateAddresses = true,
            )

            val error = try {
                resolver.copyRangeTo(
                    uri = "http://${server.address.hostAddress}:${server.port}/media.mp4",
                    range = VesperRelayDashByteRange(2, 5),
                    output = ByteArrayOutputStream(),
                    cancellation = AtomicBoolean(false),
                )
                fail("expected range copy failure")
                return
            } catch (error: IOException) {
                error
            }

            assertTrue(error.message.orEmpty().contains("invalid Content-Range"))
            assertEquals("bytes=2-5", requests.last().headers.valueFor("Range"))
        } finally {
            server.stop()
        }
    }

    @Test
    fun fileRangeResolverCopiesOnlyRequestedBytes() {
        val root = Files.createTempDirectory("vesper-dash-range").toFile()
        val manifest = File(root, "manifest.mpd").apply { writeText("<MPD />") }
        val media = File(root, "media.mp4").apply { writeText("abcdefghij") }
        val resolver = VesperRelayFileDashResourceResolver(
            origin = VesperRelayDashSourceOrigin(
                kind = "file",
                manifestUri = manifest.toURI().toString(),
                rootUri = root.canonicalFile.toURI().toString(),
            ),
        )
        val output = ByteArrayOutputStream()

        resolver.copyRangeTo(
            uri = media.toURI().toString(),
            range = VesperRelayDashByteRange(3, 6),
            output = output,
            cancellation = AtomicBoolean(false),
        )

        assertEquals("defg", output.toByteArray().toString(Charsets.UTF_8))
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

private class ByteArrayRangeResolver(
    private val uri: String,
    private val payload: ByteArray,
) : VesperRelayDashResourceResolver(
    origin = VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = uri,
        rootUri = uri,
    ),
    manifestLogicalUri = uri,
) {
    val reads = mutableListOf<Pair<String, VesperRelayDashByteRange>>()

    override fun readRange(
        uri: String,
        range: VesperRelayDashByteRange,
    ): ByteArray {
        reads += uri to range
        return payload
    }
}

private fun withBridgeApi(
    api: VesperRelayDashBridgeApi,
    block: () -> Unit,
) {
    val previous = VesperRelayDashBridgeApiProvider
    VesperRelayDashBridgeApiProvider = api
    try {
        block()
    } finally {
        VesperRelayDashBridgeApiProvider = previous
    }
}

private object FakeSegmentBaseBridgeApi : VesperRelayDashBridgeApi {
    override fun parseSidx(data: ByteArray): VesperRelayDashSidxBox =
        VesperRelayDashSidxBox(
            timescale = 1000,
            earliestPresentationTime = 0,
            firstOffset = 0,
            references = listOf(
                VesperRelayDashSidxReference(
                    referenceType = 0,
                    referencedSize = 100,
                    subsegmentDuration = 4000,
                    startsWithSap = true,
                    sapType = 1,
                    sapDeltaTime = 0,
                ),
                VesperRelayDashSidxReference(
                    referenceType = 0,
                    referencedSize = 150,
                    subsegmentDuration = 6000,
                    startsWithSap = true,
                    sapType = 1,
                    sapDeltaTime = 0,
                ),
            ),
        )

    override fun mediaSegments(
        segmentBase: VesperRelayDashByteRangeSegmentBase,
        sidx: VesperRelayDashSidxBox,
    ): List<VesperRelayDashMediaSegment> =
        listOf(
            VesperRelayDashMediaSegment(
                duration = 4.0,
                range = VesperRelayDashByteRange(200, 299),
            ),
            VesperRelayDashMediaSegment(
                duration = 6.0,
                range = VesperRelayDashByteRange(300, 449),
            ),
        )
}

private data class RecordedRequest(
    val method: String,
    val headers: Map<String, String>,
)

private class RangeHttpServer(
    private val body: ByteArray,
    private val requests: MutableList<RecordedRequest>,
    private val contentRange: String = "bytes 2-5/10",
    private val redirectLocation: String? = null,
) {
    private val loopback = InetAddress.getByName("127.0.0.1")
    private val running = AtomicBoolean(false)
    private val serverSocket = ServerSocket(0, 50, loopback)
    private val thread = Thread(::run, "vesper-dash-range-test-upstream").apply { isDaemon = true }

    val address: InetAddress
        get() = serverSocket.inetAddress

    val port: Int
        get() = serverSocket.localPort

    fun start() {
        if (running.compareAndSet(false, true)) {
            thread.start()
        }
    }

    fun stop() {
        running.set(false)
        serverSocket.close()
        thread.join(1_000)
    }

    private fun run() {
        while (running.get()) {
            val socket = try {
                serverSocket.accept()
            } catch (_: Exception) {
                break
            }
            Thread({ handle(socket) }, "vesper-dash-range-test-request").apply {
                isDaemon = true
                start()
            }
        }
    }

    private fun handle(socket: Socket) {
        socket.use { client ->
            val input = client.getInputStream().bufferedReader(Charsets.ISO_8859_1)
            val requestLine = input.readLine() ?: return
            val method = requestLine.substringBefore(' ')
            val headers = linkedMapOf<String, String>()
            while (true) {
                val line = input.readLine() ?: break
                if (line.isEmpty()) {
                    break
                }
                val separator = line.indexOf(':')
                if (separator > 0) {
                    headers[line.substring(0, separator)] = line.substring(separator + 1).trim()
                }
            }
            requests += RecordedRequest(method = method, headers = headers)

            if (redirectLocation != null) {
                val response = buildString {
                    append("HTTP/1.1 302 Found\r\n")
                    append("Location: ").append(redirectLocation).append("\r\n")
                    append("Content-Length: 0\r\n")
                    append("Connection: close\r\n")
                    append("\r\n")
                }.toByteArray(Charsets.ISO_8859_1)
                client.getOutputStream().use { output ->
                    output.write(response)
                }
                return
            }

            val range = headers.valueFor("Range")
            val responseBody =
                if (range == "bytes=2-5") {
                    body.copyOfRange(2, 6)
                } else {
                    body
                }
            val status = if (range == null) 200 else 206
            val response = buildString {
                append("HTTP/1.1 ").append(status)
                    .append(if (status == 206) " Partial Content" else " OK")
                    .append("\r\n")
                append("Content-Type: video/mp4\r\n")
                append("Accept-Ranges: bytes\r\n")
                if (status == 206) {
                    append("Content-Range: ").append(contentRange).append("\r\n")
                }
                append("Content-Length: ").append(responseBody.size).append("\r\n")
                append("Connection: close\r\n")
                append("\r\n")
            }.toByteArray(Charsets.ISO_8859_1)
            client.getOutputStream().use { output ->
                output.write(response)
                if (method != "HEAD") {
                    output.write(responseBody)
                }
            }
        }
    }
}

private fun Map<String, String>.valueFor(name: String): String? =
    entries.firstOrNull { (key, _) -> key.equals(name, ignoreCase = true) }?.value

private fun assertThrowsIndexOutOfBounds(block: () -> Unit) {
    try {
        block()
        fail("expected IndexOutOfBoundsException")
    } catch (_: IndexOutOfBoundsException) {
    }
}
