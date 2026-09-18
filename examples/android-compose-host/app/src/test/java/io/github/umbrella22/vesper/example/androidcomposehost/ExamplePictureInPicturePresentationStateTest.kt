package io.github.umbrella22.vesper.example.androidcomposehost

import io.github.umbrella22.vesper.player.android.VesperAbrPolicy
import io.github.umbrella22.vesper.player.android.VesperMediaTrack
import io.github.umbrella22.vesper.player.android.VesperMediaTrackKind
import io.github.umbrella22.vesper.player.android.VesperTrackCatalog
import io.github.umbrella22.vesper.player.android.VesperTrackSelection
import io.github.umbrella22.vesper.player.android.VesperTrackSelectionSnapshot
import io.github.umbrella22.vesper.player.android.VesperVideoVariantObservation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ExamplePictureInPicturePresentationStateTest {
    private val landscapeTrack =
        VesperMediaTrack(
            id = "landscape",
            kind = VesperMediaTrackKind.Video,
            width = 1920,
            height = 1080,
        )
    private val portraitTrack =
        VesperMediaTrack(
            id = "portrait",
            kind = VesperMediaTrackKind.Video,
            width = 1080,
            height = 1920,
        )
    private val mixedTrackCatalog =
        VesperTrackCatalog(tracks = listOf(landscapeTrack, portraitTrack))

    @Test
    fun `portrait video aspect ratio is preserved for picture in picture`() {
        assertEquals(9.0 / 16.0, exampleVideoAspectRatio(360, 640)!!, 0.000001)
        assertEquals(
            9.0 / 16.0,
            resolveExamplePictureInPictureAspectRatio(9.0 / 16.0),
            0.000001,
        )

        assertEquals(5625 to 10000, examplePictureInPictureRatioFraction(9.0 / 16.0))
    }

    @Test
    fun `invalid picture in picture aspect ratio falls back to landscape`() {
        for (invalidRatio in listOf(null, Double.NaN, Double.NEGATIVE_INFINITY, 0.0, -1.0)) {
            assertEquals(
                EXAMPLE_DEFAULT_PICTURE_IN_PICTURE_ASPECT_RATIO,
                resolveExamplePictureInPictureAspectRatio(invalidRatio),
                0.000001,
            )
        }
        assertEquals(100 to 239, examplePictureInPictureRatioFraction(0.01))
        assertEquals(239 to 100, examplePictureInPictureRatioFraction(10.0))
        for (ratio in listOf(100.0 / 239.0 + 0.00000001, 0.41842, 2.38999)) {
            val (numerator, denominator) = examplePictureInPictureRatioFraction(ratio)
            assertTrue(numerator.toDouble() / denominator >= 100.0 / 239.0)
            assertTrue(numerator.toDouble() / denominator <= 239.0 / 100.0)
        }
    }

    @Test
    fun `picture in picture ratio follows runtime evidence before track metadata`() {
        assertEquals(
            4.0 / 3.0,
            selectExamplePictureInPictureAspectRatio(
                displayAspectRatio = 4.0 / 3.0,
                videoVariantObservation = VesperVideoVariantObservation(width = 1080, height = 1920),
                trackCatalog = mixedTrackCatalog,
                effectiveVideoTrackId = "portrait",
                trackSelection = VesperTrackSelectionSnapshot(),
            )!!,
            0.000001,
        )
        assertEquals(
            9.0 / 16.0,
            selectExamplePictureInPictureAspectRatio(
                displayAspectRatio = Double.NaN,
                videoVariantObservation = VesperVideoVariantObservation(width = 1080, height = 1920),
                trackCatalog = mixedTrackCatalog,
                effectiveVideoTrackId = "landscape",
                trackSelection = VesperTrackSelectionSnapshot(),
            )!!,
            0.000001,
        )
    }

    @Test
    fun `picture in picture track fallback prefers effective then current then fixed selection`() {
        assertEquals(
            9.0 / 16.0,
            selectExamplePictureInPictureAspectRatio(
                displayAspectRatio = null,
                videoVariantObservation = null,
                trackCatalog = mixedTrackCatalog,
                effectiveVideoTrackId = "portrait",
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("landscape"),
                    ),
            )!!,
            0.000001,
        )
        assertEquals(
            9.0 / 16.0,
            selectExamplePictureInPictureAspectRatio(
                displayAspectRatio = null,
                videoVariantObservation = null,
                trackCatalog = mixedTrackCatalog,
                effectiveVideoTrackId = null,
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        video = VesperTrackSelection.auto().copy(trackId = "portrait"),
                        abrPolicy = VesperAbrPolicy.fixedTrack("landscape"),
                    ),
            )!!,
            0.000001,
        )
        assertEquals(
            9.0 / 16.0,
            selectExamplePictureInPictureAspectRatio(
                displayAspectRatio = null,
                videoVariantObservation = null,
                trackCatalog = mixedTrackCatalog,
                effectiveVideoTrackId = null,
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("portrait"),
                    ),
            )!!,
            0.000001,
        )
    }

    @Test
    fun `manual request enters presentation until mode callback exits`() {
        val requested =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureRequestStarted()

        assertTrue(requested.presentation)
        assertFalse(requested.active)

        val active = requested.onPictureInPictureModeChanged(true)
        assertTrue(active.presentation)
        assertTrue(active.active)

        val inactive = active.onPictureInPictureModeChanged(false)
        assertFalse(inactive.presentation)
        assertFalse(inactive.active)
    }

    @Test
    fun `user leave hint only enters presentation when pip is enabled`() {
        val disabled =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureUserLeaveHint(enabled = false)

        assertFalse(disabled.presentation)

        val enabled =
            disabled.onPictureInPictureUserLeaveHint(enabled = true)

        assertTrue(enabled.presentation)
        assertTrue(enabled.pendingAutoEnter)
    }

    @Test
    fun `auto enter timeout restores presentation when pip never starts`() {
        val pending =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureUserLeaveHint(enabled = true)

        val timedOut = pending.onPictureInPictureAutoEnterTimeout()

        assertFalse(timedOut.presentation)
        assertFalse(timedOut.pendingAutoEnter)
    }

    @Test
    fun `auto enter timeout keeps active pip presentation`() {
        val active =
            ExamplePictureInPicturePresentationState()
                .onPictureInPictureUserLeaveHint(enabled = true)
                .onPictureInPictureModeChanged(true)

        val timedOut = active.onPictureInPictureAutoEnterTimeout()

        assertTrue(timedOut.presentation)
        assertTrue(timedOut.active)
    }
}
