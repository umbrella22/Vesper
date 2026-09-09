package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperVideoFrameMetadataAccumulatorTest {
    @Test
    fun regularThirtyFpsFramesProduceOneBoundedWindow() {
        val accumulator =
            VesperVideoFrameMetadataAccumulator(
                maximumFramesPerWindow = 4,
                maximumWindowDurationNs = Long.MAX_VALUE,
            )

        assertNull(accumulator.record(0L, 1_000_000_000L, 30f))
        assertNull(accumulator.record(33_333L, 1_033_333_333L, 30f))
        assertNull(accumulator.record(66_666L, 1_066_666_666L, 30f))
        val window = requireNotNull(accumulator.record(99_999L, 1_099_999_999L, 30f))

        assertEquals("4", window["frameCount"])
        assertEquals("0", window["nonMonotonicPresentationCount"])
        assertEquals("0", window["releaseTimestampRegressionCount"])
        assertEquals("0", window["estimatedMissingFrameCount"])
        assertTrue(requireNotNull(window["presentationRateMilli"]).toLong() in 29_990L..30_010L)
        assertTrue(requireNotNull(window["scheduledRateMilli"]).toLong() in 29_990L..30_010L)
    }

    @Test
    fun presentationGapEstimatesMissingFramesWithoutLosingMonotonicity() {
        val accumulator =
            VesperVideoFrameMetadataAccumulator(
                maximumFramesPerWindow = 3,
                maximumWindowDurationNs = Long.MAX_VALUE,
            )

        accumulator.record(0L, 1_000_000_000L, 30f)
        accumulator.record(33_333L, 1_033_333_333L, 30f)
        val window = requireNotNull(accumulator.record(99_999L, 1_099_999_999L, 30f))

        assertEquals("1", window["largePresentationGapCount"])
        assertEquals("1", window["estimatedMissingFrameCount"])
        assertEquals("0", window["nonMonotonicPresentationCount"])
        assertEquals("66666", window["maximumPresentationGapUs"])
    }

    @Test
    fun missingDeclaredFrameRateUsesTheObservedPtsInterval() {
        val accumulator =
            VesperVideoFrameMetadataAccumulator(
                maximumFramesPerWindow = 4,
                maximumWindowDurationNs = Long.MAX_VALUE,
            )

        accumulator.record(0L, 1_000_000_000L, -1f)
        accumulator.record(33_333L, 1_033_333_333L, -1f)
        accumulator.record(66_666L, 1_066_666_666L, -1f)
        val window = requireNotNull(accumulator.record(133_332L, 1_133_332_666L, -1f))

        assertEquals("30000", window["expectedFrameRateMilli"])
        assertEquals("1", window["largePresentationGapCount"])
        assertEquals("1", window["estimatedMissingFrameCount"])
    }

    @Test
    fun timestampRegressionsAreCountedWhenTheWindowCloses() {
        val accumulator =
            VesperVideoFrameMetadataAccumulator(
                maximumFramesPerWindow = 2,
                maximumWindowDurationNs = 10L,
            )

        assertNull(accumulator.record(100L, 100L, 30f))
        val window = requireNotNull(accumulator.record(90L, 90L, 30f))

        assertEquals("1", window["nonMonotonicPresentationCount"])
        assertEquals("1", window["releaseTimestampRegressionCount"])
        assertEquals("0", window["presentationSpanUs"])
        assertEquals("0", window["releaseSpanNs"])
    }
}
