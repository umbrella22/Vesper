package io.github.ikaros.vesper.player.android

import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import android.view.ViewGroup

class VesperNativeSurfaceHost(
    private val bindings: VesperNativeBindings,
) {
    private var hostView: ViewGroup? = null
    private var textureView: TextureView? = null
    private var surface: Surface? = null

    fun attach(host: ViewGroup) {
        if (hostView === host && textureView != null) {
            reattachIfAvailable()
            return
        }

        detach()

        val view = TextureView(host.context).apply {
            isOpaque = true
            surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                override fun onSurfaceTextureAvailable(
                    surfaceTexture: SurfaceTexture,
                    width: Int,
                    height: Int,
                ) {
                    val newSurface = Surface(surfaceTexture)
                    surface = newSurface
                    bindings.attachSurface(newSurface, NativeVideoSurfaceKind.TextureView)
                }

                override fun onSurfaceTextureSizeChanged(
                    surfaceTexture: SurfaceTexture,
                    width: Int,
                    height: Int,
                ) = Unit

                override fun onSurfaceTextureDestroyed(surfaceTexture: SurfaceTexture): Boolean {
                    bindings.detachSurface()
                    surface?.release()
                    surface = null
                    return true
                }

                override fun onSurfaceTextureUpdated(surfaceTexture: SurfaceTexture) = Unit
            }
        }

        host.removeAllViews()
        host.addView(
            view,
            ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )

        hostView = host
        textureView = view
    }

    fun reattachIfAvailable() {
        surface?.let { existingSurface ->
            bindings.attachSurface(existingSurface, NativeVideoSurfaceKind.TextureView)
        }
    }

    fun detach() {
        bindings.detachSurface()
        surface?.release()
        surface = null
        textureView?.surfaceTextureListener = null
        hostView?.removeAllViews()
        textureView = null
        hostView = null
    }
}
