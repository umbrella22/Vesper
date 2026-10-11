package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperBackgroundPlaybackMode
import io.github.umbrella22.vesper.player.android.VesperBufferingPreset
import io.github.umbrella22.vesper.player.android.VesperCachePreset
import io.github.umbrella22.vesper.player.android.VesperRetryBackoff
import io.github.umbrella22.vesper.player.android.VesperSystemPlaybackControlKind
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperPlayerAndroidCallerPolicyInputTest {
    @Test
    fun closedCallerTokensRejectUnknownStringsAndWrongTypes() {
        for (bad in listOf("future-token", 7, false, emptyList<String>(), emptyMap<String, Any?>())) {
            val cases: List<Pair<String, () -> Any>> = listOf(
                "backgroundMode" to { mapOf("backgroundMode" to bad).toSystemPlaybackConfiguration() },
                "kind" to { mapOf("kind" to bad).toSystemPlaybackControlButton() },
                "buffering preset" to { mapOf("preset" to bad).toBufferingPolicy() },
                "cache preset" to { mapOf("preset" to bad).toCachePolicy() },
                "backoff" to { mapOf("backoff" to bad).toRetryPolicy() },
                "mode" to { mapOf("mode" to bad).toFrameProcessorConfiguration() },
            )
            for ((field, decode) in cases) {
                assertInvalid(field, decode)
            }
        }
    }

    @Test
    fun absentNullAndSupportedDefaultTokensKeepTheirDefaults() {
        for (map in listOf(emptyMap(), mapOf("backgroundMode" to null), mapOf("backgroundMode" to "continueAudio"))) {
            assertEquals(VesperBackgroundPlaybackMode.ContinueAudio, map.toSystemPlaybackConfiguration().backgroundMode)
        }
        for (map in listOf(emptyMap(), mapOf("kind" to null), mapOf("kind" to "playPause"))) {
            assertEquals(VesperSystemPlaybackControlKind.PlayPause, map.toSystemPlaybackControlButton().kind)
        }
        for (map in listOf(emptyMap(), mapOf("preset" to null), mapOf("preset" to "default"),
            mapOf("preset" to "defaultPreset"))) {
            assertEquals(VesperBufferingPreset.Default, map.toBufferingPolicy().preset)
            assertEquals(VesperCachePreset.Default, map.toCachePolicy().preset)
        }
        for (map in listOf(emptyMap(), mapOf("backoff" to null))) {
            assertEquals(VesperRetryBackoff.Linear, map.toRetryPolicy().backoff)
        }
        assertEquals(VesperBackgroundPlaybackMode.Disabled,
            mapOf("backgroundMode" to "disabled").toSystemPlaybackConfiguration().backgroundMode)
        for ((token, expected) in listOf("balanced" to VesperBufferingPreset.Balanced,
            "streaming" to VesperBufferingPreset.Streaming, "resilient" to VesperBufferingPreset.Resilient,
            "lowLatency" to VesperBufferingPreset.LowLatency)) {
            assertEquals(expected, mapOf("preset" to token).toBufferingPolicy().preset)
        }
        for ((token, expected) in listOf("disabled" to VesperCachePreset.Disabled,
            "streaming" to VesperCachePreset.Streaming, "resilient" to VesperCachePreset.Resilient)) {
            assertEquals(expected, mapOf("preset" to token).toCachePolicy().preset)
        }
        for ((token, expected) in listOf("fixed" to VesperRetryBackoff.Fixed,
            "linear" to VesperRetryBackoff.Linear, "exponential" to VesperRetryBackoff.Exponential)) {
            assertEquals(expected, mapOf("backoff" to token).toRetryPolicy().backoff)
        }
    }

    @Test
    fun retryMaxAttemptsPreservesNullAndNumbersButRejectsExplicitWrongTypes() {
        assertEquals(3, emptyMap<String, Any?>().toRetryPolicy().maxAttempts)
        assertNull(mapOf("maxAttempts" to null).toRetryPolicy().maxAttempts)
        for (bad in listOf(1.0, 1.5, Long.MAX_VALUE)) {
            assertInvalid("maxAttempts") { mapOf("maxAttempts" to bad).toRetryPolicy() }
        }
        for (attempts in listOf(0, -1, 6, Int.MAX_VALUE)) {
            assertEquals(attempts, mapOf("maxAttempts" to attempts).toRetryPolicy().maxAttempts)
            assertEquals(attempts, mapOf("maxAttempts" to attempts.toLong()).toRetryPolicy().maxAttempts)
        }
        for (bad in listOf("6", false, emptyList<String>(), emptyMap<String, Any?>())) {
            assertInvalid("maxAttempts") { mapOf("maxAttempts" to bad).toRetryPolicy() }
        }
    }

    @Test
    fun compactButtonsRejectWholeMalformedCollectionAndKeepValidOrdering() {
        val button = mapOf("kind" to "playPause")
        for (bad in listOf(null, 7, "not a button")) {
            assertInvalid("compactButtons") { mapOf("compactButtons" to listOf(button, bad)).toSystemPlaybackControls() }
        }
        for (bad in listOf(7, "not a list", button)) {
            assertInvalid("compactButtons") { mapOf("compactButtons" to bad).toSystemPlaybackControls() }
        }
        for (map in listOf(emptyMap(), mapOf("compactButtons" to null), mapOf("compactButtons" to emptyList<Any>()))) {
            assertTrue(map.toSystemPlaybackControls().compactButtons.isEmpty())
        }
        val controls = mapOf("compactButtons" to listOf(mapOf("kind" to "seekForward", "seekOffsetMs" to 12_345L),
            button, mapOf("kind" to "seekBack", "seekOffsetMs" to 2_345L))).toSystemPlaybackControls()
        assertEquals(listOf(VesperSystemPlaybackControlKind.SeekForward, VesperSystemPlaybackControlKind.PlayPause,
            VesperSystemPlaybackControlKind.SeekBack), controls.compactButtons.map { it.kind })
        assertEquals(12_345L, controls.compactButtons[0].seekOffsetMs)
        assertEquals(2_345L, controls.compactButtons[2].seekOffsetMs)
        assertInvalid("controls") { mapOf("controls" to "not a map").toSystemPlaybackConfiguration() }
    }

    @Test
    fun invalidResilienceConfigurationReturnsExistingCreationChannelError() {
        for ((field, badPolicy) in listOf(
            "maxAttempts" to mapOf("retry" to mapOf("maxAttempts" to "6")),
            "backoff" to mapOf("retry" to mapOf("backoff" to "future")),
            "preset" to mapOf("buffering" to mapOf("preset" to 7)),
            "preset" to mapOf("cache" to mapOf("preset" to "future")),
        )) {
            var errors = 0
            VesperPlayerAndroidPlugin().onMethodCall(MethodCall("createPlayer", mapOf("resiliencePolicy" to badPolicy)),
                object : MethodChannel.Result {
                    override fun success(result: Any?) { fail("Invalid policy must fail creation") }
                    override fun notImplemented() { fail("Creation must be implemented") }
                    override fun error(code: String, message: String?, details: Any?) {
                        errors += 1
                        assertEquals("vesper_create_failed", code)
                        assertTrue(message.orEmpty().contains(field))
                        assertEquals(false, (details as Map<*, *>)["retriable"])
                    }
                })
            assertEquals(1, errors)
        }
    }

    private fun assertInvalid(field: String, decode: () -> Any?) {
        try {
            decode()
            fail("Invalid supplied $field must fail")
        } catch (error: IllegalArgumentException) {
            assertTrue(error.message.orEmpty(), error.message.orEmpty().contains(field))
        }
    }
}
