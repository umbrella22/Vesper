package io.github.umbrella22.vesper.player.android

import androidx.media3.common.Format

fun resolveVideoVariantObservation(
    currentVideoFormat: Format?,
): VesperVideoVariantObservation? {
    val format = currentVideoFormat ?: return null
    val width = format.width.takeIf { it != Format.NO_VALUE && it > 0 }
    val height = format.height.takeIf { it != Format.NO_VALUE && it > 0 }
    val bitRate = format.bitrate.takeIf { it != Format.NO_VALUE && it > 0 }?.toLong()
    if (width == null && height == null && bitRate == null) {
        return null
    }
    return VesperVideoVariantObservation(
        bitRate = bitRate,
        width = width,
        height = height,
    )
}

fun resolveEffectiveVideoTrackId(
    videoTracks: List<VesperMediaTrack>,
    currentVideoFormat: Format?,
): String? {
    val format = currentVideoFormat ?: return null
    if (videoTracks.isEmpty()) {
        return null
    }

    val currentFormatId = format.id?.takeIf(String::isNotBlank)
    val exactFormatIdMatches =
        currentFormatId?.let { formatId ->
            videoTracks.filter { trackFormatIdComponent(it.id) == formatId }
        }.orEmpty()
    selectBestEffectiveVideoTrackMatch(exactFormatIdMatches, format)?.let { track ->
        return track.id
    }

    val width = format.width.takeIf { it != Format.NO_VALUE && it > 0 }
    val height = format.height.takeIf { it != Format.NO_VALUE && it > 0 }
    val bitRate = format.bitrate.takeIf { it != Format.NO_VALUE && it > 0 }?.toLong()
    val codec = nativeTrackCodec(format)

    if (width != null && height != null && bitRate != null) {
        val exactSizeAndBitRateMatches =
            videoTracks.filter { track ->
                track.width == width &&
                    track.height == height &&
                    track.bitRate == bitRate
            }
        selectBestEffectiveVideoTrackMatch(exactSizeAndBitRateMatches, format)?.let { track ->
            return track.id
        }
    }

    if (width != null && height != null && codec != null) {
        val exactSizeAndCodecMatches =
            videoTracks.filter { track ->
                track.width == width &&
                    track.height == height &&
                    track.codec == codec
            }
        selectBestEffectiveVideoTrackMatch(exactSizeAndCodecMatches, format)?.let { track ->
            return track.id
        }
    }

    if (bitRate != null && codec != null) {
        val exactBitRateAndCodecMatches =
            videoTracks.filter { track ->
                track.bitRate == bitRate &&
                    track.codec == codec
            }
        selectBestEffectiveVideoTrackMatch(exactBitRateAndCodecMatches, format)?.let { track ->
            return track.id
        }
    }

    return null
}

internal fun selectBestEffectiveVideoTrackMatch(
    candidates: List<VesperMediaTrack>,
    currentVideoFormat: Format,
): VesperMediaTrack? {
    if (candidates.isEmpty()) {
        return null
    }
    if (candidates.size == 1) {
        return candidates.first()
    }

    return candidates.minWithOrNull(
        compareBy<VesperMediaTrack> { track ->
            effectiveVideoTrackDistance(track.width, currentVideoFormat.width)
        }.thenBy { track ->
            effectiveVideoTrackDistance(track.height, currentVideoFormat.height)
        }.thenBy { track ->
            effectiveVideoTrackDistance(track.bitRate, currentVideoFormat.bitrate)
        }.thenBy { track ->
            effectiveVideoFrameRateDistance(track.frameRate, currentVideoFormat.frameRate)
        }.thenByDescending { track ->
            if (track.codec == nativeTrackCodec(currentVideoFormat)) 1 else 0
        }.thenBy { track ->
            track.id
        },
    )
}

internal fun effectiveVideoTrackDistance(trackValue: Int?, formatValue: Int): Long {
    if (formatValue == Format.NO_VALUE || formatValue <= 0) {
        return 0
    }
    val candidate = trackValue ?: return Long.MAX_VALUE / 4
    return kotlin.math.abs(candidate.toLong() - formatValue.toLong())
}

internal fun effectiveVideoTrackDistance(trackValue: Long?, formatValue: Int): Long {
    if (formatValue == Format.NO_VALUE || formatValue <= 0) {
        return 0
    }
    val candidate = trackValue ?: return Long.MAX_VALUE / 4
    return kotlin.math.abs(candidate - formatValue.toLong())
}

internal fun effectiveVideoFrameRateDistance(trackValue: Float?, formatValue: Float): Long {
    if (formatValue == FORMAT_NO_VALUE_FLOAT || !formatValue.isFinite() || formatValue <= 0f) {
        return 0
    }
    val candidate = trackValue ?: return Long.MAX_VALUE / 4
    return kotlin.math.abs(((candidate - formatValue) * 100).toLong())
}

internal fun trackFormatIdComponent(trackId: String): String? {
    val lastSeparatorIndex = trackId.lastIndexOf(':')
    if (lastSeparatorIndex <= 0) {
        return null
    }
    val secondLastSeparatorIndex = trackId.lastIndexOf(':', lastSeparatorIndex - 1)
    if (secondLastSeparatorIndex < 0 || secondLastSeparatorIndex + 1 >= lastSeparatorIndex) {
        return null
    }
    return trackId.substring(secondLastSeparatorIndex + 1, lastSeparatorIndex)
}
