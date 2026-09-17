package io.github.umbrella22.vesper.player.flutter.android

import java.lang.reflect.InvocationTargetException
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Test

class VesperPlayerAndroidPictureInPictureTest {
    @Test fun systemEntryBelongsToTheLastConfiguredPlayer() {
        val plugin = VesperPlayerAndroidPlugin()
        val first = session("first")
        val second = session("second")
        try {
            sessions(plugin).putAll(mapOf(first.id to first, second.id to second))
            invoke(plugin, "configurePictureInPictureOwner", first)
            invoke(plugin, "configurePictureInPictureOwner", second)
            // A stale entering state must not steal the system callback from B.
            first.pictureInPictureState = "entering"
            invokeMode(plugin, true)
            assertFalse(first.pictureInPictureActive)
            assertTrue(second.pictureInPictureActive)
            assertEquals("active", second.pictureInPictureState)
            invoke(plugin, "configurePictureInPictureOwner", first)
            invokeMode(plugin, false)
            assertFalse(second.pictureInPictureActive)
            assertEquals("inactive", second.pictureInPictureState)
        } finally {
            first.controller.dispose()
            second.controller.dispose()
        }
    }

    @Test fun rejectedRequestPreservesTheConfiguredOwner() {
        val plugin = VesperPlayerAndroidPlugin()
        val first = session("first")
        val second = session("second")
        try {
            sessions(plugin).putAll(mapOf(first.id to first, second.id to second))
            invoke(plugin, "configurePictureInPictureOwner", first)
            try {
                invoke(plugin, "requestPictureInPicture", second)
                throw AssertionError("A detached Activity must reject PiP")
            } catch (error: InvocationTargetException) {
                assertTrue(error.cause is PictureInPictureRequestException)
            }
            invokeMode(plugin, true)
            assertTrue(first.pictureInPictureActive)
            assertFalse(second.pictureInPictureActive)
        } finally {
            first.controller.dispose()
            second.controller.dispose()
        }
    }

    private fun session(id: String) = PlayerSession(id, VesperPlayerControllerFactory.createPreview(),
        pictureInPictureConfiguration = FlutterPictureInPictureConfiguration(autoEnter = true))

    @Suppress("UNCHECKED_CAST")
    private fun sessions(plugin: VesperPlayerAndroidPlugin): MutableMap<String, PlayerSession> =
        VesperPlayerAndroidPlugin::class.java.getDeclaredField("sessions").apply { isAccessible = true }
            .get(plugin) as MutableMap<String, PlayerSession>

    private fun invoke(plugin: VesperPlayerAndroidPlugin, name: String, session: PlayerSession) {
        VesperPlayerAndroidPlugin::class.java.getDeclaredMethod(name, PlayerSession::class.java)
            .apply { isAccessible = true }.invoke(plugin, session)
    }

    private fun invokeMode(plugin: VesperPlayerAndroidPlugin, active: Boolean) {
        VesperPlayerAndroidPlugin::class.java.getDeclaredMethod("handlePictureInPictureModeChanged", Boolean::class.javaPrimitiveType)
            .apply { isAccessible = true }.invoke(plugin, active)
    }

    @Test fun extremePortraitRatiosRemainInsideThePlatformRangeAfterRounding() {
        assertEquals(100 to 239, pictureInPictureRatioFraction(9.0 / 24))
        assertEquals(239 to 100, pictureInPictureRatioFraction(3.0))
        for (ratio in listOf(100.0 / 239.0 + 0.00000001, 0.41842, 2.38999)) {
            val (numerator, denominator) = pictureInPictureRatioFraction(ratio)
            assertTrue(numerator.toDouble() / denominator >= 100.0 / 239.0)
            assertTrue(numerator.toDouble() / denominator <= 239.0 / 100.0)
        }
    }

    @Test fun portraitVideoOverridesLandscapeViewportAndHost() {
        assertEquals(9.0 / 16, resolvePictureInPictureAspectRatio(null, 9.0 / 16, 16.0 / 9, 2.0), 0.000001)
        assertEquals(1.0, resolvePictureInPictureAspectRatio(1.0, 9.0 / 16, 16.0 / 9, 2.0), 0.000001)
    }

    @Test fun unknownOrInvalidDimensionsFallBackInOrder() {
        assertEquals(0.75, resolvePictureInPictureAspectRatio(null, null, 0.75, 2.0), 0.000001)
        assertEquals(2.0, resolvePictureInPictureAspectRatio(null, Double.NaN, Double.POSITIVE_INFINITY, 2.0), 0.000001)
        assertEquals(16.0 / 9, resolvePictureInPictureAspectRatio(null, null, 0.0, -1.0), 0.000001)
    }

    @Test(expected = IllegalArgumentException::class)
    fun invalidExplicitRatioIsRejected() {
        FlutterPictureInPictureConfiguration(preferredAspectRatio = Double.NaN)
    }
}
