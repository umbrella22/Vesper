package io.github.ikaros.vesper.player.android

import androidx.media3.exoplayer.DefaultRenderersFactory

enum class VesperDecoderBackend {
    SystemOnly,
    SystemPreferred,
    ExtensionPreferred,
}

internal fun VesperDecoderBackend.toExtensionRendererMode(): Int =
    when (this) {
        VesperDecoderBackend.SystemOnly -> DefaultRenderersFactory.EXTENSION_RENDERER_MODE_OFF
        VesperDecoderBackend.SystemPreferred -> DefaultRenderersFactory.EXTENSION_RENDERER_MODE_ON
        VesperDecoderBackend.ExtensionPreferred ->
            DefaultRenderersFactory.EXTENSION_RENDERER_MODE_PREFER
    }
