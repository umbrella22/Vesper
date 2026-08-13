package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class VesperPluginReferenceTest {
    @Test
    fun referencePreservesIdentityAndKnownTransport() {
        val reference =
            VesperPluginReference(
                pluginId = "dev.vesper.example-plugin",
                capabilityInstanceId = "dev.vesper.example-plugin.decoder",
                transport = VesperPluginTransport.Native,
            )

        assertEquals("dev.vesper.example-plugin", reference.pluginId)
        assertEquals("dev.vesper.example-plugin.decoder", reference.capabilityInstanceId)
        assertEquals("native", reference.transportWireName)
    }

    @Test
    fun decoderPreservesUnknownTransportWithoutNativeFallback() {
        val reference =
            VesperPluginReference.fromWire(
                pluginId = "dev.vesper.example-plugin",
                capabilityInstanceId = null,
                transportRawValue = "future-sandbox",
            )

        assertEquals(VesperPluginTransport.Unknown, reference.transport)
        assertEquals("future-sandbox", reference.transportRawValue)
        assertEquals("future-sandbox", reference.transportWireName)
    }

    @Test
    fun referenceRejectsMissingTransportAndLossyIdentityForms() {
        expectIllegalArgument {
            VesperPluginReference.fromWire(
                pluginId = "dev.vesper.example-plugin",
                capabilityInstanceId = null,
                transportRawValue = "",
            )
        }
        for (invalid in listOf("Vesper.Plugin", " dev.vesper.plugin ", "dev..plugin", "开发.插件")) {
            expectIllegalArgument {
                VesperPluginReference(
                    pluginId = invalid,
                    transport = VesperPluginTransport.Native,
                )
            }
        }
    }

    @Test
    fun bundledRemuxReferenceUsesCanonicalNativeIdentity() {
        assertEquals(
            "io.github.umbrella22.vesper.remux-ffmpeg",
            VesperBundledPluginReferences.remuxFfmpeg.pluginId,
        )
        assertEquals(
            VesperPluginTransport.Native,
            VesperBundledPluginReferences.remuxFfmpeg.transport,
        )
    }

    private fun expectIllegalArgument(block: () -> Unit) {
        try {
            block()
            fail("expected IllegalArgumentException")
        } catch (_: IllegalArgumentException) {
            // Expected.
        }
    }
}
