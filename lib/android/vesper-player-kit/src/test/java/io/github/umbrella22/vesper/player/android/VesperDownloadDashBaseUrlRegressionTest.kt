package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Test

class VesperDownloadDashBaseUrlRegressionTest {
    @Test
    fun periodBaseUrlParticipatesInEveryRepresentationResource() {
        val manifest = """
            <MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><BaseURL>movie/</BaseURL>
            <Period><BaseURL>media/</BaseURL><AdaptationSet mimeType="video/mp4"><BaseURL>video/</BaseURL>
            <SegmentTemplate initialization="init.mp4" media="seg-${'$'}Number${'$'}.m4s" duration="2" timescale="1"/>
            <Representation id="v"><BaseURL>high/</BaseURL></Representation>
            </AdaptationSet></Period></MPD>
        """.trimIndent()
        val selected = selectDashRepresentations(parseXmlDocument(manifest), "https://fixture.test/root/manifest.mpd", VesperDownloadProfile()).single()
        assertEquals("https://fixture.test/root/movie/media/video/high/", selected.baseUri)
        assertEquals("https://fixture.test/root/movie/media/video/high/init.mp4", resolveRemoteReference(selected.baseUri, selected.template!!.initialization!!))
    }

    @Test
    fun absentPeriodBaseUrlAndAbsoluteChildKeepTheirMeaning() {
        val manifest = """
            <MPD><BaseURL>movie/</BaseURL><Period><AdaptationSet mimeType="video/mp4">
            <Representation id="v"><BaseURL>https://cdn.fixture.test/clip.mp4?a=1&amp;b=2</BaseURL></Representation>
            </AdaptationSet></Period></MPD>
        """.trimIndent()
        val selected = selectDashRepresentations(parseXmlDocument(manifest), "https://fixture.test/root/manifest.mpd", VesperDownloadProfile()).single()
        assertEquals("https://cdn.fixture.test/clip.mp4?a=1&b=2", selected.baseUri)
    }
}
