package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class VesperPlayerAndroidInputMappingTest {
    @Test
    fun autoSurfaceKindUsesSurfaceView() {
        assertEquals(VesperVideoSurfaceKind.SurfaceView, null.toVesperVideoSurfaceKind())
        assertEquals(VesperVideoSurfaceKind.SurfaceView, "auto".toVesperVideoSurfaceKind())
    }

    @Test
    fun explicitSurfaceKindsArePreserved() {
        assertEquals(VesperVideoSurfaceKind.TextureView, "textureView".toVesperVideoSurfaceKind())
        assertEquals(VesperVideoSurfaceKind.SurfaceView, "surfaceView".toVesperVideoSurfaceKind())
    }

    @Test
    fun unknownSurfaceKindFails() {
        try {
            "unknown".toVesperVideoSurfaceKind()
            fail("Expected unknown renderSurfaceKind to throw.")
        } catch (_: IllegalArgumentException) {
        }
    }
}
