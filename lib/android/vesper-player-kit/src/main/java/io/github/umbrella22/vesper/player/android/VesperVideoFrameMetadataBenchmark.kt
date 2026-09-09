package io.github.umbrella22.vesper.player.android

import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.video.VideoFrameMetadataListener
import kotlin.math.roundToLong

private const val DEFAULT_FRAME_WINDOW_SIZE = 300
private const val DEFAULT_FRAME_WINDOW_DURATION_NS = 10_000_000_000L
private const val MICROS_PER_SECOND = 1_000_000L
private const val NANOS_PER_SECOND = 1_000_000_000L
private const val MAXIMUM_INFERRED_FRAME_INTERVAL_US = 250_000L

/**
 * Aggregates render-boundary frame metadata into bounded windows. Media3 calls
 * this path immediately before rendering, so the result describes scheduled
 * video frames rather than compositor presentation.
 */
internal class VesperVideoFrameMetadataAccumulator(
    private val maximumFramesPerWindow: Int = DEFAULT_FRAME_WINDOW_SIZE,
    private val maximumWindowDurationNs: Long = DEFAULT_FRAME_WINDOW_DURATION_NS,
) {
    private var windowIndex = 0L
    private var frameCount = 0
    private var firstPresentationTimeUs = 0L
    private var lastPresentationTimeUs = 0L
    private var previousPresentationTimeUs: Long? = null
    private var firstReleaseTimeNs = 0L
    private var lastReleaseTimeNs = 0L
    private var previousReleaseTimeNs: Long? = null
    private var expectedFrameIntervalUs: Long? = null
    private var nonMonotonicPresentationCount = 0L
    private var releaseTimestampRegressionCount = 0L
    private var largePresentationGapCount = 0L
    private var estimatedMissingFrameCount = 0L
    private var maximumPresentationGapUs = 0L

    init {
        require(maximumFramesPerWindow > 1) {
            "maximumFramesPerWindow must be greater than one"
        }
        require(maximumWindowDurationNs > 0L) {
            "maximumWindowDurationNs must be positive"
        }
    }

    fun record(
        presentationTimeUs: Long,
        releaseTimeNs: Long,
        frameRate: Float,
    ): Map<String, String>? {
        if (frameCount == 0) {
            firstPresentationTimeUs = presentationTimeUs
            firstReleaseTimeNs = releaseTimeNs
        }
        frameCount += 1
        lastPresentationTimeUs = presentationTimeUs
        lastReleaseTimeNs = releaseTimeNs

        val declaredFrameIntervalUs =
            frameRate
            .takeIf { it.isFinite() && it > 0f }
            ?.let { (MICROS_PER_SECOND / it).roundToLong().coerceAtLeast(1L) }
        declaredFrameIntervalUs?.let { expectedFrameIntervalUs = it }

        previousPresentationTimeUs?.let { previous ->
            val gapUs = presentationTimeUs - previous
            if (gapUs <= 0L) {
                nonMonotonicPresentationCount += 1L
            } else {
                maximumPresentationGapUs = maxOf(maximumPresentationGapUs, gapUs)
                if (
                    expectedFrameIntervalUs == null &&
                    gapUs <= MAXIMUM_INFERRED_FRAME_INTERVAL_US
                ) {
                    expectedFrameIntervalUs = gapUs
                }
                expectedFrameIntervalUs?.let { expectedUs ->
                    if (gapUs > expectedUs + expectedUs / 2L) {
                        largePresentationGapCount += 1L
                        val representedFrames =
                            ((gapUs + expectedUs / 2L) / expectedUs).coerceAtLeast(1L)
                        estimatedMissingFrameCount += representedFrames - 1L
                    }
                }
            }
        }
        previousPresentationTimeUs = presentationTimeUs

        previousReleaseTimeNs?.let { previous ->
            if (releaseTimeNs <= previous) {
                releaseTimestampRegressionCount += 1L
            }
        }
        previousReleaseTimeNs = releaseTimeNs

        val releaseSpanNs = (lastReleaseTimeNs - firstReleaseTimeNs).coerceAtLeast(0L)
        return if (
            frameCount >= maximumFramesPerWindow ||
                releaseSpanNs >= maximumWindowDurationNs
        ) {
            snapshotAndReset(releaseSpanNs)
        } else {
            null
        }
    }

    private fun snapshotAndReset(releaseSpanNs: Long): Map<String, String> {
        val presentationSpanUs =
            (lastPresentationTimeUs - firstPresentationTimeUs).coerceAtLeast(0L)
        val intervalUs = expectedFrameIntervalUs
        val attributes =
            mapOf(
                "windowIndex" to windowIndex.toString(),
                "frameCount" to frameCount.toString(),
                "firstPresentationTimeUs" to firstPresentationTimeUs.toString(),
                "lastPresentationTimeUs" to lastPresentationTimeUs.toString(),
                "presentationSpanUs" to presentationSpanUs.toString(),
                "releaseSpanNs" to releaseSpanNs.toString(),
                "expectedFrameRateMilli" to
                    (intervalUs?.let { MICROS_PER_SECOND * 1_000L / it } ?: 0L).toString(),
                "presentationRateMilli" to
                    rateMilli(frameCount, presentationSpanUs, MICROS_PER_SECOND).toString(),
                "scheduledRateMilli" to
                    rateMilli(frameCount, releaseSpanNs, NANOS_PER_SECOND).toString(),
                "nonMonotonicPresentationCount" to nonMonotonicPresentationCount.toString(),
                "releaseTimestampRegressionCount" to releaseTimestampRegressionCount.toString(),
                "largePresentationGapCount" to largePresentationGapCount.toString(),
                "estimatedMissingFrameCount" to estimatedMissingFrameCount.toString(),
                "maximumPresentationGapUs" to maximumPresentationGapUs.toString(),
            )
        windowIndex += 1L
        frameCount = 0
        firstPresentationTimeUs = 0L
        lastPresentationTimeUs = 0L
        firstReleaseTimeNs = 0L
        lastReleaseTimeNs = 0L
        nonMonotonicPresentationCount = 0L
        releaseTimestampRegressionCount = 0L
        largePresentationGapCount = 0L
        estimatedMissingFrameCount = 0L
        maximumPresentationGapUs = 0L
        return attributes
    }

    private fun rateMilli(
        count: Int,
        span: Long,
        unitsPerSecond: Long,
    ): Long {
        if (count <= 1 || span <= 0L) return 0L
        val numerator = (count - 1L).coerceAtMost(Long.MAX_VALUE / unitsPerSecond / 1_000L)
        return numerator * unitsPerSecond * 1_000L / span
    }
}

@OptIn(UnstableApi::class)
internal fun VesperNativeJniBindings.buildVideoFrameMetadataBenchmarkListener(
    callbackGeneration: Long,
): VideoFrameMetadataListener {
    val accumulator = VesperVideoFrameMetadataAccumulator()
    return VideoFrameMetadataListener { presentationTimeUs, releaseTimeNs, format, _ ->
        if (!isCurrentSystemPlaybackCallback(callbackGeneration) || !benchmarkRecorder.isEnabled) {
            return@VideoFrameMetadataListener
        }
        accumulator.record(presentationTimeUs, releaseTimeNs, format.frameRate)?.let { attributes ->
            recordBenchmark("video_frame_metadata_window", attributes)
        }
    }
}
