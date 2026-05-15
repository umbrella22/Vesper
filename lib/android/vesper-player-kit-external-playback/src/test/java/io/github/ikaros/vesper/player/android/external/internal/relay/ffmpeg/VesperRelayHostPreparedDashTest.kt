package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperRelayHostPreparedDashTest {
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
