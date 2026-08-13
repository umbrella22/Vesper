package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VesperDroppedVideoFramesBenchmarkTest {
    @Test
    fun currentEnabledCallbackProducesBenchmarkAttributes() {
        assertEquals(
            mapOf("count" to "7", "elapsedMs" to "250"),
            media3DroppedVideoFramesBenchmarkAttributes(
                callbackIsCurrent = true,
                benchmarkIsEnabled = true,
                droppedFrames = 7,
                elapsedMs = 250L,
            ),
        )
    }

    @Test
    fun disabledOrStaleCallbackProducesNoAttributes() {
        assertNull(
            media3DroppedVideoFramesBenchmarkAttributes(
                callbackIsCurrent = true,
                benchmarkIsEnabled = false,
                droppedFrames = 1,
                elapsedMs = 10L,
            ),
        )
        assertNull(
            media3DroppedVideoFramesBenchmarkAttributes(
                callbackIsCurrent = false,
                benchmarkIsEnabled = true,
                droppedFrames = 1,
                elapsedMs = 10L,
            ),
        )
    }

    @Test
    fun nonPositiveFrameCountProducesNoAttributes() {
        assertNull(
            media3DroppedVideoFramesBenchmarkAttributes(
                callbackIsCurrent = true,
                benchmarkIsEnabled = true,
                droppedFrames = 0,
                elapsedMs = 10L,
            ),
        )
        assertNull(
            media3DroppedVideoFramesBenchmarkAttributes(
                callbackIsCurrent = true,
                benchmarkIsEnabled = true,
                droppedFrames = -1,
                elapsedMs = 10L,
            ),
        )
    }

    @Test
    fun negativeReportingWindowIsClampedToZero() {
        assertEquals(
            mapOf("count" to "1", "elapsedMs" to "0"),
            media3DroppedVideoFramesBenchmarkAttributes(
                callbackIsCurrent = true,
                benchmarkIsEnabled = true,
                droppedFrames = 1,
                elapsedMs = -1L,
            ),
        )
    }
}
