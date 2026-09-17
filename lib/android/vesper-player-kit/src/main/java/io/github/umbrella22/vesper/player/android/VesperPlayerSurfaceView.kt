package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.util.AttributeSet
import android.widget.FrameLayout
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Native playback host. Geometry belongs to this view and uses local dp. */
class VesperPlayerSurfaceView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : FrameLayout(context, attrs) {
    private val mutableGeometry = MutableStateFlow<VesperVideoSurfaceGeometry?>(null)
    val geometry = mutableGeometry.asStateFlow()
    var onGeometryChanged: ((VesperVideoSurfaceGeometry?) -> Unit)? = null
        set(value) {
            field = value
            value?.invoke(geometry.value)
        }

    internal fun updateGeometry(value: VesperVideoSurfaceGeometry?) {
        if (value == geometry.value) return
        mutableGeometry.value = value
        onGeometryChanged?.invoke(value)
    }
}
