package io.github.umbrella22.vesper.player.android

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.exoplayer.dash.manifest.AdaptationSet
import androidx.media3.exoplayer.dash.manifest.DashManifest
import androidx.media3.exoplayer.hls.HlsManifest
import androidx.media3.exoplayer.hls.playlist.HlsMultivariantPlaylist
import java.nio.charset.StandardCharsets
import java.util.Base64

/** Manifest-level subtitle declaration, before Media3 support filtering. */
internal data class NativeSubtitleManifestDeclaration(
    val id: String,
    val label: String?,
    val language: String?,
    val codec: String?,
    val isDefault: Boolean,
    val isForced: Boolean,
)

/**
 * Facts owned by the source manifest. [Tracks] is only a selectable view and
 * must not be used to reconstruct these counts after platform filtering.
 */
internal data class NativeSubtitleManifestInfo(
    val declarations: List<NativeSubtitleManifestDeclaration>,
    val defaultGroupCount: Int,
    val failure: NativeTrackSelectionFailure? = null,
) {
    val advertisedTrackCount: Int
        get() = declarations.size
}

internal fun subtitleManifestInfo(
    manifest: Any?,
    sourceProtocol: VesperPlayerSourceProtocol?,
): NativeSubtitleManifestInfo? =
    when (sourceProtocol) {
        VesperPlayerSourceProtocol.Dash -> (manifest as? DashManifest)?.let(::dashSubtitleManifestInfo)
        VesperPlayerSourceProtocol.Hls -> when (manifest) {
            is HlsManifest -> hlsSubtitleManifestInfo(manifest.multivariantPlaylist)
            is HlsMultivariantPlaylist -> hlsSubtitleManifestInfo(manifest)
            else -> null
        }
        else -> null
    }

/** Returns whether a source protocol has a manifest-owned subtitle catalog. */
internal fun subtitleManifestIsRequired(sourceProtocol: VesperPlayerSourceProtocol?): Boolean =
    sourceProtocol == VesperPlayerSourceProtocol.Dash ||
        sourceProtocol == VesperPlayerSourceProtocol.Hls

/**
 * Returns true only when Media3 exposed the typed manifest needed to derive
 * advertised subtitle declarations. A filtered `Tracks` snapshot alone is
 * never sufficient for DASH/HLS readiness.
 */
internal fun hasTypedSubtitleManifest(
    manifest: Any?,
    sourceProtocol: VesperPlayerSourceProtocol?,
): Boolean =
    when (sourceProtocol) {
        VesperPlayerSourceProtocol.Dash -> manifest is DashManifest
        VesperPlayerSourceProtocol.Hls ->
            manifest is HlsManifest || manifest is HlsMultivariantPlaylist
        else -> true
    }

