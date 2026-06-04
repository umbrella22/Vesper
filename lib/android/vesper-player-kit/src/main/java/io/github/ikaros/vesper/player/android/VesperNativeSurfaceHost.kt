package io.github.ikaros.vesper.player.android

import android.graphics.Matrix
import android.graphics.Color
import android.graphics.SurfaceTexture
import android.util.Log
import android.view.Gravity
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout

internal class VesperNativeSurfaceHost(
    private val bindings: VesperNativeBindings,
    private val surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
) {
    private var hostView: ViewGroup? = null
    private var renderView: View? = null
    private var surface: Surface? = null
    private var attachedSurface: Surface? = null
    private var attachedSurfaceKind: NativeVideoSurfaceKind? = null
    private var videoLayoutInfo: NativeVideoLayoutInfo? = null
    private var keepScreenOn = false

    private val hostLayoutListener =
        View.OnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
            applyVideoTransform()
        }

    fun attach(host: ViewGroup) {
        Log.d(
            TAG,
            "surfaceHost attach kind=$surfaceKind reuse=${renderView != null} " +
                "hostAttached=${host.isAttachedToWindow} hostSize=${host.width}x${host.height} " +
                "hostChildren=${host.childCount}",
        )
        host.setBackgroundColor(Color.BLACK)
        if (hostView === host && renderView != null) {
            applyVideoTransform()
            reattachIfAvailable()
            postSurfaceViewAttachCheck("same-host")
            return
        }

        hostView?.removeOnLayoutChangeListener(hostLayoutListener)

        val existingView = renderView
        if (existingView != null) {
            Log.d(TAG, "surfaceHost moving existing $surfaceKind render view to new host")
            (existingView.parent as? ViewGroup)?.removeView(existingView)
            host.removeAllViews()
            host.addView(existingView, matchParentLayoutParams())
            hostView = host
            host.addOnLayoutChangeListener(hostLayoutListener)
            applyVideoTransform()
            reattachIfAvailable()
            postSurfaceViewAttachCheck("move-host")
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
        applyKeepScreenOn()
        host.addOnLayoutChangeListener(hostLayoutListener)
        applyVideoTransform()
        postSurfaceViewAttachCheck("attach-created")
    }

    fun reattachIfAvailable() {
        val existingSurface = surface
        if (existingSurface == null) {
            Log.d(TAG, "surfaceHost reattach skipped kind=$surfaceKind reason=no-surface")
            return
        }
        if (!existingSurface.isValid) {
            Log.d(TAG, "surfaceHost reattach skipped kind=$surfaceKind reason=invalid-surface")
            return
        }
        if (attachedSurface === existingSurface && attachedSurfaceKind == surfaceKind) {
            Log.d(TAG, "surfaceHost reattach skipped kind=$surfaceKind reason=already-attached")
            return
        }
        Log.d(TAG, "surfaceHost reattach kind=$surfaceKind")
        rememberAttachedSurface(existingSurface, surfaceKind)
        bindings.attachSurface(existingSurface, surfaceKind)
    }

    fun updateVideoLayout(layoutInfo: NativeVideoLayoutInfo?) {
        videoLayoutInfo = layoutInfo
        applyVideoTransform()
    }

    fun setKeepScreenOn(active: Boolean) {
        keepScreenOn = active
        applyKeepScreenOn()
    }

    fun detach(expectedHost: ViewGroup? = null) {
        detach(expectedHost = expectedHost, notifyNative = true)
    }

    fun detachWithoutNativeNotification(expectedHost: ViewGroup? = null) {
        detach(expectedHost = expectedHost, notifyNative = false)
    }

    private fun detach(
        expectedHost: ViewGroup? = null,
        notifyNative: Boolean,
    ) {
        if (expectedHost != null && hostView !== expectedHost) {
            return
        }
        Log.d(
            TAG,
            "surfaceHost detach kind=$surfaceKind notifyNative=$notifyNative " +
                "hasSurface=${surface != null}",
        )
        setKeepScreenOn(false)
        if (notifyNative) {
            bindings.detachSurface()
        }
        clearAttachedSurface()
        when (surfaceKind) {
            NativeVideoSurfaceKind.TextureView -> {
                surface?.release()
                (renderView as? TextureView)?.surfaceTextureListener = null
            }
            NativeVideoSurfaceKind.SurfaceView -> {
                (renderView as? SurfaceView)?.let { view ->
                    view.holder.removeCallback(surfaceHolderCallback)
                    view.removeOnAttachStateChangeListener(surfaceViewAttachStateListener)
                }
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
            Log.d(TAG, "surfaceHost create SurfaceView")
            holder.addCallback(surfaceHolderCallback)
            addOnAttachStateChangeListener(surfaceViewAttachStateListener)
            keepScreenOn = this@VesperNativeSurfaceHost.keepScreenOn
        }

    private val surfaceHolderCallback = object : SurfaceHolder.Callback {
        override fun surfaceCreated(holder: SurfaceHolder) {
            Log.d(
                TAG,
                "surfaceHost SurfaceView surfaceCreated valid=${holder.surface?.isValid == true}",
            )
            attachSurfaceHolderIfValid(holder, reason = "surfaceCreated")
        }

        override fun surfaceChanged(
            holder: SurfaceHolder,
            format: Int,
            width: Int,
            height: Int,
        ) {
            Log.d(
                TAG,
                "surfaceHost SurfaceView surfaceChanged format=$format size=${width}x$height " +
                    "valid=${holder.surface?.isValid == true}",
            )
            attachSurfaceHolderIfValid(holder, reason = "surfaceChanged")
        }

        override fun surfaceDestroyed(holder: SurfaceHolder) {
            Log.d(TAG, "surfaceHost SurfaceView surfaceDestroyed")
            bindings.detachSurface()
            clearAttachedSurface(holder.surface)
            surface = null
        }
    }

    private val surfaceViewAttachStateListener =
        object : View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(view: View) {
                Log.d(TAG, "surfaceHost SurfaceView attachedToWindow")
                val capturedView = view as SurfaceView
                view.post {
                    if (!isCurrentSurfaceView(capturedView)) {
                        Log.d(TAG, "surfaceHost SurfaceView attach skipped reason=stale-viewAttached")
                        return@post
                    }
                    attachSurfaceHolderIfValid(
                        capturedView.holder,
                        reason = "viewAttached",
                    )
                }
            }

            override fun onViewDetachedFromWindow(view: View) {
                Log.d(TAG, "surfaceHost SurfaceView detachedFromWindow")
            }
        }

    private fun postSurfaceViewAttachCheck(reason: String) {
        val view = renderView as? SurfaceView ?: return
        view.post {
            if (!isCurrentSurfaceView(view)) {
                Log.d(TAG, "surfaceHost SurfaceView attach skipped reason=stale-$reason")
                return@post
            }
            attachSurfaceHolderIfValid(view.holder, reason = reason)
        }
    }

    private fun attachSurfaceHolderIfValid(
        holder: SurfaceHolder,
        reason: String,
    ): Boolean {
        val newSurface = holder.surface
        if (newSurface == null || !newSurface.isValid) {
            Log.d(TAG, "surfaceHost SurfaceView attach skipped reason=$reason valid=false")
            return false
        }
        surface = newSurface
        if (attachedSurface === newSurface && attachedSurfaceKind == NativeVideoSurfaceKind.SurfaceView) {
            Log.d(TAG, "surfaceHost SurfaceView attach skipped reason=$reason already-attached")
            return true
        }
        Log.i(TAG, "surfaceHost SurfaceView attachSurface reason=$reason")
        rememberAttachedSurface(newSurface, NativeVideoSurfaceKind.SurfaceView)
        bindings.attachSurface(newSurface, NativeVideoSurfaceKind.SurfaceView)
        return true
    }

    // ── TextureView ─────────────────────────────────────────────────────

    private fun createTextureView(host: ViewGroup): TextureView =
        TextureView(host.context).apply {
            Log.d(TAG, "surfaceHost create TextureView")
            isOpaque = true
            keepScreenOn = this@VesperNativeSurfaceHost.keepScreenOn
            surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                override fun onSurfaceTextureAvailable(
                    surfaceTexture: SurfaceTexture,
                    width: Int,
                    height: Int,
                ) {
                    Log.d(TAG, "surfaceHost TextureView available size=${width}x$height")
                    val newSurface = Surface(surfaceTexture)
                    surface = newSurface
                    rememberAttachedSurface(newSurface, NativeVideoSurfaceKind.TextureView)
                    bindings.attachSurface(newSurface, NativeVideoSurfaceKind.TextureView)
                }

                override fun onSurfaceTextureSizeChanged(
                    surfaceTexture: SurfaceTexture,
                    width: Int,
                    height: Int,
                ) = Unit

                override fun onSurfaceTextureDestroyed(surfaceTexture: SurfaceTexture): Boolean {
                    Log.d(TAG, "surfaceHost TextureView destroyed")
                    try {
                        bindings.detachSurface()
                    } finally {
                        clearAttachedSurface(surface)
                        surface?.release()
                        surface = null
                    }
                    return true
                }

                override fun onSurfaceTextureUpdated(surfaceTexture: SurfaceTexture) = Unit
            }
        }

    // ── Aspect-preserving fit transform ─────────────────────────────────

    private fun applyKeepScreenOn() {
        hostView?.keepScreenOn = keepScreenOn
        renderView?.keepScreenOn = keepScreenOn
    }

    private fun isCurrentSurfaceView(view: SurfaceView): Boolean =
        renderView === view && hostView != null

    private fun rememberAttachedSurface(surface: Surface, kind: NativeVideoSurfaceKind) {
        attachedSurface = surface
        attachedSurfaceKind = kind
    }

    private fun clearAttachedSurface(surface: Surface? = null) {
        if (surface == null || attachedSurface === surface) {
            attachedSurface = null
            attachedSurfaceKind = null
        }
    }

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

        val transform = calculateAspectFitScale(
            containerWidth = viewWidth,
            containerHeight = viewHeight,
            videoWidth = layout.width,
            videoHeight = layout.height,
            pixelWidthHeightRatio = layout.pixelWidthHeightRatio,
        ) ?: run {
            view.setTransform(Matrix())
            return
        }

        val matrix =
            Matrix().apply {
                setScale(transform.scaleX, transform.scaleY, viewWidth / 2f, viewHeight / 2f)
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

        val targetSize =
            calculateAspectFitSize(
                containerWidth = hostWidth,
                containerHeight = hostHeight,
                videoWidth = layout.width,
                videoHeight = layout.height,
                pixelWidthHeightRatio = layout.pixelWidthHeightRatio,
            ) ?: return
        val targetWidth = targetSize.width
        val targetHeight = targetSize.height

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

private const val TAG = "VesperPlayerAndroidHost"

internal data class AspectFitSize(
    val width: Int,
    val height: Int,
)

internal data class AspectFitScale(
    val scaleX: Float,
    val scaleY: Float,
)

internal fun calculateAspectFitSize(
    containerWidth: Int,
    containerHeight: Int,
    videoWidth: Int,
    videoHeight: Int,
    pixelWidthHeightRatio: Float = 1.0f,
): AspectFitSize? {
    if (
        containerWidth <= 0 ||
        containerHeight <= 0 ||
        videoWidth <= 0 ||
        videoHeight <= 0 ||
        pixelWidthHeightRatio <= 0f
    ) {
        return null
    }
    val videoAspectRatio = (videoWidth.toFloat() * pixelWidthHeightRatio) / videoHeight.toFloat()
    if (videoAspectRatio <= 0f) return null
    val containerAspectRatio = containerWidth.toFloat() / containerHeight.toFloat()
    return if (videoAspectRatio > containerAspectRatio) {
        AspectFitSize(
            width = containerWidth,
            height = (containerWidth / videoAspectRatio).toInt().coerceAtLeast(1),
        )
    } else {
        AspectFitSize(
            width = (containerHeight * videoAspectRatio).toInt().coerceAtLeast(1),
            height = containerHeight,
        )
    }
}

internal fun calculateAspectFitScale(
    containerWidth: Float,
    containerHeight: Float,
    videoWidth: Int,
    videoHeight: Int,
    pixelWidthHeightRatio: Float = 1.0f,
): AspectFitScale? {
    if (
        containerWidth <= 0f ||
        containerHeight <= 0f ||
        videoWidth <= 0 ||
        videoHeight <= 0 ||
        pixelWidthHeightRatio <= 0f
    ) {
        return null
    }
    val videoAspectRatio = (videoWidth.toFloat() * pixelWidthHeightRatio) / videoHeight.toFloat()
    if (videoAspectRatio <= 0f) return null
    val containerAspectRatio = containerWidth / containerHeight
    return if (videoAspectRatio > containerAspectRatio) {
        AspectFitScale(
            scaleX = 1.0f,
            scaleY = containerAspectRatio / videoAspectRatio,
        )
    } else {
        AspectFitScale(
            scaleX = videoAspectRatio / containerAspectRatio,
            scaleY = 1.0f,
        )
    }
}
