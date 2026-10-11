package io.github.umbrella22.vesper.player.flutter.externalplayback

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.github.umbrella22.vesper.player.android.external.VesperExternalFallbackFormat
import io.github.umbrella22.vesper.player.android.external.VesperExternalFormatAdaptationConfig
import io.github.umbrella22.vesper.player.android.external.VesperExternalProxyPolicy
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperExternalPlaybackInputTest {
    private val source = mapOf("uri" to "https://fixture.test/video", "kind" to "remote", "protocol" to "progressive")

    @Test
    fun closedPoliciesRejectUnknownTokensAndWrongTypes() {
        for (bad in listOf("future", 7, true, emptyList<Any>(), emptyMap<String, Any>())) {
            assertInvalid("proxyPolicy") { mapOf("proxyPolicy" to bad).toMediaItem() }
            assertInvalid("preferredFallback") { mapOf("preferredFallback" to bad).toFormatAdaptationConfig() }
        }
        for ((token, expected) in listOf("auto" to VesperExternalProxyPolicy.Auto,
            "always" to VesperExternalProxyPolicy.Always, "never" to VesperExternalProxyPolicy.Never)) {
            assertEquals(expected, mapOf("proxyPolicy" to token).toMediaItem().proxyPolicy)
        }
        for ((token, expected) in listOf("mpegTs" to VesperExternalFallbackFormat.MpegTs,
            "hls" to VesperExternalFallbackFormat.Hls)) {
            assertEquals(expected, mapOf("preferredFallback" to token).toFormatAdaptationConfig().preferredFallback)
        }
    }

    @Test
    fun absentAndNullFieldsKeepNativeDefaults() {
        for (item in listOf(emptyMap(), mapOf("sources" to null, "proxyPolicy" to null, "formatAdaptation" to null))) {
            val decoded = item.toMediaItem()
            assertTrue(decoded.sources.isEmpty())
            assertEquals(VesperExternalProxyPolicy.Auto, decoded.proxyPolicy)
            assertEquals(VesperExternalFormatAdaptationConfig(), decoded.formatAdaptation)
        }
        assertEquals(VesperExternalFormatAdaptationConfig(), mapOf(
            "preferredFallback" to null, "remoteDashMediaRequestHeaders" to null,
            "enabled" to null, "allowHls" to null, "enableRangeCache" to null,
            "allowRemoteDashMediaReferences" to null, "allowPrivateRemoteDashMediaAddresses" to null,
            "debugDiagnostics" to null,
        ).toFormatAdaptationConfig())
    }

    @Test
    fun headerListRejectsBadElementsWithoutShortening() {
        for (bad in listOf(null, 7, false, emptyMap<String, Any>())) {
            assertInvalid("remoteDashMediaRequestHeaders[1]") {
                mapOf("remoteDashMediaRequestHeaders" to listOf("Authorization", bad)).toFormatAdaptationConfig()
            }
        }
        for (bad in listOf("Authorization", 7, mapOf("header" to "Authorization"))) {
            assertInvalid("remoteDashMediaRequestHeaders") {
                mapOf("remoteDashMediaRequestHeaders" to bad).toFormatAdaptationConfig()
            }
        }
    }

    @Test
    fun explicitEmptyHeadersAndAwkwardStringsRemainUnchanged() {
        val empty = mapOf("remoteDashMediaRequestHeaders" to emptyList<String>()).toFormatAdaptationConfig()
        assertTrue(empty.remoteDashMediaRequestHeaders.isEmpty())
        val headers = listOf("", " ", " X-Custom 中文 ", "Authorization", "authorization", "Authorization")
        val config = mapOf("remoteDashMediaRequestHeaders" to headers).toFormatAdaptationConfig()
        assertEquals(headers.toSet(), config.remoteDashMediaRequestHeaders)
    }

    @Test
    fun malformedSourcesRejectEntireItemInsteadOfDroppingEntries() {
        for (bad in listOf(null, 7, "not a source")) {
            assertInvalid("sources[1]") { mapOf("sources" to listOf(source, bad)).toMediaItem() }
        }
        for (bad in listOf(7, "not a list", source)) {
            assertInvalid("sources") { mapOf("sources" to bad).toMediaItem() }
        }
        assertInvalid("sources[0]") {
            mapOf("sources" to listOf(mapOf(7 to "field", "uri" to "https://fixture.test"))).toMediaItem()
        }
        val uris = listOf("https://fixture.test/one?x=1", "https://fixture.test/二?x=%20")
        val decoded = mapOf("sources" to uris.map { source + ("uri" to it) }).toMediaItem()
        assertEquals(uris, decoded.sources.map { it.uri })
        assertTrue(mapOf("sources" to emptyList<Any>()).toMediaItem().sources.isEmpty())
    }

    @Test
    fun malformedFormatAdaptationRejectsInsteadOfDisabling() {
        for (bad in listOf(7, false, "not a config", emptyList<Any>(), mapOf(7 to "field"))) {
            assertInvalid("formatAdaptation") { mapOf("formatAdaptation" to bad).toMediaItem() }
        }
        assertTrue(mapOf("formatAdaptation" to mapOf("enabled" to true)).toMediaItem().formatAdaptation.enabled)
    }

    @Test
    fun explicitlyWrongBooleanSettingsFailInsteadOfChangingPolicy() {
        for (key in listOf("enabled", "allowHls", "enableRangeCache", "allowRemoteDashMediaReferences",
            "allowPrivateRemoteDashMediaAddresses", "debugDiagnostics")) {
            for (bad in listOf("true", 1, emptyList<Any>())) {
                assertInvalid(key) { mapOf(key to bad).toFormatAdaptationConfig() }
            }
        }
        val config = mapOf("enabled" to true, "allowHls" to false, "enableRangeCache" to false,
            "allowRemoteDashMediaReferences" to true, "allowPrivateRemoteDashMediaAddresses" to true,
            "debugDiagnostics" to true).toFormatAdaptationConfig()
        assertTrue(config.enabled)
        assertFalse(config.allowHls)
        assertFalse(config.enableRangeCache)
        assertTrue(config.allowRemoteDashMediaReferences)
        assertTrue(config.allowPrivateRemoteDashMediaAddresses)
        assertTrue(config.debugDiagnostics)
    }

    @Test
    fun invalidLoadReturnsExistingChannelErrorBeforeControllerAccess() {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        try {
            val plugin = VesperPlayerExternalPlaybackPlugin()
            // Decode failures must be reported before an external controller is required.
            plugin.javaClass.getDeclaredField("scope").apply { isAccessible = true }.set(plugin, scope)
            val badItems = listOf(
                "not an item", 7, mapOf(7 to "field"),
                mapOf("proxyPolicy" to "future"),
                mapOf("sources" to listOf(source, null)),
                mapOf("formatAdaptation" to "not a config"),
                mapOf("formatAdaptation" to mapOf("preferredFallback" to false)),
                mapOf("formatAdaptation" to mapOf("remoteDashMediaRequestHeaders" to listOf("Accept", 7))),
            )
            for (item in badItems) {
                var errors = 0
                plugin.onMethodCall(MethodCall("load", mapOf("item" to item)), object : MethodChannel.Result {
                    override fun success(result: Any?) { fail("Malformed caller input must fail load") }
                    override fun notImplemented() { fail("Load must be implemented") }
                    override fun error(errorCode: String, errorMessage: String?, errorDetails: Any?) {
                        errors += 1
                        assertEquals("vesper_external_playback_error", errorCode)
                        assertTrue(errorMessage.orEmpty().isNotBlank())
                        assertEquals(IllegalArgumentException::class.java.name, (errorDetails as Map<*, *>)["exception"])
                        assertEquals(false, errorDetails["retriable"])
                    }
                })
                assertEquals(1, errors)
            }
        } finally {
            scope.cancel()
        }
    }

    private fun assertInvalid(field: String, decode: () -> Any) {
        try {
            decode()
            fail("Expected invalid $field to fail")
        } catch (error: IllegalArgumentException) {
            assertTrue(error.message.orEmpty().contains(field))
        }
    }
}
