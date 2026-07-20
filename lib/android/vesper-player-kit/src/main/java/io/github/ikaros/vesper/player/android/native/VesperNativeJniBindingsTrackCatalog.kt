package io.github.ikaros.vesper.player.android

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks

internal fun collectTrackCatalog(
    tracks: Tracks,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
): NativeTrackCatalog {
    val trackInfos = mutableListOf<NativeTrackInfo>()
    val subtitleIds = mutableSetOf<String>()
    var subtitleIdentityFailure: NativeTrackSelectionFailure? = null
    var advertisedSubtitleTrackCount = 0
    var adaptiveVideo = false
    var adaptiveAudio = false
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash

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
            if (kind == NativeTrackKind.Subtitle && isDashSource) {
                advertisedSubtitleTrackCount += 1
            }
            // Subtitle public id must come from the manifest
            // `Representation@id` (Media3 `Format.id` for DASH
            // text tracks) so it survives source refresh, track reorder,
            // and resilience restore. The stable id is gated on DASH
            // sources only — non-DASH subtitle tracks (HLS CEA-608, MP4
            // embedded captions) keep the legacy positional id so they are
            // not mislabeled as `subtitle:dash:*`. Video/audio keep the
            // legacy position-derived `nativeTrackId` (plan: video/audio id
            // behavior is out of scope for this work).
            val trackId =
                if (kind == NativeTrackKind.Subtitle && isDashSource) {
                    val formatId = format.id
                    when {
                        formatId.isNullOrBlank() -> {
                            subtitleIdentityFailure =
                                subtitleIdentityFailure
                                    ?: NativeTrackSelectionFailure(
                                        kind = NativeTrackKind.Subtitle,
                                        trackId = null,
                                        code = "subtitle_track_identity_ambiguous",
                                        phase = "identity",
                                        message = "DASH subtitle representation id is missing",
                                    )
                            nativeTrackId(group.mediaTrackGroup, trackIndex, format)
                        }
                        !subtitleIds.add(formatId) -> {
                            subtitleIdentityFailure =
                                subtitleIdentityFailure
                                    ?: NativeTrackSelectionFailure(
                                        kind = NativeTrackKind.Subtitle,
                                        trackId = formatId,
                                        code = "subtitle_track_identity_ambiguous",
                                        phase = "identity",
                                        message = "DASH subtitle representation ids are not unique",
                                    )
                            nativeTrackId(group.mediaTrackGroup, trackIndex, format)
                        }
                        else -> subtitleStableTrackId(format)
                    }
                } else {
                    nativeTrackId(group.mediaTrackGroup, trackIndex, format)
                }
            trackInfos +=
                NativeTrackInfo(
                    id = trackId,
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
        tracks = trackInfos
            .filterNot { subtitleIdentityFailure != null && it.kindOrdinal == NativeTrackKind.Subtitle.ordinal }
            .toTypedArray(),
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
        subtitleIdentityFailure =
            subtitleIdentityFailure?.copy(
                advertisedTrackCount = advertisedSubtitleTrackCount,
            ),
    )
}

internal fun collectTrackSelection(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
): NativeTrackSelectionSnapshotPayload =
    NativeTrackSelectionSnapshotPayload(
        video = collectTrackSelectionForType(C.TRACK_TYPE_VIDEO, tracks, parameters, sourceProtocol),
        audio = collectTrackSelectionForType(C.TRACK_TYPE_AUDIO, tracks, parameters, sourceProtocol),
        subtitle = collectTrackSelectionForType(C.TRACK_TYPE_TEXT, tracks, parameters, sourceProtocol),
        abrPolicy = collectAbrPolicy(tracks, parameters),
    )

internal fun collectTrackSelectionForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
): NativeTrackSelectionPayload {
    if (parameters.disabledTrackTypes.contains(trackType)) {
        return NativeTrackSelectionPayload(
            modeOrdinal = NativeTrackSelectionMode.Disabled.ordinal,
            trackId = null,
        )
    }

    val selectedTrackId = currentSelectedTrackId(trackType, tracks, sourceProtocol)
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

internal fun currentSelectedTrackId(
    trackType: Int,
    tracks: Tracks,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
): String? {
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash
    tracks.groups.forEach { group ->
        if (group.type != trackType) return@forEach
        for (trackIndex in 0 until group.length) {
            if (group.isTrackSelected(trackIndex)) {
                val format = group.getTrackFormat(trackIndex)
                // Subtitle selections publish the stable id so the snapshot
                // a Flutter consumer observes matches the catalog id.
                if (trackType == C.TRACK_TYPE_TEXT && isDashSource) {
                    val formatId = format.id?.takeIf { it.isNotBlank() } ?: return null
                    val matchingIdentityCount =
                        tracks.groups.sumOf { candidateGroup ->
                            if (candidateGroup.type != C.TRACK_TYPE_TEXT) {
                                0
                            } else {
                                (0 until candidateGroup.length).count { candidateIndex ->
                                    candidateGroup.getTrackFormat(candidateIndex).id == formatId
                                }
                            }
                        }
                    return subtitleStableTrackId(format)
                        .takeIf { matchingIdentityCount == 1 }
                }
                return nativeTrackId(group.mediaTrackGroup, trackIndex, format)
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

/**
 * Stable public id for DASH subtitle tracks.
 *
 * Mirrors the iOS catalog convention `subtitle:dash:<representation id>` so
 * Android and iOS publish identical ids for the same manifest. The id is
 * stable across source refresh, track reorder, and resilience restore
 * because it derives only from the manifest-provided `Representation@id`
 * (Media3 surfaces this as `Format.id`). Returns an empty string when no
 * manifest id is present; catalog and selection callers treat that as an
 * identity failure rather than synthesizing a positional id.
 *
 * Used only for subtitle tracks. Video/audio id behavior is out of scope
 * and continues to use `nativeTrackId`.
 */
internal fun subtitleStableTrackId(format: Format): String {
    val formatId = format.id?.takeIf { it.isNotBlank() } ?: return ""
    return "subtitle:dash:$formatId"
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
