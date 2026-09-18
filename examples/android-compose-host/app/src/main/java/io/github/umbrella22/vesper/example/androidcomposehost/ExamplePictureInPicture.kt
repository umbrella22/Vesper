package io.github.umbrella22.vesper.example.androidcomposehost

import android.app.PictureInPictureParams
import android.os.Build
import android.util.Rational
import io.github.umbrella22.vesper.player.android.VesperAbrMode
import io.github.umbrella22.vesper.player.android.VesperTrackCatalog
import io.github.umbrella22.vesper.player.android.VesperTrackSelectionSnapshot
import io.github.umbrella22.vesper.player.android.VesperVideoVariantObservation
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.roundToInt

internal const val EXAMPLE_DEFAULT_PICTURE_IN_PICTURE_ASPECT_RATIO = 16.0 / 9.0

private fun validExampleVideoAspectRatio(aspectRatio: Double?): Double? =
    aspectRatio?.takeIf { it.isFinite() && it > 0.0 }

internal fun exampleVideoAspectRatio(width: Int?, height: Int?): Double? {
    if (width == null || height == null || width <= 0 || height <= 0) {
        return null
    }
    return width.toDouble() / height.toDouble()
}

internal fun selectExamplePictureInPictureAspectRatio(
    displayAspectRatio: Double?,
    videoVariantObservation: VesperVideoVariantObservation?,
    trackCatalog: VesperTrackCatalog,
    effectiveVideoTrackId: String?,
    trackSelection: VesperTrackSelectionSnapshot,
): Double? {
    validExampleVideoAspectRatio(displayAspectRatio)?.let { return it }
    videoVariantObservation
        ?.let { observation -> exampleVideoAspectRatio(observation.width, observation.height) }
        ?.let { return it }

    val videoTracks = trackCatalog.videoTracks
    val selectedVideoTrackId =
        trackSelection.abrPolicy.trackId
            .takeIf { trackSelection.abrPolicy.mode == VesperAbrMode.FixedTrack }
    for (
        trackId in
            listOfNotNull(
                effectiveVideoTrackId,
                trackSelection.video.trackId,
                selectedVideoTrackId,
            ).distinct()
    ) {
        videoTracks
            .firstOrNull { track -> track.id == trackId }
            ?.let { track -> exampleVideoAspectRatio(track.width, track.height) }
            ?.let { return it }
    }
    return videoTracks.firstNotNullOfOrNull { track ->
        exampleVideoAspectRatio(track.width, track.height)
    }
}

internal fun resolveExamplePictureInPictureAspectRatio(aspectRatio: Double?): Double =
    validExampleVideoAspectRatio(aspectRatio)
        ?: EXAMPLE_DEFAULT_PICTURE_IN_PICTURE_ASPECT_RATIO

internal fun examplePictureInPictureRatioFraction(aspectRatio: Double): Pair<Int, Int> {
    val minimum = 100.0 / 239.0
    val maximum = 239.0 / 100.0
    val clamped = aspectRatio.coerceIn(minimum, maximum)
    if (clamped == minimum) {
        return 100 to 239
    }
    if (clamped == maximum) {
        return 239 to 100
    }

    val denominator = 10_000
    val numerator =
        (clamped * denominator)
            .roundToInt()
            .coerceIn(
                ceil(minimum * denominator).toInt(),
                floor(maximum * denominator).toInt(),
            )
    return numerator to denominator
}

internal fun buildExamplePictureInPictureParams(
    autoEnter: Boolean,
    videoAspectRatio: Double?,
): PictureInPictureParams {
    check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        "Picture in Picture params require Android O or newer."
    }
    val (numerator, denominator) =
        examplePictureInPictureRatioFraction(
            resolveExamplePictureInPictureAspectRatio(videoAspectRatio),
        )
    val builder =
        PictureInPictureParams.Builder()
            .setAspectRatio(Rational(numerator, denominator))
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        builder.setAutoEnterEnabled(autoEnter)
    }
    return builder.build()
}
