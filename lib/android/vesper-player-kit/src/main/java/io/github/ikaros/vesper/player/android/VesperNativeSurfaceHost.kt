package io.github.ikaros.vesper.player.android

import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.view.Gravity
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout

class VesperNativeSurfaceHost(
    private val bindings: VesperNativeBindings,
    private val surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
) {
    private var hostView: ViewGroup? = null
    private var renderView: View? = null
    private var surface: Surface? = null
    private var videoLayoutInfo: NativeVideoLayoutInfo? = null

    private val hostLayoutListener =
        View.OnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
            applyVideoTransform()
        }

    fun attach(host: ViewGroup) {
        if (hostView === host && renderView != null) {
            applyVideoTransform()
            reattachIfAvailable()
            return
        }

        hostView?.removeOnLayoutChangeListener(hostLayoutListener)

        val existingView = renderView
        if (existingView != null) {
            (existingView.parent as? ViewGroup)?.removeView(existingView)
            host.removeAllViews()
            host.addView(existingView, matchParentLayoutParams())
            hostView = host
            host.addOnLayoutChangeListener(hostLayoutListener)
            applyVideoTransform()
            reattachIfAvailable()
            return
        }

        val view = when (surfaceKind) {
            NativeVideoSurfaceKind.SurfaceView -> createSurfaceView(host)
            NativeVideoSurfaceKind.TextureView -> createTextureView(host)
        }

        host.removeAllViews()
        host.addView(view, matchParentLayoutParams())
        hostView = host
        renderView = view
        host.addOnLayoutChangeListener(hostLayoutListener)
        applyVideoTransform()
    }

    fun reattachIfAvailable() {
        surface?.let { existingSurface ->
            bindings.attachSurface(existingSurface, surfaceKind)
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
        when (surfaceKind) {
            NativeVideoSurfaceKind.TextureView -> {
                surface?.release()
                (renderView as? TextureView)?.surfaceTextureListener = null
            }
            NativeVideoSurfaceKind.SurfaceView -> {
                (renderView as? SurfaceView)?.holder?.removeCallback(surfaceHolderCallback)
            }
        }
        surface = null
        hostView?.removeOnLayoutChangeListener(hostLayoutListener)
        hostView?.removeAllViews()
        renderView = null
        hostView = null
    }

    // ── SurfaceView ─────────────────────────────────────────────────────

    private fun createSurfaceView(host: ViewGroup): SurfaceView =
        SurfaceView(host.context).apply {
            holder.addCallback(surfaceHolderCallback)
        }

    private val surfaceHolderCallback = object : SurfaceHolder.Callback {
        override fun surfaceCreated(holder: SurfaceHolder) {
            val newSurface = holder.surface
            surface = newSurface
            bindings.attachSurface(newSurface, NativeVideoSurfaceKind.SurfaceView)
        }

        override fun surfaceChanged(
            holder: SurfaceHolder,
            format: Int,
            width: Int,
            height: Int,
        ) = Unit

        override fun surfaceDestroyed(holder: SurfaceHolder) {
            bindings.detachSurface()
            surface = null
        }
    }

    // ── TextureView ─────────────────────────────────────────────────────

    private fun createTextureView(host: ViewGroup): TextureView =
        TextureView(host.context).apply {
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

    // ── 宽高比适配 ──────────────────────────────────────────────────────

    private fun applyVideoTransform() {
        when (surfaceKind) {
            NativeVideoSurfaceKind.TextureView -> applyTextureViewTransform()
            NativeVideoSurfaceKind.SurfaceView -> applySurfaceViewLayout()
        }
    }

    private fun applyTextureViewTransform() {
        val view = renderView as? TextureView ?: return
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

    private fun applySurfaceViewLayout() {
        val view = renderView as? SurfaceView ?: return
        val host = hostView ?: return
        val layout = videoLayoutInfo
        val hostWidth = host.width
        val hostHeight = host.height

        if (layout == null || hostWidth <= 0 || hostHeight <= 0 || layout.width <= 0 || layout.height <= 0) {
            val lp = view.layoutParams ?: return
            if (lp.width != ViewGroup.LayoutParams.MATCH_PARENT || lp.height != ViewGroup.LayoutParams.MATCH_PARENT) {
                lp.width = ViewGroup.LayoutParams.MATCH_PARENT
                lp.height = ViewGroup.LayoutParams.MATCH_PARENT
                if (lp is FrameLayout.LayoutParams) lp.gravity = Gravity.CENTER
                view.layoutParams = lp
            }
            return
        }

        val videoAspectRatio =
            (layout.width.toFloat() * layout.pixelWidthHeightRatio) / layout.height.toFloat()
        if (videoAspectRatio <= 0f) return

        val hostAspectRatio = hostWidth.toFloat() / hostHeight.toFloat()
        val targetWidth: Int
        val targetHeight: Int

        if (videoAspectRatio > hostAspectRatio) {
            targetWidth = hostWidth
            targetHeight = (hostWidth / videoAspectRatio).toInt()
        } else {
            targetHeight = hostHeight
            targetWidth = (hostHeight * videoAspectRatio).toInt()
        }

        val lp = view.layoutParams
        if (lp is FrameLayout.LayoutParams) {
            if (lp.width != targetWidth || lp.height != targetHeight || lp.gravity != Gravity.CENTER) {
                lp.width = targetWidth
                lp.height = targetHeight
                lp.gravity = Gravity.CENTER
                view.layoutParams = lp
            }
        }
    }

    private fun matchParentLayoutParams(): FrameLayout.LayoutParams =
        FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT,
            Gravity.CENTER,
        )
}
