package io.github.ikaros.vesper.player.android

import android.graphics.Matrix
import android.graphics.Color
import android.graphics.SurfaceTexture
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.TextView
import androidx.media3.common.text.Cue

private const val SUBTITLE_OVERLAY_TAG =
    "io.github.ikaros.vesper.player.subtitle-overlay"

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
    private var subtitleView: TextView? = null
    private var subtitleStyle = VesperSubtitleStyle.Default
    private var subtitleText = ""

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
            postVideoTransform()
            reattachIfAvailable()
            postSurfaceViewAttachCheck("same-host")
            return
        }

        hostView?.removeOnLayoutChangeListener(hostLayoutListener)

        val existingView = renderView
        if (existingView != null) {
            Log.d(TAG, "surfaceHost moving existing $surfaceKind render view to new host")
            (existingView.parent as? ViewGroup)?.removeView(existingView)
            attachRenderAndSubtitleViews(host, existingView)
            hostView = host
            host.addOnLayoutChangeListener(hostLayoutListener)
            applyVideoTransform()
            postVideoTransform()
            reattachIfAvailable()
            postSurfaceViewAttachCheck("move-host")
            return
        }

        val view = when (surfaceKind) {
            NativeVideoSurfaceKind.SurfaceView -> createSurfaceView(host)
            NativeVideoSurfaceKind.TextureView -> createTextureView(host)
        }

        attachRenderAndSubtitleViews(host, view)
        hostView = host
        renderView = view
        applyKeepScreenOn()
        host.addOnLayoutChangeListener(hostLayoutListener)
        applyVideoTransform()
        postVideoTransform()
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
        postVideoTransform()
    }

    fun setKeepScreenOn(active: Boolean) {
        keepScreenOn = active
        applyKeepScreenOn()
    }

    fun updateSubtitleCues(cues: List<Cue>) {
        subtitleText =
            cues.mapNotNull { cue -> cue.text?.toString()?.trim()?.takeIf(String::isNotEmpty) }
                .joinToString("\n")
        applySubtitleState()
    }

    fun updateSubtitleStyle(style: VesperSubtitleStyle) {
        require(style.fontScale.isFinite() && style.fontScale in 0.5f..3.0f) {
            "Subtitle fontScale must be finite and between 0.5 and 3.0."
        }
        subtitleStyle = style
        applySubtitleState()
    }

    fun close() {
        bindings.setOnVideoLayoutInfoListener(null)
        bindings.setOnSubtitleCuesListener(null)
        subtitleText = ""
        subtitleView?.text = ""
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
        subtitleView = null
        renderView = null
        hostView = null
    }

    private fun attachRenderAndSubtitleViews(host: ViewGroup, view: View) {
        (subtitleView?.parent as? ViewGroup)?.removeView(subtitleView)
        host.removeAllViews()
        host.addView(view, matchParentLayoutParams())
        val overlay = subtitleView ?: createSubtitleView(host).also { subtitleView = it }
        host.addView(overlay, subtitleLayoutParams(host))
        applySubtitleState()
    }

    private fun createSubtitleView(host: ViewGroup): TextView =
        TextView(host.context).apply {
            tag = SUBTITLE_OVERLAY_TAG
            setTextColor(Color.WHITE)
            setBackgroundColor(Color.TRANSPARENT)
            gravity = Gravity.CENTER
            setShadowLayer(4f, 0f, 2f, Color.BLACK)
            setPadding(dp(host, 16), dp(host, 8), dp(host, 16), dp(host, 8))
            maxLines = 4
            isClickable = false
            isFocusable = false
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }

    private fun subtitleLayoutParams(host: ViewGroup): FrameLayout.LayoutParams =
        FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL,
        ).apply {
            leftMargin = dp(host, 16)
            rightMargin = dp(host, 16)
            bottomMargin = dp(host, 32)
        }

    private fun applySubtitleState() {
        val view = subtitleView ?: return
        view.text = subtitleText
        view.visibility =
            if (subtitleStyle.visible && subtitleText.isNotEmpty()) View.VISIBLE else View.GONE
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f * subtitleStyle.fontScale)
    }

    private fun dp(host: ViewGroup, value: Int): Int =
        (value * host.resources.displayMetrics.density).toInt()

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

    private fun postVideoTransform() {
        val view = renderView ?: return
        view.post {
            if (renderView === view && hostView != null) {
                applyVideoTransform()
            }
        }
    }

    private fun applyTextureViewTransform() {
        val view = renderView as? TextureView ?: return
        val layout = videoLayoutInfo
        val viewWidth = view.width.toFloat()
        val viewHeight = view.height.toFloat()

        if (layout == null || viewWidth <= 0f || viewHeight <= 0f || layout.width <= 0 || layout.height <= 0) {
            view.isOpaque = false
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
            view.isOpaque = false
            view.setTransform(Matrix())
            return
        }

        // A scaled TextureView does not cover its full host bounds. Let the
        // host's black background fill the letterbox area instead of treating
        // an old SurfaceTexture buffer as an opaque replacement.
        view.isOpaque =
            transform.scaleX >= 0.999f && transform.scaleY >= 0.999f

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
            val layoutParamsChanged =
                lp.width != targetWidth || lp.height != targetHeight || lp.gravity != Gravity.CENTER
            if (layoutParamsChanged) {
                Log.d(
                    TAG,
                    "surfaceHost apply SurfaceView layout host=${hostWidth}x$hostHeight " +
                        "video=${layout.width}x${layout.height} target=${targetWidth}x$targetHeight " +
                        "current=${lp.width}x${lp.height}",
                )
                lp.width = targetWidth
                lp.height = targetHeight
                lp.gravity = Gravity.CENTER
                view.layoutParams = lp
            }
            if (
                !layoutParamsChanged &&
                (view.width != targetWidth || view.height != targetHeight)
            ) {
                val viewNeedsLayoutRequest = !view.isLayoutRequested
                val hostNeedsLayoutRequest = !host.isLayoutRequested
                if (viewNeedsLayoutRequest || hostNeedsLayoutRequest) {
                    Log.d(
                        TAG,
                        "surfaceHost request stale SurfaceView layout target=${targetWidth}x$targetHeight " +
                            "actual=${view.width}x${view.height} host=${hostWidth}x$hostHeight",
                    )
                    if (viewNeedsLayoutRequest) view.requestLayout()
                    if (hostNeedsLayoutRequest) host.requestLayout()
                }
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
