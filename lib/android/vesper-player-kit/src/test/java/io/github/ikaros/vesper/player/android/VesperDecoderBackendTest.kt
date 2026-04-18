package io.github.ikaros.vesper.player.android

import androidx.media3.exoplayer.DefaultRenderersFactory
import org.junit.Assert.assertEquals
import org.junit.Test

class VesperDecoderBackendTest {
    @Test
    fun systemOnlyDisablesExtensionRenderers() {
        assertEquals(
            DefaultRenderersFactory.EXTENSION_RENDERER_MODE_OFF,
            VesperDecoderBackend.SystemOnly.toExtensionRendererMode(),
        )
    }

    @Test
    fun systemPreferredKeepsSystemDecodersAheadOfExtensions() {
        assertEquals(
            DefaultRenderersFactory.EXTENSION_RENDERER_MODE_ON,
            VesperDecoderBackend.SystemPreferred.toExtensionRendererMode(),
        )
    }

    @Test
    fun extensionPreferredMovesExtensionsAheadOfSystemDecoders() {
        assertEquals(
            DefaultRenderersFactory.EXTENSION_RENDERER_MODE_PREFER,
            VesperDecoderBackend.ExtensionPreferred.toExtensionRendererMode(),
        )
    }
}
