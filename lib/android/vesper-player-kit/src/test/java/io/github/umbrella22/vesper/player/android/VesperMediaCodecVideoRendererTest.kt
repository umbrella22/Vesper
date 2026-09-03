package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperMediaCodecVideoRendererTest {
    @Test
    fun api29MtkOmxCodecRequiresSetOutputSurfaceWorkaround() {
        assertTrue(
            vesperCodecNeedsSetOutputSurfaceWorkaround(
                media3RequiresWorkaround = false,
                sdkInt = 29,
                codecName = "OMX.MTK.VIDEO.DECODER.HEVC",
            ),
        )
    }

    @Test
    fun preApi29MtkOmxCodecRequiresSetOutputSurfaceWorkaround() {
        assertTrue(
            vesperCodecNeedsSetOutputSurfaceWorkaround(
                media3RequiresWorkaround = false,
                sdkInt = 26,
                codecName = "OMX.MTK.VIDEO.DECODER.AVC",
            ),
        )
    }

    @Test
    fun api30MtkOmxCodecUsesMedia3Default() {
        assertFalse(
            vesperCodecNeedsSetOutputSurfaceWorkaround(
                media3RequiresWorkaround = false,
                sdkInt = 30,
                codecName = "OMX.MTK.VIDEO.DECODER.HEVC",
            ),
        )
    }

    @Test
    fun nonMtkCodecUsesMedia3Default() {
        assertFalse(
            vesperCodecNeedsSetOutputSurfaceWorkaround(
                media3RequiresWorkaround = false,
                sdkInt = 29,
                codecName = "c2.mtk.hevc.decoder",
            ),
        )
    }

    @Test
    fun media3WorkaroundIsRetained() {
        assertTrue(
            vesperCodecNeedsSetOutputSurfaceWorkaround(
                media3RequiresWorkaround = true,
                sdkInt = 35,
                codecName = "OMX.vendor.video.decoder.avc",
            ),
        )
    }
}