private fun dashSubtitleManifestInfo(manifest: DashManifest): NativeSubtitleManifestInfo {
    val declarations = mutableListOf<NativeSubtitleManifestDeclaration>()
    val ids = mutableSetOf<String>()
    var defaultGroupCount = 0
    var failure: NativeTrackSelectionFailure? = null

    for (periodIndex in 0 until manifest.periodCount) {
        val period = manifest.getPeriod(periodIndex)
        for (adaptationSet in period.adaptationSets) {
            if (!isSubtitleAdaptationSet(adaptationSet)) continue
            val representations = adaptationSet.representations
            val groupIsDefault = representations.any { representation ->
                (representation.format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0
            }
            if (groupIsDefault) defaultGroupCount += 1
            representations.forEachIndexed { representationIndex, representation ->
                val format = representation.format
                val rawId = format.id?.takeIf { it.isNotBlank() }
                val publicId = rawId?.let { "subtitle:dash:$it" }
                if (rawId == null) {
                    failure =
                        failure ?: NativeTrackSelectionFailure(
                            kind = NativeTrackKind.Subtitle,
                            trackId = null,
                            code = "subtitle_track_identity_ambiguous",
                            phase = "identity",
                            message = "DASH subtitle representation id is missing",
                        )
                } else if (!ids.add(rawId)) {
                    failure =
                        failure ?: NativeTrackSelectionFailure(
                            kind = NativeTrackKind.Subtitle,
                            trackId = rawId,
                            code = "subtitle_track_identity_ambiguous",
                            phase = "identity",
                            message = "subtitle track identities are not unique",
                        )
                }
                declarations +=
                    NativeSubtitleManifestDeclaration(
                        id = publicId.orEmpty(),
                        label = format.label,
                        language = format.language?.takeUnless { it.equals("und", ignoreCase = true) },
                        codec = nativeTrackCodec(format),
                        // A default role belongs to the adaptation set. Only
                        // its first representation is the default catalog
                        // representative, matching the Rust HLS bridge.
                        isDefault = groupIsDefault && representationIndex == 0,
                        isForced = (format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0,
                    )
            }
        }
    }
    if (defaultGroupCount > 1) {
        failure =
            failure ?: NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = null,
                code = "subtitle_default_track_ambiguous",
                phase = "identity",
                message = "a subtitle group may contain at most one default track",
            )
    }
    return NativeSubtitleManifestInfo(declarations, defaultGroupCount, failure)
}

private fun hlsSubtitleManifestInfo(
    playlist: HlsMultivariantPlaylist,
): NativeSubtitleManifestInfo {
    val declarations = mutableListOf<NativeSubtitleManifestDeclaration>()
    val ids = mutableSetOf<String>()
    var failure: NativeTrackSelectionFailure? = null
    playlist.subtitles.groupBy { rendition -> rendition.groupId }.forEach { (_, renditions) ->
        val defaultRenditions = renditions.filter { rendition ->
            (rendition.format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0
        }
        if (defaultRenditions.size > 1) {
            failure =
                failure ?: NativeTrackSelectionFailure(
                    kind = NativeTrackKind.Subtitle,
                    trackId = null,
                    code = "subtitle_default_track_ambiguous",
                    phase = "identity",
                    message = "a subtitle group may contain at most one default track",
                )
        }
        renditions.forEachIndexed { index, rendition ->
            val format = rendition.format
            val identity = format.id?.takeIf { it.isNotBlank() }
                ?: "${rendition.groupId}:${rendition.name}"
            val publicId = subtitleTrackId(format, isDashSource = false)
                .takeIf { it.isNotBlank() }
                ?: "subtitle:hls:$identity"
            if (!ids.add(identity)) {
                failure =
                    failure ?: NativeTrackSelectionFailure(
                        kind = NativeTrackKind.Subtitle,
                        trackId = publicId,
                        code = "subtitle_track_identity_ambiguous",
                        phase = "identity",
                        message = "subtitle track identities are not unique",
                    )
            }
            declarations +=
                NativeSubtitleManifestDeclaration(
                    id = publicId,
                    label = rendition.name.takeIf { it.isNotBlank() } ?: format.label,
                    language = format.language?.takeUnless { it.equals("und", ignoreCase = true) },
                    codec = nativeTrackCodec(format),
                    isDefault = defaultRenditions.isNotEmpty() && index == 0,
                    isForced = (format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0,
                )
        }
    }
    // HLS group ids define independent logical subtitle groups. Do not merge
    // defaults from separate groups with caller-declared external tracks; the
    // per-group duplicate check above is the invariant this layer owns.
    return NativeSubtitleManifestInfo(declarations, defaultGroupCount = 0, failure)
}

private fun isSubtitleAdaptationSet(adaptationSet: AdaptationSet): Boolean =
    adaptationSet.type == C.TRACK_TYPE_TEXT ||
        adaptationSet.representations.any { representation ->
            MimeTypes.isText(representation.format.sampleMimeType)
        }

internal fun collectTrackCatalog(
    tracks: Tracks,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
    advertisedExternalSubtitleCount: Int = externalSubtitleIds.size,
    advertisedExternalDefaultCount: Int? = null,
    declaredExternalSubtitleIds: List<String> = externalSubtitleIds,
    manifestInfo: NativeSubtitleManifestInfo? = null,
    playbackPath: String? = null,
    surfaceKind: String? = null,
    decoderName: String? = null,
    hdrType: String? = null,
): NativeTrackCatalog {
    val trackInfos = mutableListOf<NativeTrackInfo>()
    val advertisedSubtitleIds = mutableSetOf<String>()
    val embeddedSubtitleIds = mutableSetOf<String>()
    val manifestDeclarations = manifestInfo?.declarations?.associateBy { declaration -> declaration.id }
    var subtitleIdentityFailure: NativeTrackSelectionFailure? = null
    var advertisedEmbeddedSubtitleTrackCount = 0
    var embeddedDefaultSubtitleTrackCount = 0
    var embeddedDefaultSubtitleGroupConflict = false
    var observedExternalDefaultSubtitleTrackCount = 0
    var adaptiveVideo = false
    var adaptiveAudio = false
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash

    tracks.groups.forEach { group ->
        val kind = nativeTrackKind(group.type) ?: return@forEach
        if (kind == NativeTrackKind.Subtitle) {
            var groupDefaultSubtitleTrackCount = 0
            for (trackIndex in 0 until group.length) {
                val format = group.getTrackFormat(trackIndex)
                val isExternal = externalSubtitleTrackId(format, externalSubtitleIds) != null
                val candidateId = subtitleTrackId(format, isDashSource, externalSubtitleIds)
                if (candidateId.isBlank()) {
                    subtitleIdentityFailure =
                        subtitleIdentityFailure
                            ?: NativeTrackSelectionFailure(
                                kind = NativeTrackKind.Subtitle,
                                trackId = format.id?.takeIf { it.isNotBlank() },
                                code = "subtitle_track_identity_ambiguous",
                                phase = "identity",
                                message =
                                    if (isDashSource) {
                                        "DASH subtitle representation id is missing"
                                    } else {
                                        "embedded subtitle metadata does not identify a unique track"
                                    },
                            )
                } else if (!advertisedSubtitleIds.add(candidateId)) {
                    subtitleIdentityFailure =
                        subtitleIdentityFailure
                            ?: NativeTrackSelectionFailure(
                                kind = NativeTrackKind.Subtitle,
                                trackId = format.id?.takeIf { it.isNotBlank() } ?: candidateId,
                                code = "subtitle_track_identity_ambiguous",
                                phase = "identity",
                                message = "subtitle track identities are not unique",
                            )
                }
                if (!isExternal) {
                    embeddedSubtitleIds += candidateId
                    advertisedEmbeddedSubtitleTrackCount += 1
                }
                val declaredDefault = manifestDeclarations?.get(candidateId)?.isDefault
                val isDefault = declaredDefault
                    ?: ((format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0)
                if (isDefault) {
                    if (isExternal) {
                        observedExternalDefaultSubtitleTrackCount += 1
                    } else {
                        embeddedDefaultSubtitleTrackCount += 1
                        groupDefaultSubtitleTrackCount += 1
                    }
                }
            }
            if (!isDashSource && groupDefaultSubtitleTrackCount > 1) {
                embeddedDefaultSubtitleGroupConflict = true
            }
        }
        if (kind == NativeTrackKind.Video && group.isAdaptiveSupported) {
            adaptiveVideo = true
        }
        if (kind == NativeTrackKind.Audio && group.isAdaptiveSupported) {
            adaptiveAudio = true
        }

        for (trackIndex in 0 until group.length) {
            // Video tracks remain visible even when Media3 says they exceed
            // the current capabilities or are unsupported. The public
            // support record explains why an explicit fixed-track request
            // will be rejected. Audio/text retain their existing selectable
            // filtering and subtitle identity rules.
            if (kind != NativeTrackKind.Video && !group.isTrackSupported(trackIndex, true)) {
                continue
            }
            val format = group.getTrackFormat(trackIndex)
            if (kind == NativeTrackKind.Subtitle &&
                subtitleTrackId(format, isDashSource, externalSubtitleIds) in unavailableExternalSubtitleIds
            ) {
                continue
            }
            // Subtitle identity is derived from the source-local external id,
            // DASH Representation@id, or stable media metadata. Positional
            // ids are never used for subtitle tracks.
            val trackId =
                if (kind == NativeTrackKind.Subtitle) {
                    val candidateId = subtitleTrackId(format, isDashSource, externalSubtitleIds)
                    candidateId.takeIf { it.isNotBlank() }
                        ?: nativeTrackId(group.mediaTrackGroup, trackIndex, format)
                } else {
                    nativeTrackId(group.mediaTrackGroup, trackIndex, format)
                }
            trackInfos +=
                NativeTrackInfo(
                    id = trackId,
                    kindOrdinal = kind.ordinal,
                    label = manifestDeclarations?.get(trackId)?.label ?: format.label,
                    language = manifestDeclarations?.get(trackId)?.language
                        ?: format.language?.takeUnless { it.equals("und", ignoreCase = true) },
                    codec = manifestDeclarations?.get(trackId)?.codec ?: nativeTrackCodec(format),
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
                    isDefault = manifestDeclarations?.get(trackId)?.isDefault
                        ?: ((format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0),
                    isForced = manifestDeclarations?.get(trackId)?.isForced
                        ?: ((format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0),
                    support =
                        trackSupportForFormatSupport(
                            formatSupport = group.trackSupportOrNull(trackIndex),
                            playbackPath = playbackPath,
                            surfaceKind = surfaceKind,
                            decoderName = decoderName,
                            hdrType = hdrType,
                        ),
                )
        }
    }

    val declaredManifestIds = manifestInfo?.declarations?.map { declaration -> declaration.id }.orEmpty()
    val declaredExternalIds = declaredExternalSubtitleIds.filter { it.isNotBlank() }
    val duplicateDeclaredExternalId =
        declaredExternalIds.groupingBy { it }.eachCount().entries.firstOrNull { entry -> entry.value > 1 }?.key
    val embeddedExternalConflict =
        (declaredManifestIds.toSet() intersect declaredExternalIds.toSet()).firstOrNull()
            ?: (embeddedSubtitleIds intersect declaredExternalIds.toSet()).firstOrNull()
    if (duplicateDeclaredExternalId != null || embeddedExternalConflict != null) {
        subtitleIdentityFailure =
            subtitleIdentityFailure ?: NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = duplicateDeclaredExternalId ?: embeddedExternalConflict,
                code = "subtitle_track_identity_ambiguous",
                phase = "identity",
                message = "subtitle track identities are not unique",
            )
    }

    manifestInfo?.failure?.let { failure ->
        subtitleIdentityFailure = subtitleIdentityFailure ?: failure
    }

    val advertisedSubtitleTrackCount =
        (manifestInfo?.advertisedTrackCount ?: advertisedEmbeddedSubtitleTrackCount) +
            advertisedExternalSubtitleCount
    val externalDefaultSubtitleTrackCount =
        advertisedExternalDefaultCount ?: observedExternalDefaultSubtitleTrackCount
    val defaultSubtitleTrackConflict =
        (manifestInfo == null && embeddedDefaultSubtitleGroupConflict) ||
            externalDefaultSubtitleTrackCount > 1 ||
            (manifestInfo == null && isDashSource &&
                embeddedDefaultSubtitleTrackCount + externalDefaultSubtitleTrackCount > 1) ||
            // DASH adaptation-set defaults are one logical group-level
            // contract, so multiple default groups are ambiguous. HLS
            // renditions, however, are validated per GROUP-ID by the
            // manifest inspector; defaults in separate HLS groups are valid
            // and must not be rejected as a global duplicate.
            (manifestInfo != null && isDashSource && manifestInfo.defaultGroupCount > 1)
    val catalogFailure =
        subtitleIdentityFailure
            ?: if (defaultSubtitleTrackConflict) {
                NativeTrackSelectionFailure(
                    kind = NativeTrackKind.Subtitle,
                    trackId = null,
                    code = "subtitle_default_track_ambiguous",
                    phase = "identity",
                    message = "a subtitle group may contain at most one default track",
                    advertisedTrackCount = advertisedSubtitleTrackCount,
                )
            } else {
                null
            }

    return NativeTrackCatalog(
        tracks = trackInfos
            .filterNot { catalogFailure != null && it.kindOrdinal == NativeTrackKind.Subtitle.ordinal }
            .toTypedArray(),
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
        subtitleIdentityFailure = catalogFailure?.copy(advertisedTrackCount = advertisedSubtitleTrackCount),
        advertisedSubtitleTrackCount = advertisedSubtitleTrackCount,
        playbackPath = playbackPath,
    )
}

internal fun Tracks.Group.trackSupportOrNull(trackIndex: Int): Int? =
    try {
        getTrackSupport(trackIndex)
    } catch (_: RuntimeException) {
        // A platform track query can fail while a renderer is being rebuilt.
        // Preserve the track with an explicit unknown result rather than
        // turning a transient query failure into an unsupported claim.
        null
    }

internal fun trackSupportForFormatSupport(
    formatSupport: Int?,
    playbackPath: String? = null,
    surfaceKind: String? = null,
    decoderName: String? = null,
    hdrType: String? = null,
): NativeTrackSupport {
    val (status, reason) =
        when (formatSupport) {
            C.FORMAT_HANDLED ->
                NativeTrackSupportStatus.Supported to NativeTrackSupportReason.None
            C.FORMAT_EXCEEDS_CAPABILITIES ->
                NativeTrackSupportStatus.ExceedsCapabilities to
                    NativeTrackSupportReason.FormatExceedsCapabilities
            C.FORMAT_UNSUPPORTED_DRM ->
                NativeTrackSupportStatus.Unsupported to NativeTrackSupportReason.UnsupportedDrm
            C.FORMAT_UNSUPPORTED_SUBTYPE ->
                NativeTrackSupportStatus.Unsupported to NativeTrackSupportReason.UnsupportedSubtype
            C.FORMAT_UNSUPPORTED_TYPE ->
                NativeTrackSupportStatus.Unsupported to NativeTrackSupportReason.UnsupportedType
            else -> NativeTrackSupportStatus.Unknown to NativeTrackSupportReason.PlatformUnknown
        }
    return NativeTrackSupport(
        statusOrdinal = status.ordinal,
        reasonOrdinal = reason.ordinal,
        sourceOrdinal = NativeTrackSupportSource.RuntimeTrackCatalog.ordinal,
        playbackPath = playbackPath,
        formatSupportRawValue = formatSupport?.formatSupportName(),
        decoderName = decoderName,
        surfaceKind = surfaceKind,
        hdrType = hdrType,
    )
}

internal fun NativeTrackCatalog.withCatalogRevision(revision: Long): NativeTrackCatalog =
    NativeTrackCatalog(
        tracks = tracks,
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
        subtitleIdentityFailure = subtitleIdentityFailure,
        advertisedSubtitleTrackCount = advertisedSubtitleTrackCount,
        catalogRevision = revision.coerceAtLeast(0L),
        playbackPath = playbackPath,
    )

private fun StringBuilder.appendFingerprintField(value: String?) {
    if (value == null) {
        append("-1:")
    } else {
        append(value.length).append(':').append(value)
    }
    append('|')
}

/**
 * Builds a session-local catalog identity without exposing manifest URLs or
 * other source secrets. Track order is normalized because Media3 may reorder
 * groups while retaining the same stable ids.
 */
internal fun NativeTrackCatalog.catalogFingerprint(
    sourceProtocol: VesperPlayerSourceProtocol?,
    surfaceKind: String?,
    route: String?,
    drmKeySystem: String?,
    catalogReady: Boolean,
    runtimeTrackRejectionKey: String? = null,
): String {
    val builder = StringBuilder()
    builder.appendFingerprintField(playbackPath)
    builder.appendFingerprintField(sourceProtocol?.name)
    builder.appendFingerprintField(surfaceKind)
    builder.appendFingerprintField(route)
    builder.appendFingerprintField(drmKeySystem)
    builder.appendFingerprintField(catalogReady.toString())
    builder.appendFingerprintField(runtimeTrackRejectionKey)
    builder.appendFingerprintField(adaptiveVideo.toString())
    builder.appendFingerprintField(adaptiveAudio.toString())
    builder.appendFingerprintField(advertisedSubtitleTrackCount.toString())
    tracks
        .sortedWith(compareBy<NativeTrackInfo> { it.kindOrdinal }.thenBy { it.id })
        .forEach { track ->
            builder.appendFingerprintField(track.id)
            builder.appendFingerprintField(track.kindOrdinal.toString())
            builder.appendFingerprintField(track.codec)
            builder.appendFingerprintField(track.width.takeIf { track.hasWidth }?.toString())
            builder.appendFingerprintField(track.height.takeIf { track.hasHeight }?.toString())
            builder.appendFingerprintField(track.bitRate.takeIf { track.hasBitRate }?.toString())
            builder.appendFingerprintField(track.support.statusOrdinal.toString())
            builder.appendFingerprintField(track.support.reasonOrdinal.toString())
            builder.appendFingerprintField(track.support.sourceOrdinal.toString())
            builder.appendFingerprintField(track.support.formatSupportRawValue)
            builder.appendFingerprintField(track.support.playbackPath)
        }
    return builder.toString()
}

internal fun collectTrackSelection(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
    subtitleModeOrdinal: Int? = null,
): NativeTrackSelectionSnapshotPayload =
    NativeTrackSelectionSnapshotPayload(
        video = collectTrackSelectionForType(C.TRACK_TYPE_VIDEO, tracks, parameters, sourceProtocol),
        audio = collectTrackSelectionForType(C.TRACK_TYPE_AUDIO, tracks, parameters, sourceProtocol),
        subtitle =
            collectTrackSelectionForType(
                C.TRACK_TYPE_TEXT,
                tracks,
                parameters,
                sourceProtocol,
                externalSubtitleIds,
                unavailableExternalSubtitleIds,
                subtitleModeOrdinal,
            ),
        abrPolicy = collectAbrPolicy(tracks, parameters),
    )

/**
 * Returns the subtitle preference that Media3 has accepted into its
 * [TrackSelectionParameters]. It intentionally does not use
 * [Tracks.Group.isTrackSelected]: that signal is renderer-active state and
 * can remain empty while a prepared player is paused.
 */
internal fun collectAppliedSubtitleSelection(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
    requestedModeOrdinal: Int? = null,
): VesperTrackSelection {
    if (parameters.disabledTrackTypes.contains(C.TRACK_TYPE_TEXT)) {
        return VesperTrackSelection.disabled()
    }

    val mode =
        requestedModeOrdinal
            ?.let { NativeTrackSelectionMode.entries.getOrNull(it) }
            ?: NativeTrackSelectionMode.Auto
    if (mode == NativeTrackSelectionMode.Disabled) {
        return VesperTrackSelection.disabled()
    }

    val override = currentOverrideForType(C.TRACK_TYPE_TEXT, tracks, parameters)
    val trackIndex = override?.trackIndices?.singleOrNull()
    val trackId =
        trackIndex?.let { index ->
            subtitleTrackId(
                override.mediaTrackGroup.getFormat(index),
                sourceProtocol == VesperPlayerSourceProtocol.Dash,
                externalSubtitleIds,
            ).takeIf { it.isNotBlank() && it !in unavailableExternalSubtitleIds }
        }

    return when (mode) {
        NativeTrackSelectionMode.Auto ->
            VesperTrackSelection(VesperTrackSelectionMode.Auto, trackId)
        NativeTrackSelectionMode.Track ->
            trackId?.let(VesperTrackSelection::track) ?: VesperTrackSelection.auto()
        NativeTrackSelectionMode.Disabled -> VesperTrackSelection.disabled()
    }
}

internal fun collectTrackSelectionForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
    requestedModeOrdinal: Int? = null,
): NativeTrackSelectionPayload {
    if (parameters.disabledTrackTypes.contains(trackType)) {
        return NativeTrackSelectionPayload(
            modeOrdinal = NativeTrackSelectionMode.Disabled.ordinal,
            trackId = null,
        )
    }

    val selectedTrackId =
        currentSelectedTrackId(
            trackType,
            tracks,
            sourceProtocol,
            externalSubtitleIds,
            unavailableExternalSubtitleIds,
        )
    val modeOrdinal =
        if (trackType == C.TRACK_TYPE_TEXT && requestedModeOrdinal != null) {
            requestedModeOrdinal
        } else if (trackType == C.TRACK_TYPE_TEXT && selectedTrackId == null) {
            NativeTrackSelectionMode.Disabled.ordinal
        } else {
            NativeTrackSelectionMode.Auto.ordinal
        }

    return NativeTrackSelectionPayload(
        modeOrdinal = modeOrdinal,
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
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
): String? {
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash
    tracks.groups.forEach { group ->
        if (group.type != trackType) return@forEach
        for (trackIndex in 0 until group.length) {
            if (group.isTrackSelected(trackIndex)) {
                val format = group.getTrackFormat(trackIndex)
                // Subtitle selections publish the stable id so the snapshot
                // a Flutter consumer observes matches the catalog id.
                if (trackType == C.TRACK_TYPE_TEXT) {
                    val identity = subtitleTrackId(format, isDashSource, externalSubtitleIds)
                    if (identity in unavailableExternalSubtitleIds) continue
                    val matchingIdentityCount =
                        tracks.groups.sumOf { candidateGroup ->
                            if (candidateGroup.type != C.TRACK_TYPE_TEXT) {
                                0
                            } else {
                                (0 until candidateGroup.length).count { candidateIndex ->
                                    subtitleTrackId(
                                        candidateGroup.getTrackFormat(candidateIndex),
                                        isDashSource,
                                        externalSubtitleIds,
                                    ).let { candidateIdentity ->
                                        candidateIdentity == identity &&
                                            candidateIdentity !in unavailableExternalSubtitleIds
                                    }
                                }
                            }
                        }
                    return identity.takeIf { it.isNotBlank() && matchingIdentityCount == 1 }
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

/** Returns the opaque, source-scoped identity used for a subtitle catalog entry. */
internal fun subtitleTrackId(
    format: Format,
    isDashSource: Boolean,
    externalSubtitleIds: List<String> = emptyList(),
): String {
    val formatId = format.id?.takeIf { it.isNotBlank() }
    externalSubtitleTrackId(format, externalSubtitleIds)?.let { return it }
    val primaryFormatId =
        formatId?.let { id ->
            if (externalSubtitleIds.isNotEmpty() && id.startsWith("0:")) {
                id.removePrefix("0:")
            } else {
                id
            }
        }
    if (isDashSource) {
        return primaryFormatId?.let { "subtitle:dash:$it" }.orEmpty()
    }
    // Media3's Format.id is the backend identity when present. Metadata such
    // as label/default may change across a manifest refresh and must not
    // silently rename the same track.
    val values =
        primaryFormatId?.let(::listOf)
            ?: listOf(
                format.language,
                format.label,
                format.sampleMimeType,
                format.containerMimeType,
                format.codecs,
                format.selectionFlags.toString(),
                format.roleFlags.toString(),
            )
    val payload = buildString {
        values.forEach { value ->
            val bytes = value?.toByteArray(StandardCharsets.UTF_8)
            if (bytes == null) {
                append("n:")
            } else {
                append(bytes.size).append(':').append(value)
            }
            append(';')
        }
    }
    val encoded =
        Base64.getUrlEncoder().withoutPadding()
            .encodeToString(payload.toByteArray(StandardCharsets.UTF_8))
    return "subtitle:media3:$encoded"
}

/** Resolves only caller-declared external identities from Media3 merge ids. */
private fun externalSubtitleTrackId(
    format: Format,
    externalSubtitleIds: List<String>,
): String? {
    val formatId = format.id?.takeIf { it.isNotBlank() } ?: return null
    externalSubtitleIds.forEachIndexed { index, externalId ->
        // MergingMediaPeriod prefixes each child Format.id with its child
        // index. Match the complete value so opaque ids containing ':' remain
        // lossless and cannot collide through suffix matching.
        if (formatId == externalId || formatId == "${index + 1}:$externalId") {
            return externalId
        }
    }
    return null
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
