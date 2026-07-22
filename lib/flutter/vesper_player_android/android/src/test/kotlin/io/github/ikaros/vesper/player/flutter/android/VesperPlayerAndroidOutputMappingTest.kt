package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperExternalSubtitleSource
import io.github.ikaros.vesper.player.android.VesperPlayerDrmConfiguration
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerUnsupportedOperation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlayerAndroidOutputMappingTest {
    @Test
    fun sourceOutputContainsSubtitleMetadataButNeverRequestHeaders() {
        val source =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mpd",
                label = "Video",
                externalSubtitles =
                    listOf(
                        VesperExternalSubtitleSource(
                            id = "caption-en",
                            uri = "https://example.com/en.vtt",
                            mimeType = VesperExternalSubtitleSource.MIME_WEBVTT,
                            language = "en",
                            label = "English",
                            headers = mapOf("Authorization" to "secret"),
                            isDefault = true,
                            isForced = false,
                        ),
                    ),
                headers = mapOf("Authorization" to "media-secret"),
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://example.com/license",
                        licenseHeaders = mapOf("Authorization" to "license-secret"),
                    ),
            )

        val output = source.toMap()
        assertFalse(output.containsKey("headers"))
        assertTrue(output["externalSubtitles"] is List<*>)
        val subtitle = (output["externalSubtitles"] as List<*>).single() as Map<*, *>
        assertEquals("caption-en", subtitle["id"])
        assertEquals("text/vtt", subtitle["mimeType"])
        assertFalse(subtitle.containsKey("headers"))
        val drm = output["drmConfiguration"] as Map<*, *>
        assertFalse(drm.containsKey("licenseHeaders"))
    }

    @Test
    fun subtitleErrorEventEnvelopePreservesStructuredDetails() {
        val methodError =
            VesperPlayerUnsupportedOperation(
                "confirmation timed out",
                mapOf(
                    "domain" to "subtitle",
                    "code" to "subtitle_selection_timeout",
                    "phase" to "selection",
                    "trackId" to "external-en",
                    "retriable" to true,
                    "commandId" to 42L,
                    "sourceEpoch" to 9L,
                    "message" to "confirmation timed out",
                ),
            ).toErrorMap()

        val eventError = methodError.toEventErrorMap()

        assertEquals("backendFailure", eventError["code"])
        assertEquals("platform", eventError["category"])
        assertEquals(true, eventError["retriable"])
        val details = eventError["details"] as Map<*, *>
        assertEquals("subtitle", details["domain"])
        assertEquals("subtitle_selection_timeout", details["code"])
        assertEquals("selection", details["phase"])
        assertEquals("external-en", details["trackId"])
        assertEquals(42L, details["commandId"])
        assertEquals(9L, details["sourceEpoch"])
    }
}
