package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class VesperSubtitleRequestHeadersTest {
    @Test
    fun externalSubtitleRequestDoesNotInheritMediaAuthorization() {
        val subtitleUri = "https://subtitle.example/sub-en.vtt"
        val headers =
            resolveResourceRequestHeaders(
                role = NativeResourceRequestRole.ExternalSubtitle,
                mediaHeaders = mapOf("Authorization" to "media-secret"),
                subtitleHeaders = mapOf("X-Subtitle" to "subtitle-token"),
            )

        assertEquals("subtitle-token", headers["X-Subtitle"])
        assertFalse(headers.containsKey("Authorization"))
    }

    @Test
    fun externalSubtitleWithNoHeadersDoesNotInheritMediaHeaders() {
        val subtitleUri = "https://subtitle.example/sub-en.vtt"
        val headers =
            resolveResourceRequestHeaders(
                role = NativeResourceRequestRole.ExternalSubtitle,
                mediaHeaders = mapOf("Authorization" to "media-secret"),
                subtitleHeaders = emptyMap(),
            )

        assertEquals(emptyMap<String, String>(), headers)
    }

    @Test
    fun mediaRequestRetainsMediaHeaders() {
        val headers =
            resolveResourceRequestHeaders(
                role = NativeResourceRequestRole.Media,
                mediaHeaders = mapOf("Authorization" to "media-secret"),
                subtitleHeaders = mapOf("X-Subtitle" to "subtitle-token"),
            )

        assertEquals("media-secret", headers["Authorization"])
    }

    @Test
    fun equalMediaAndSubtitleUrisDoNotChangeHeaderOwnership() {
        val mediaHeaders = mapOf("Authorization" to "media-secret")
        val subtitleHeaders = mapOf("Authorization" to "subtitle-secret")

        assertEquals(
            mediaHeaders,
            resolveResourceRequestHeaders(
                role = NativeResourceRequestRole.Media,
                mediaHeaders = mediaHeaders,
                subtitleHeaders = subtitleHeaders,
            ),
        )
        assertEquals(
            subtitleHeaders,
            resolveResourceRequestHeaders(
                role = NativeResourceRequestRole.ExternalSubtitle,
                mediaHeaders = mediaHeaders,
                subtitleHeaders = subtitleHeaders,
            ),
        )
    }
}
