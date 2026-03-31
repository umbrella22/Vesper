package io.github.ikaros.vesper.player.android

import android.graphics.Matrix
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
    private var videoLayoutInfo: NativeVideoLayoutInfo? = null

    fun attach(host: ViewGroup) {
        if (hostView === host && textureView != null) {
            applyVideoTransform()
            reattachIfAvailable()
            return
        }

        val existingView = textureView
        if (existingView != null) {
            (existingView.parent as? ViewGroup)?.removeView(existingView)
            host.removeAllViews()
            host.addView(
                existingView,
                ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                ),
            )
            hostView = host
            applyVideoTransform()
            reattachIfAvailable()
            return
        }

        val view = TextureView(host.context).apply {
            isOpaque = true
            addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
                applyVideoTransform()
            }
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
        applyVideoTransform()
    }

    fun reattachIfAvailable() {
        surface?.let { existingSurface ->
            bindings.attachSurface(existingSurface, NativeVideoSurfaceKind.TextureView)
        }
    }

    fun updateVideoLayout(layoutInfo: NativeVideoLayoutInfo?) {
        videoLayoutInfo = layoutInfo
        applyVideoTransform()
    }

    fun detach(expectedHost: ViewGroup? = null) {
        if (expectedHost != null && hostView !== expectedHost) {
            return
        }
        bindings.detachSurface()
        surface?.release()
        surface = null
        textureView?.surfaceTextureListener = null
        hostView?.removeAllViews()
        textureView = null
        hostView = null
    }

    private fun applyVideoTransform() {
        val view = textureView ?: return
        val layout = videoLayoutInfo
        val viewWidth = view.width.toFloat()
        val viewHeight = view.height.toFloat()

        if (layout == null || viewWidth <= 0f || viewHeight <= 0f || layout.width <= 0 || layout.height <= 0) {
            view.setTransform(Matrix())
            return
        }

        val videoAspectRatio =
            (layout.width.toFloat() * layout.pixelWidthHeightRatio) / layout.height.toFloat()
        if (videoAspectRatio <= 0f) {
            view.setTransform(Matrix())
            return
        }

        val viewAspectRatio = viewWidth / viewHeight
        val scaleX: Float
        val scaleY: Float

        if (videoAspectRatio > viewAspectRatio) {
            scaleX = 1.0f
            scaleY = viewAspectRatio / videoAspectRatio
        } else {
            scaleX = videoAspectRatio / viewAspectRatio
            scaleY = 1.0f
        }

        val matrix =
            Matrix().apply {
                setScale(scaleX, scaleY, viewWidth / 2f, viewHeight / 2f)
            }
        view.setTransform(matrix)
    }
}
