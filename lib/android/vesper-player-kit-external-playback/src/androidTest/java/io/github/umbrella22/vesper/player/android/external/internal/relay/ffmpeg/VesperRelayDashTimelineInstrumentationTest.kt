package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperRelayDashTimelineInstrumentationTest {
    @Test
    fun nativeBridgeExpandsRawTimeAndNumberWithOffset() {
        val track = plan("""<S t="5000" d="2000" r="2" /><S d="1000" />""", "PT7S").tracks.single()
        assertEquals(listOf(7L, 8L, 9L, 10L), track.segments.map { it.index })
        assertEquals(
            listOf("chunk-05000-7.m4s", "chunk-07000-8.m4s", "chunk-09000-9.m4s", "chunk-11000-10.m4s"),
            track.segments.map { it.uri.substringAfterLast('/') },
        )
    }

    @Test
    fun nativeBridgeBoundsNegativeRepeatByThePresentationEnd() {
        val track = plan("""<S t="5000" d="2000" r="-1" />""", "PT5S").tracks.single()
        assertEquals(listOf("chunk-05000-7.m4s", "chunk-07000-8.m4s", "chunk-09000-9.m4s"),
            track.segments.map { it.uri.substringAfterLast('/') })
    }

    @Test
    fun nativeBridgeUnboundedRepeatBecomesStructuredUnsupported() {
        val error = assertThrows(VesperRelayHostInputException::class.java) {
            plan("""<S t="5000" d="2000" r="-1" />""", null)
        }
        assertEquals(415, error.status)
        assertEquals("unsupported_dash_layout", error.diagnostic.code)
        assertTrue(error.diagnostic.details.getValue("hostError").contains("r=-1 requires"))
    }

    @Test
    fun nativeBridgeCannotMaterializeMoreThanTheHostTrackCap() {
        val error = assertThrows(VesperRelayHostInputException::class.java) {
            plan("""<S t="5000" d="1" r="8192" />""", null)
        }
        assertEquals("8193", error.diagnostic.details["segmentCount"])
        assertTrue(error.diagnostic.message.contains("too many segments"))
    }

    private fun plan(entries: String, duration: String?): VesperRelayDashPlan {
        val durationAttribute = duration?.let { "mediaPresentationDuration=\"$it\"" } ?: ""
        return planHostPreparedDash(
            """
            <MPD type="static" $durationAttribute><Period><AdaptationSet mimeType="video/mp4">
              <Representation id="v1">
                <SegmentTemplate timescale="1000" startNumber="7" presentationTimeOffset="5000"
                  initialization="init-${'$'}RepresentationID${'$'}.mp4" media="chunk-${'$'}Time%05d${'$'}-${'$'}Number${'$'}.m4s">
                  <SegmentTimeline>$entries</SegmentTimeline>
                </SegmentTemplate>
              </Representation>
            </AdaptationSet></Period></MPD>
            """.trimIndent(),
            "https://example.com/manifest.mpd",
        )
    }
}
