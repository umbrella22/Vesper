package io.github.ikaros.vesper.player.android

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks

internal fun collectTrackCatalog(tracks: Tracks): NativeTrackCatalog {
    val trackInfos = mutableListOf<NativeTrackInfo>()
    var adaptiveVideo = false
    var adaptiveAudio = false

    tracks.groups.forEach { group ->
        val kind = nativeTrackKind(group.type) ?: return@forEach
        if (kind == NativeTrackKind.Video && group.isAdaptiveSupported) {
            adaptiveVideo = true
        }
        if (kind == NativeTrackKind.Audio && group.isAdaptiveSupported) {
            adaptiveAudio = true
        }

        for (trackIndex in 0 until group.length) {
            if (!group.isTrackSupported(trackIndex, true)) {
                continue
            }
            val format = group.getTrackFormat(trackIndex)
            trackInfos +=
                NativeTrackInfo(
                    id = nativeTrackId(group.mediaTrackGroup, trackIndex, format),
                    kindOrdinal = kind.ordinal,
                    label = format.label,
                    language = format.language?.takeUnless { it.equals("und", ignoreCase = true) },
                    codec = nativeTrackCodec(format),
                    hasBitRate = format.bitrate != Format.NO_VALUE,
                    bitRate = format.bitrate.coerceAtLeast(0).toLong(),
                    hasWidth = format.width != Format.NO_VALUE,
                    width = format.width.coerceAtLeast(0),
                    hasHeight = format.height != Format.NO_VALUE,
                    height = format.height.coerceAtLeast(0),
                    hasFrameRate = format.frameRate != FORMAT_NO_VALUE_FLOAT,
                    frameRate =
                        if (format.frameRate != FORMAT_NO_VALUE_FLOAT) format.frameRate else 0f,
                    hasChannels = format.channelCount != Format.NO_VALUE,
                    channels = format.channelCount.coerceAtLeast(0),
                    hasSampleRate = format.sampleRate != Format.NO_VALUE,
                    sampleRate = format.sampleRate.coerceAtLeast(0),
                    isDefault = (format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0,
                    isForced = (format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0,
                )
        }
    }

    return NativeTrackCatalog(
        tracks = trackInfos.toTypedArray(),
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
    )
}

internal fun collectTrackSelection(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeTrackSelectionSnapshotPayload =
    NativeTrackSelectionSnapshotPayload(
        video = collectTrackSelectionForType(C.TRACK_TYPE_VIDEO, tracks, parameters),
        audio = collectTrackSelectionForType(C.TRACK_TYPE_AUDIO, tracks, parameters),
        subtitle = collectTrackSelectionForType(C.TRACK_TYPE_TEXT, tracks, parameters),
        abrPolicy = collectAbrPolicy(tracks, parameters),
    )

internal fun collectTrackSelectionForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeTrackSelectionPayload {
    if (parameters.disabledTrackTypes.contains(trackType)) {
        return NativeTrackSelectionPayload(
            modeOrdinal = NativeTrackSelectionMode.Disabled.ordinal,
            trackId = null,
        )
    }

    val override = currentOverrideForType(trackType, tracks, parameters)
    if (override != null) {
        val selectedTrackIndex = override.trackIndices.firstOrNull()
        return if (selectedTrackIndex != null) {
            NativeTrackSelectionPayload(
                modeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                trackId = nativeTrackId(
                    override.mediaTrackGroup,
                    selectedTrackIndex,
                    override.mediaTrackGroup.getFormat(selectedTrackIndex),
                ),
            )
        } else {
            NativeTrackSelectionPayload(
                modeOrdinal = NativeTrackSelectionMode.Disabled.ordinal,
                trackId = null,
            )
        }
    }

    val selectedTrackId = currentSelectedTrackId(trackType, tracks)
    val defaultMode =
        if (trackType == C.TRACK_TYPE_TEXT && selectedTrackId == null) {
            NativeTrackSelectionMode.Disabled
        } else {
            NativeTrackSelectionMode.Auto
        }

    return NativeTrackSelectionPayload(
        modeOrdinal = defaultMode.ordinal,
        trackId = selectedTrackId,
    )
}

internal fun collectAbrPolicy(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeAbrPolicyPayload {
    val videoOverride = currentOverrideForType(C.TRACK_TYPE_VIDEO, tracks, parameters)
    if (videoOverride != null) {
        val selectedTrackIndex = videoOverride.trackIndices.firstOrNull()
        return NativeAbrPolicyPayload(
            modeOrdinal = NativeAbrMode.FixedTrack.ordinal,
            trackId = selectedTrackIndex?.let {
                nativeTrackId(
                    videoOverride.mediaTrackGroup,
                    it,
                    videoOverride.mediaTrackGroup.getFormat(it),
                )
            },
            hasMaxBitRate = parameters.maxVideoBitrate != Int.MAX_VALUE,
            maxBitRate = parameters.maxVideoBitrate.coerceAtLeast(0).toLong(),
            hasMaxWidth = parameters.maxVideoWidth != Int.MAX_VALUE,
            maxWidth = parameters.maxVideoWidth.coerceAtLeast(0),
            hasMaxHeight = parameters.maxVideoHeight != Int.MAX_VALUE,
            maxHeight = parameters.maxVideoHeight.coerceAtLeast(0),
        )
    }

    val hasConstraints =
        parameters.forceLowestBitrate ||
            parameters.forceHighestSupportedBitrate ||
            parameters.maxVideoBitrate != Int.MAX_VALUE ||
            parameters.maxVideoWidth != Int.MAX_VALUE ||
            parameters.maxVideoHeight != Int.MAX_VALUE

    return NativeAbrPolicyPayload(
        modeOrdinal = if (hasConstraints) NativeAbrMode.Constrained.ordinal else NativeAbrMode.Auto.ordinal,
        trackId = null,
        hasMaxBitRate = parameters.maxVideoBitrate != Int.MAX_VALUE,
        maxBitRate = parameters.maxVideoBitrate.coerceAtLeast(0).toLong(),
        hasMaxWidth = parameters.maxVideoWidth != Int.MAX_VALUE,
        maxWidth = parameters.maxVideoWidth.coerceAtLeast(0),
        hasMaxHeight = parameters.maxVideoHeight != Int.MAX_VALUE,
        maxHeight = parameters.maxVideoHeight.coerceAtLeast(0),
    )
}

internal fun currentOverrideForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): TrackSelectionOverride? =
    parameters.overrides.values.firstOrNull { override ->
        override.type == trackType && currentTracksContainGroup(tracks, override.mediaTrackGroup)
    }

internal fun currentSelectedTrackId(trackType: Int, tracks: Tracks): String? {
    tracks.groups.forEach { group ->
        if (group.type != trackType) return@forEach
        for (trackIndex in 0 until group.length) {
            if (group.isTrackSelected(trackIndex)) {
                return nativeTrackId(group.mediaTrackGroup, trackIndex, group.getTrackFormat(trackIndex))
            }
        }
    }
    return null
}

internal fun currentTracksContainGroup(tracks: Tracks, trackGroup: TrackGroup): Boolean =
    tracks.groups.any { group -> group.mediaTrackGroup == trackGroup }

internal fun nativeTrackKind(trackType: Int): NativeTrackKind? =
    when (trackType) {
        C.TRACK_TYPE_VIDEO -> NativeTrackKind.Video
        C.TRACK_TYPE_AUDIO -> NativeTrackKind.Audio
        C.TRACK_TYPE_TEXT -> NativeTrackKind.Subtitle
        else -> null
    }

internal fun nativeTrackId(trackGroup: TrackGroup, trackIndex: Int, format: Format): String {
    val groupId =
        trackGroup.id.takeIf { it.isNotBlank() }
            ?: "type${trackGroup.type}"
    val formatId = format.id?.takeIf { it.isNotBlank() } ?: "track$trackIndex"
    return "$groupId:$formatId:$trackIndex"
}

internal fun nativeTrackCodec(format: Format): String? =
    format.codecs ?: format.sampleMimeType ?: format.containerMimeType

internal fun videoMimeType(format: Format): String? {
    format.sampleMimeType?.takeIf(MimeTypes::isVideo)?.let { return it }
    format.codecs
        ?.let(MimeTypes::getMediaMimeType)
        ?.takeIf(MimeTypes::isVideo)
        ?.let { return it }
    return format.containerMimeType?.takeIf(MimeTypes::isVideo)
}
