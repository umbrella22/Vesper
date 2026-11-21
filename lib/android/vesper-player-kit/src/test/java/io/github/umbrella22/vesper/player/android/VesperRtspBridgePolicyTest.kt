package io.github.umbrella22.vesper.player.android

import androidx.media3.common.C
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperRtspBridgePolicyTest {
    @Test
    fun privateHlsWindowPublishesLiveTimeWithoutSeekability() {
        val sample = ExoTimelineSample(
            timelinePositionMs = 8_500L,
            durationMs = 6_000L,
            isLive = true,
            isSeekable = true,
            seekableStartMs = 6_000L,
            seekableEndMs = 12_000L,
            liveEdgeMs = 12_000L,
        ).asLiveOnly()
        assertTrue(sample.isLive)
        assertFalse(sample.isSeekable)
        assertEquals(C.TIME_UNSET, sample.durationMs)
        assertEquals(C.TIME_UNSET, sample.seekableStartMs)
        assertEquals(C.TIME_UNSET, sample.seekableEndMs)
        assertEquals(8_500L, sample.timelinePositionMs)
        assertEquals(12_000L, sample.liveEdgeMs)
    }

    @Test
    fun originTimeoutAndMissingFeatureHaveDifferentErrorContracts() {
        val timeout = rtspBridgeFailure("rtsp_origin_timeout", "timeout") as VesperPlayerCommandException
        assertEquals(VesperPlayerErrorCode.Timeout, timeout.errorState.code)
        assertEquals(VesperPlayerErrorCategory.Network, timeout.errorState.category)
        assertTrue(timeout.errorState.retriable)

        val missing = rtspBridgeFailure("rtsp_bridge_not_bundled", "missing") as VesperPlayerUnsupportedOperation
        assertEquals("rtsp_bridge_not_bundled", missing.details["reason"])
        assertFalse(missing.toPlayerErrorState().retriable)
    }
}
