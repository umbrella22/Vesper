package io.github.umbrella22.vesper.player.flutter.android

import io.flutter.plugin.common.EventChannel
import io.github.umbrella22.vesper.player.android.VesperFirstFrameObservation
import io.github.umbrella22.vesper.player.android.VesperFirstFrameObservationKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperFirstFrameEventDeliveryTest {
    @Test
    fun absentSinkRetainsDeliveryAndReconnectionDoesNotDuplicateDeliveredEpochs() {
        val delivery = VesperFirstFrameEventDelivery()
        val first = VesperFirstFrameObservation(1, 120, VesperFirstFrameObservationKind.Media3RenderedFirstFrame, 90_000)
        val events = mutableListOf<Map<*, *>>()
        val sink = object : EventChannel.EventSink {
            override fun success(value: Any?) { events.add(value as Map<*, *>) }
            override fun error(code: String, message: String?, details: Any?) = Unit
            override fun endOfStream() = Unit
        }
        delivery.deliver("p", first, null)
        assertTrue(events.isEmpty())
        delivery.deliver("p", first, sink)
        delivery.deliver("p", first, null)
        delivery.deliver("p", first, sink)
        assertEquals(1, events.size)
        assertEquals(90_000L, (events.single()["observation"] as Map<*, *>)["mediaPositionMs"])
        delivery.deliver("p", first.copy(playbackEpoch = 2), sink)
        assertEquals(2, events.size)
    }
}
