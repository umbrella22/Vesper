package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.assertThrows
import org.junit.Test

class VesperRelayDashTimelineTest {
    @Test
    fun plansTheDlnaFixtureWithAllSixVideoAndAudioFragments() {
        val bridge = RecordedDashTemplateBridge()
        withDashTemplateBridgeExecutor(bridge) {
            val manifest = javaClass.getResourceAsStream("/dlna-segment-timeline.mpd")!!
                .bufferedReader().use { it.readText() }
            val plan = planHostPreparedDash(manifest, "https://example.com/dash/manifest.mpd")
            assertEquals(listOf("video", "audio"), plan.tracks.map { it.kind })
            plan.tracks.forEachIndexed { trackIndex, track ->
                assertEquals("https://example.com/dash/init-stream$trackIndex.m4s", track.initializationUri)
                assertEquals(listOf(1L, 2L, 3L, 4L, 5L, 6L), track.segments.map { it.index })
                assertEquals(
                    (1..6).map { "https://example.com/dash/chunk-stream$trackIndex-${it.toString().padStart(5, '0')}.m4s" },
                    track.segments.map { it.uri },
                )
            }
            val requests = bridge.requests.filter { it.getString("operation") == "template_segments" }
            assertEquals(2, requests.size)
            assertEquals(3, requests.last().getJSONObject("segmentTemplate").getJSONArray("timeline").length())
        }
    }

