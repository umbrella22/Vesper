package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperPlayerSurfaceView
import io.github.umbrella22.vesper.player.android.VesperVideoPresentation
import io.github.umbrella22.vesper.player.android.VesperVideoSurfaceGeometry
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel

internal fun VesperVideoPresentation.toFlutterMap(): Map<String, Any> = mapOf(
    "displayWidth" to displayWidth, "displayHeight" to displayHeight,
)

internal fun VesperVideoSurfaceGeometry.toFlutterMap(): Map<String, Any> = mapOf(
    "width" to width, "height" to height,
    "contentRect" to mapOf("left" to contentRect.left, "top" to contentRect.top,
        "width" to contentRect.width, "height" to contentRect.height),
)

/** One channel per platform view; a replaced view cannot update its successor. */
internal class VideoGeometryStream(
    messenger: BinaryMessenger,
    viewId: Int,
    private val host: VesperPlayerSurfaceView,
    private val onChanged: () -> Unit,
) : EventChannel.StreamHandler {
    private val channel = EventChannel(messenger,
        "io.github.umbrella22.vesper_player/views/$viewId/geometry")
    private var sink: EventChannel.EventSink? = null
    private var closed = false

    init {
        channel.setStreamHandler(this)
        host.onGeometryChanged = {
            sink?.success(it?.toFlutterMap())
            onChanged()
        }
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
        sink = events
        if (closed) {
            events.endOfStream()
            return
        }
        events.success(host.geometry.value?.toFlutterMap())
    }

    override fun onCancel(arguments: Any?) {
        sink = null
        if (closed) channel.setStreamHandler(null)
    }

    fun close() {
        if (closed) return
        closed = true
        host.onGeometryChanged = null
        val activeSink = sink
        sink = null
        if (activeSink == null) {
            channel.setStreamHandler(null)
        } else {
            // Flutter can release the native view before cancelling its stream.
            // Keep the handler until that cancellation has been acknowledged.
            activeSink.endOfStream()
        }
    }
}
