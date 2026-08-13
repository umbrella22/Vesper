package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperExternalSubtitleSource
import io.github.umbrella22.vesper.player.android.VesperFixedTrackSelectionException
import io.github.umbrella22.vesper.player.android.VesperPlayerCommandException
import io.github.umbrella22.vesper.player.android.VesperPlayerErrorCategory
import io.github.umbrella22.vesper.player.android.VesperPlayerErrorCode
import io.github.umbrella22.vesper.player.android.VesperPlayerErrorState
import io.github.umbrella22.vesper.player.android.VesperPlayerDrmConfiguration
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperPlayerUnsupportedOperation
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

    @Test
    fun fixedTrackErrorPreservesCapabilityEvidence() {
        val error =
            VesperFixedTrackSelectionException(
                code = "trackExceedsCapabilities",
                trackId = "video:4k",
                expectedCatalogRevision = 4L,
                actualCatalogRevision = 5L,
                message = "track rejected",
                extraDetails =
                    mapOf(
                        "reason" to "formatExceedsCapabilities",
                        "formatSupportRawValue" to "exceedsCapabilities",
                        "futureEvidence" to mapOf("renderer" to "video"),
                    ),
            ).toErrorMap()

        assertEquals("fixedTrack", error["domain"])
        assertEquals("trackExceedsCapabilities", error["code"])
        assertEquals("formatExceedsCapabilities", error["reason"])
        assertEquals("exceedsCapabilities", error["formatSupportRawValue"])
        val futureEvidence = error["futureEvidence"] as Map<*, *>
        assertEquals("video", futureEvidence["renderer"])
    }

    @Test
    fun genericAbrPolicyCommandErrorKeepsItsOwnTaxonomy() {
        val error =
            VesperPlayerCommandException(
                VesperPlayerErrorState(
                    message = "constraints are required",
                    code = VesperPlayerErrorCode.InvalidArgument,
                    category = VesperPlayerErrorCategory.Input,
                    retriable = false,
                    details =
                        mapOf(
                            "domain" to "abrPolicy",
                            "operation" to "setAbrPolicy",
                        ),
                )
            ).toErrorMap()

        assertEquals("invalidArgument", error["code"])
        assertEquals("input", error["category"])
        assertEquals(false, error["retriable"])
        val details = error["details"] as Map<*, *>
        assertEquals("abrPolicy", details["domain"])
        assertEquals("setAbrPolicy", details["operation"])
    }
}