    @Test
    fun timeTokensUseRawTimelineTicksWithPresentationTimeOffset() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            val track = timelinePlan(
                entries = """<S t="5000" d="2000" r="2" /><S d="1000" />""",
                templateAttributes = """timescale="1000" startNumber="7" presentationTimeOffset="5000" """,
                mpdAttributes = """type="static" mediaPresentationDuration="PT7S" """,
                media = "chunk-${'$'}Time%05d${'$'}-${'$'}Number${'$'}.m4s",
            ).tracks.single()
            assertEquals(listOf(7L, 8L, 9L, 10L), track.segments.map { it.index })
            assertEquals(
                listOf("chunk-05000-7.m4s", "chunk-07000-8.m4s", "chunk-09000-9.m4s", "chunk-11000-10.m4s"),
                track.segments.map { it.uri.substringAfterLast('/') },
            )
        }
    }

    @Test
    fun negativeRepeatCanEndAtTheNextExplicitTimeWithoutMpdDuration() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            val track = timelinePlan(
                entries = """<S t="1000" d="1000" r="-1" /><S t="4000" d="500" />""",
                templateAttributes = """timescale="1000" startNumber="20" presentationTimeOffset="1000" """,
                mpdAttributes = """type="dynamic" """,
            ).tracks.single()
            assertEquals(listOf(20L, 21L, 22L, 23L), track.segments.map { it.index })
            assertEquals(listOf("chunk-1000.m4s", "chunk-2000.m4s", "chunk-3000.m4s", "chunk-4000.m4s"),
                track.segments.map { it.uri.substringAfterLast('/') })
        }
    }

    @Test
    fun negativeRepeatCanEndAtFiniteMpdDurationIncludingTheOffset() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            val track = timelinePlan(
                entries = """<S t="5000" d="2000" r="-1" />""",
                templateAttributes = """timescale="1000" presentationTimeOffset="5000" """,
                mpdAttributes = """type="static" mediaPresentationDuration="PT5S" """,
            ).tracks.single()
            assertEquals(listOf("chunk-5000.m4s", "chunk-7000.m4s", "chunk-9000.m4s"),
                track.segments.map { it.uri.substringAfterLast('/') })
        }
    }

    @Test
    fun unboundedNegativeRepeatPreservesBridgeErrorAsUnsupportedLayout() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            val error = assertThrows(VesperRelayHostInputException::class.java) {
                timelinePlan("""<S t="0" d="4" r="-1" />""", mpdAttributes = """type="dynamic" """ )
            }
            assertEquals(415, error.status)
            assertEquals("unsupported_dash_layout", error.diagnostic.code)
            assertTrue(error.diagnostic.details.getValue("hostError").contains("requires next S@t or mediaPresentationDuration"))
        }
    }

    @Test
    fun duplicateAndDecreasingExplicitTimesPreserveBridgeErrors() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            for (firstTime in listOf(1000, 2000)) {
                val error = assertThrows(VesperRelayHostInputException::class.java) {
                    timelinePlan("""<S t="$firstTime" d="1000" /><S t="1000" d="1000" />""")
                }
                assertEquals("unsupported_dash_layout", error.diagnostic.code)
                assertTrue(error.diagnostic.details.getValue("hostError").contains("next S@t"))
            }
        }
    }

    @Test
    fun rejectsInvalidTimelineAttributesBeforeCallingNative() {
        val bridge = RecordedDashTemplateBridge()
        withDashTemplateBridgeExecutor(bridge) {
            val invalidEntries = listOf(
                """<S d="0" />""", """<S d="-1" />""", """<S />""", """<S d="bad" />""",
                """<S t="bad" d="1" />""", """<S t="-1" d="1" />""",
                """<S t="9223372036854775808" d="1" />""", """<S d="1" r="-2" />""",
                """<S d="1" r="2147483648" />""", """<S d="1" r="1.5" />""", "",
            )
            for (entry in invalidEntries) {
                val error = assertThrows(VesperRelayHostInputException::class.java) { timelinePlan(entry) }
                assertEquals("unsupported_dash_layout", error.diagnostic.code)
            }
            assertTrue(bridge.requests.isEmpty())
        }
    }

    @Test
    fun rejectsInvalidTemplateAttributesInsteadOfUsingDefaults() {
        val bridge = RecordedDashTemplateBridge()
        withDashTemplateBridgeExecutor(bridge) {
            for (attributes in listOf(
                """timescale="0" """, """timescale="bad" """, """timescale="" """,
                """startNumber="bad" """, """startNumber="-1" """,
                """presentationTimeOffset="bad" """, """presentationTimeOffset="-1" """,
                """duration="0" """, """duration="9223372036854775808" """,
            )) {
                assertThrows(VesperRelayHostInputException::class.java) {
                    timelinePlan("""<S d="1000" />""", templateAttributes = attributes)
                }
            }
            assertTrue(bridge.requests.isEmpty())
        }
    }

    @Test
    fun timelineInAnUnselectedRepresentationDoesNotRejectTheSelectedFixedTemplate() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            val plan = planHostPreparedDash(
                """
                <MPD type="static" mediaPresentationDuration="PT4S"><Period>
                  <AdaptationSet mimeType="video/mp4">
                    <Representation id="v1"><SegmentTemplate duration="4" initialization="init.mp4" media="chunk-${'$'}Number${'$'}.m4s" /></Representation>
                    <Representation id="unused"><SegmentTemplate initialization="init.mp4" media="chunk-${'$'}Time${'$'}.m4s"><SegmentTimeline><S d="bad" /></SegmentTimeline></SegmentTemplate></Representation>
                  </AdaptationSet>
                </Period></MPD>
                """.trimIndent(),
                "https://example.com/manifest.mpd",
            )
            assertEquals("https://example.com/chunk-1.m4s", plan.tracks.single().segments.single().uri)
        }
    }

    @Test
    fun rejectsTimelineResultsPastTheHostSegmentCap() {
        val response = JSONArray()
        repeat(MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK + 1) {
            response.put(JSONObject().put("number", 1).put("time", 0).put("duration", 1.0).put("presentationTime", 0))
        }
        withDashTemplateBridgeExecutor({ response.toString() }) {
            val error = assertThrows(VesperRelayHostInputException::class.java) {
                timelinePlan("""<S d="1" r="8192" />""", mpdAttributes = """type="static" """ )
            }
            assertTrue(error.diagnostic.message.contains("too many segments"))
            assertEquals("8193", error.diagnostic.details["segmentCount"])
        }
    }

    @Test
    fun rejectsBridgeIdentitiesOutsideTheHostLongRangeWithoutWrapping() {
        withDashTemplateBridgeExecutor({ """[{"number":9223372036854775808,"time":0,"duration":1.0,"presentationTime":0}]""" }) {
            val error = assertThrows(VesperRelayHostInputException::class.java) {
                timelinePlan("""<S d="1" />""")
            }
            assertTrue(error.diagnostic.details.getValue("hostError").contains("host Long range"))
        }
    }

    @Test
    fun sharedUriExpansionKeepsBandwidthFormattingAndEscapedDollar() {
        withDashTemplateBridgeExecutor(RecordedDashTemplateBridge()) {
            assertEquals("cash-${'$'}-v1-00001234-007-5000.m4s", JniVesperRelayDashBridgeApi.expandTemplate(
                "cash-${'$'}${'$'}-${'$'}RepresentationID${'$'}-${'$'}Bandwidth%08d${'$'}-${'$'}Number%03d${'$'}-${'$'}Time${'$'}.m4s",
                "v1", 1234L, 7L, 5000L,
            ))
        }
    }

    private fun timelinePlan(
        entries: String,
        templateAttributes: String = """timescale="1000" """,
        mpdAttributes: String = """type="static" mediaPresentationDuration="PT4S" """,
        media: String = "chunk-${'$'}Time${'$'}.m4s",
    ): VesperRelayDashPlan = planHostPreparedDash(
        """
        <MPD $mpdAttributes><Period><AdaptationSet mimeType="video/mp4"><Representation id="v1">
          <SegmentTemplate $templateAttributes initialization="init.mp4" media="$media">
            <SegmentTimeline>$entries</SegmentTimeline>
          </SegmentTemplate>
        </Representation></AdaptationSet></Period></MPD>
        """.trimIndent(),
        "https://example.com/manifest.mpd",
    )
}
