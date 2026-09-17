package io.github.umbrella22.vesper.player.flutter.android

import android.content.ContextWrapper
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.StandardMethodCodec
import io.github.umbrella22.vesper.player.android.VesperPlayerSurfaceView
import java.nio.ByteBuffer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlayerAndroidVideoGeometryStreamTest {
    @Test fun releasingNativeViewStillAcknowledgesTheLaterDartCancellation() {
        val messenger = GeometryMessenger()
        val stream = VideoGeometryStream(messenger, 7, VesperPlayerSurfaceView(ContextWrapper(null))) {}
        messenger.call("listen")
        stream.close()
        assertEquals(1, messenger.endEvents)
        assertTrue(messenger.handlers.containsKey(channel))
        messenger.call("cancel")
        assertFalse(messenger.handlers.containsKey(channel))
        stream.close()
        assertEquals(1, messenger.endEvents)
    }

    @Test fun cancellationBeforeViewReleaseAlsoUnregistersTheChannel() {
        val messenger = GeometryMessenger()
        val stream = VideoGeometryStream(messenger, 7, VesperPlayerSurfaceView(ContextWrapper(null))) {}
        messenger.call("listen")
        messenger.call("cancel")
        stream.close()
        assertFalse(messenger.handlers.containsKey(channel))
        assertEquals(0, messenger.endEvents)
    }

    private class GeometryMessenger : BinaryMessenger {
        val handlers = mutableMapOf<String, BinaryMessenger.BinaryMessageHandler>()
        var endEvents = 0

        override fun setMessageHandler(channel: String, handler: BinaryMessenger.BinaryMessageHandler?) {
            if (handler == null) handlers.remove(channel) else handlers[channel] = handler
        }

        override fun send(channel: String, message: ByteBuffer?) {
            if (message == null) endEvents++
        }

        override fun send(channel: String, message: ByteBuffer?, callback: BinaryMessenger.BinaryReply?) {
            send(channel, message)
            callback?.reply(null)
        }

        fun call(method: String) {
            val message = StandardMethodCodec.INSTANCE.encodeMethodCall(MethodCall(method, null))
            message.flip()
            var replied = false
            requireNotNull(handlers[channel]).onMessage(message) { response ->
                assertTrue(response != null)
                replied = true
            }
            assertTrue(replied)
        }
    }

    companion object {
        private const val channel = "io.github.umbrella22.vesper_player/views/7/geometry"
    }
}
