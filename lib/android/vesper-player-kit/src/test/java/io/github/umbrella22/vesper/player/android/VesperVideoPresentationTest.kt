package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VesperVideoPresentationTest {
    @Test
    fun renderedPortraitSizeAndNonSquarePixelsDefineDisplayAspect() {
        val portrait = NativeVideoLayoutInfo(1080, 1920).toVideoPresentation()!!
        assertEquals(9.0 / 16.0, portrait.displayAspectRatio, 0.000001)

        val anamorphic = NativeVideoLayoutInfo(720, 576, 16f / 15f).toVideoPresentation()!!
        assertEquals(4.0 / 3.0, anamorphic.displayAspectRatio, 0.000001)
    }

    @Test
    fun unavailableDimensionsNeverProduceDisplayGeometry() {
        for (layout in listOf(
            NativeVideoLayoutInfo(0, 1920),
            NativeVideoLayoutInfo(1080, 0),
            NativeVideoLayoutInfo(1080, 1920, Float.NaN),
            NativeVideoLayoutInfo(1080, 1920, Float.POSITIVE_INFINITY),
            NativeVideoLayoutInfo(1080, 1920, 0f),
        )) {
            assertNull(layout.toVideoPresentation())
        }
    }
}
