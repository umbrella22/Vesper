package io.github.ikaros.vesper.player.android

import android.util.Log
import androidx.media3.common.C
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.exoplayer.ExoPlayer

internal fun hasTrackBasedPreferenceOverrides(policy: VesperTrackPreferencePolicy): Boolean =
    policy.audioSelection.mode == VesperTrackSelectionMode.Track ||
        policy.subtitleSelection.mode == VesperTrackSelectionMode.Track ||
        policy.abrPolicy.mode == VesperAbrMode.FixedTrack

/**
 * Structured track-selection failure produced when JNI cannot resolve a
 * requested track id against the current Media3 [Tracks] state. The bridge
 * forwards this to the runtime-warning channel so Flutter observes a
 * structured `subtitle_*` failure instead of a silent `Log.w` no-op.
 */
internal data class NativeTrackSelectionFailure(
    val kind: NativeTrackKind,
    val trackId: String?,
    val code: String,
    val phase: String,
    val message: String,
    val retriable: Boolean = false,
    val advertisedTrackCount: Int? = null,
    /** Callback identity used to reject delayed failures from an older command. */
    val sourceCallbackGeneration: Long? = null,
    val commandGeneration: Long? = null,
)

internal data class AutomaticSubtitleOverrideResult(
    val override: TrackSelectionOverride? = null,
    val failure: NativeTrackSelectionFailure? = null,
)

internal fun applyTrackPreferenceDefaults(
    exoPlayer: ExoPlayer,
    policy: VesperTrackPreferencePolicy,
    videoEnabled: Boolean = true,
) {
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    applyAudioPreferenceDefaults(builder, policy)
    applySubtitlePreferenceDefaults(builder, policy)
    if (videoEnabled) {
        applyAbrPreferenceDefaults(builder, policy.abrPolicy)
    } else {
        builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
        builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, true)
    }
    exoPlayer.setTrackSelectionParameters(builder.build())
}

internal fun applyAudioPreferenceDefaults(
    builder: TrackSelectionParameters.Builder,
    policy: VesperTrackPreferencePolicy,
) {
    when (policy.audioSelection.mode) {
        VesperTrackSelectionMode.Disabled -> builder.setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, true)
        VesperTrackSelectionMode.Auto,
        VesperTrackSelectionMode.Track,
        -> builder.setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
    }
    builder.setPreferredAudioLanguage(policy.preferredAudioLanguage)
}

internal fun applySubtitlePreferenceDefaults(
    builder: TrackSelectionParameters.Builder,
    policy: VesperTrackPreferencePolicy,
) {
    val shouldEnableText =
        when (policy.subtitleSelection.mode) {
            VesperTrackSelectionMode.Disabled -> false
            VesperTrackSelectionMode.Track -> true
            VesperTrackSelectionMode.Auto ->
                policy.selectSubtitlesByDefault ||
                    policy.selectUndeterminedSubtitleLanguage ||
                    !policy.preferredSubtitleLanguage.isNullOrBlank()
        }

    builder.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, !shouldEnableText)
    builder.setPreferredTextLanguage(policy.preferredSubtitleLanguage)
    builder.setSelectUndeterminedTextLanguage(policy.selectUndeterminedSubtitleLanguage)
    builder.setIgnoredTextSelectionFlags(0)
}

internal fun applyAbrPreferenceDefaults(
    builder: TrackSelectionParameters.Builder,
    policy: VesperAbrPolicy,
) {
    builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
    builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, false)
    resetAbrConstraints(builder)

    when (policy.mode) {
        VesperAbrMode.Auto,
        VesperAbrMode.FixedTrack,
        -> Unit
        VesperAbrMode.Constrained -> {
            policy.maxBitRate?.let { builder.setMaxVideoBitrate(it.clampToIntMax()) }
            if (policy.maxWidth != null || policy.maxHeight != null) {
                builder.setMaxVideoSize(
                    policy.maxWidth?.coerceAtLeast(0) ?: Int.MAX_VALUE,
                    policy.maxHeight?.coerceAtLeast(0) ?: Int.MAX_VALUE,
                )
            }
        }
    }
}

internal fun applyTrackPreferenceTrackOverrides(
    exoPlayer: ExoPlayer,
    policy: VesperTrackPreferencePolicy,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
) {
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    var hasChanges = false

    if (policy.audioSelection.mode == VesperTrackSelectionMode.Track) {
        val trackId = policy.audioSelection.trackId
        val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_AUDIO, it) }
        if (override != null) {
            builder.clearOverridesOfType(C.TRACK_TYPE_AUDIO)
            builder.setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
            builder.setOverrideForType(override)
            hasChanges = true
        } else {
            Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to apply default audio track preference id=$trackId")
        }
    }

    if (policy.subtitleSelection.mode == VesperTrackSelectionMode.Track) {
        val trackId = policy.subtitleSelection.trackId
        val override =
            trackId?.let {
                findTrackOverride(
                    exoPlayer.currentTracks,
                    C.TRACK_TYPE_TEXT,
                    it,
                    sourceProtocol,
                    externalSubtitleIds,
                    unavailableExternalSubtitleIds,
                )
            }
        if (override != null) {
            builder.clearOverridesOfType(C.TRACK_TYPE_TEXT)
            builder.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            builder.setOverrideForType(override)
            hasChanges = true
        } else {
            Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to apply default subtitle track preference id=$trackId")
        }
    }

    if (policy.abrPolicy.mode == VesperAbrMode.FixedTrack) {
        val trackId = policy.abrPolicy.trackId
        val override =
            trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_VIDEO, it) }
        if (override != null) {
            builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
            builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, false)
            resetAbrConstraints(builder)
            builder.setOverrideForType(override)
            hasChanges = true
        } else {
            Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to apply default fixed ABR track preference id=$trackId")
        }
    }

    if (hasChanges) {
        exoPlayer.setTrackSelectionParameters(builder.build())
    }
}

internal fun applyTrackSelectionCommand(
    exoPlayer: ExoPlayer,
    kind: NativeTrackKind,
    selection: NativeTrackSelectionPayload,
    onTrackSelectionFailure: ((NativeTrackSelectionFailure) -> Unit)? = null,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
): Boolean {
    val trackType = media3TrackType(kind)
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    builder.clearOverridesOfType(trackType)

    when (selection.modeOrdinal) {
        NativeTrackSelectionMode.Auto.ordinal -> {
            if (kind == NativeTrackKind.Subtitle) {
                val resolution =
                    findAutomaticSubtitleOverride(
                        exoPlayer.currentTracks,
                        exoPlayer.trackSelectionParameters,
                        sourceProtocol,
                        externalSubtitleIds,
                        unavailableExternalSubtitleIds,
                    )
                resolution.failure?.let { failure ->
                    Log.w(NATIVE_JNI_BINDINGS_TAG, failure.code)
                    onTrackSelectionFailure?.invoke(failure)
                    return false
                }
                val override = resolution.override
                if (override == null) {
                    val failure = NativeTrackSelectionFailure(
                        kind = kind,
                        trackId = null,
                        code = "subtitle_auto_candidate_unavailable",
                        phase = "selection",
                        message = "no selectable subtitle candidate is available",
                    )
                    Log.w(NATIVE_JNI_BINDINGS_TAG, failure.message)
                    onTrackSelectionFailure?.invoke(failure)
                    return false
                }
                builder.setOverrideForType(override)
            }
            builder.setTrackTypeDisabled(trackType, false)
        }
        NativeTrackSelectionMode.Disabled.ordinal -> {
            builder.setTrackTypeDisabled(trackType, true)
        }
        NativeTrackSelectionMode.Track.ordinal -> {
            val trackId = selection.trackId
            val override =
                trackId?.let {
                    findTrackOverride(
                        exoPlayer.currentTracks,
                        trackType,
                        it,
                        sourceProtocol,
                        externalSubtitleIds,
                        unavailableExternalSubtitleIds,
                    )
                }
            if (override == null) {
                // A Track-mode lookup failure must surface as a structured
                // runtime warning, not a silent Log.w. The
                // subtitle-specific `subtitle_track_not_found` code is
                // emitted only for subtitle lookups; audio/video keep their
                // generic message so the new channel does not invent new
                // contracts for them.
                val code = if (kind == NativeTrackKind.Subtitle) "subtitle_track_not_found" else "track_not_found"
                val failure = NativeTrackSelectionFailure(
                    kind = kind,
                    trackId = trackId,
                    code = code,
                    phase = "selection",
                    message = "failed to find $kind track for id=$trackId",
                )
                Log.w(NATIVE_JNI_BINDINGS_TAG, failure.code)
                onTrackSelectionFailure?.invoke(failure)
                return false
            }
            builder.setTrackTypeDisabled(trackType, false)
            if (kind == NativeTrackKind.Video) {
                resetAbrConstraints(builder)
            }
            builder.setOverrideForType(override)
        }
        else -> return false
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
    return true
}

internal fun applyAbrPolicyCommand(
    exoPlayer: ExoPlayer,
    policy: NativeAbrPolicyPayload,
    expectedCatalogRevision: Long? = null,
    actualCatalogRevision: Long? = null,
    sourceEpoch: Long? = null,
    runtimeTrackRejection: NativeRuntimeTrackRejection? = null,
    playbackPath: String? = "systemPlayer",
    surfaceKind: String? = null,
    decoderName: String? = null,
    hdrType: String? = null,
) {
    val fixedTrackLocation =
        if (policy.modeOrdinal == NativeAbrMode.FixedTrack.ordinal) {
            validateFixedTrackSelection(
                tracks = exoPlayer.currentTracks,
                trackId = policy.trackId,
                expectedCatalogRevision = expectedCatalogRevision,
                actualCatalogRevision = actualCatalogRevision,
                sourceEpoch = sourceEpoch,
                runtimeTrackRejection = runtimeTrackRejection,
                playbackPath = playbackPath,
                surfaceKind = surfaceKind,
                decoderName = decoderName,
                hdrType = hdrType,
            )
        } else {
            null
        }

    // Do not build or mutate selection parameters until fixed-track
    // validation above has passed. This preserves the effective player state
    // on every structured rejection.
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    builder.clearOverridesOfType(C.TRACK_TYPE_VIDEO)
    builder.setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, false)
    resetAbrConstraints(builder)

    when (policy.modeOrdinal) {
        NativeAbrMode.Auto.ordinal -> Unit
        NativeAbrMode.Constrained.ordinal -> {
            if (policy.hasMaxBitRate) {
                builder.setMaxVideoBitrate(policy.maxBitRate.clampToIntMax())
            }
            if (policy.hasMaxWidth || policy.hasMaxHeight) {
                builder.setMaxVideoSize(
                    if (policy.hasMaxWidth) policy.maxWidth.coerceAtLeast(0) else Int.MAX_VALUE,
                    if (policy.hasMaxHeight) policy.maxHeight.coerceAtLeast(0) else Int.MAX_VALUE,
                )
            }
        }
        NativeAbrMode.FixedTrack.ordinal -> {
            val location =
                fixedTrackLocation
                    ?: throw IllegalStateException("validated fixed-track location is missing")
            builder.setOverrideForType(
                TrackSelectionOverride(
                    location.group.mediaTrackGroup,
                    location.trackIndex,
                )
            )
        }
        else -> return
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
}

internal fun validateFixedTrackSelection(
    tracks: Tracks,
    trackId: String?,
    expectedCatalogRevision: Long?,
    actualCatalogRevision: Long?,
    sourceEpoch: Long? = null,
    runtimeTrackRejection: NativeRuntimeTrackRejection? = null,
    playbackPath: String? = "systemPlayer",
    surfaceKind: String? = null,
    decoderName: String? = null,
    hdrType: String? = null,
): NativeTrackLocation {
    if (expectedCatalogRevision != null &&
        actualCatalogRevision != null &&
        expectedCatalogRevision != actualCatalogRevision
    ) {
        throw VesperFixedTrackSelectionException(
            code = "staleCatalog",
            trackId = trackId,
            expectedCatalogRevision = expectedCatalogRevision,
            actualCatalogRevision = actualCatalogRevision,
            message = "the track catalog changed before the fixed-track command was applied",
        )
    }
    val location =
        trackId?.let {
            findTrackLocation(
                tracks,
                C.TRACK_TYPE_VIDEO,
                it,
            )
        }
            ?: throw VesperFixedTrackSelectionException(
                code = "trackUnavailable",
                trackId = trackId,
                expectedCatalogRevision = expectedCatalogRevision,
                actualCatalogRevision = actualCatalogRevision,
                message = "the requested video track is not in the current catalog",
            )
    val support =
        trackSupportForFormatSupport(
            formatSupport = location.group.trackSupportOrNull(location.trackIndex),
            playbackPath = playbackPath,
            surfaceKind = surfaceKind,
            decoderName = decoderName,
            hdrType = hdrType,
        )
    val status = NativeTrackSupportStatus.entries.getOrNull(support.statusOrdinal)
    val rejectionCode =
        when (status) {
            NativeTrackSupportStatus.ExceedsCapabilities -> "trackExceedsCapabilities"
            NativeTrackSupportStatus.Unsupported -> "trackUnsupported"
            NativeTrackSupportStatus.Supported,
            NativeTrackSupportStatus.Unknown,
            null,
            -> null
        }
    if (rejectionCode != null) {
        val statusMessage =
            if (rejectionCode == "trackExceedsCapabilities") {
                "the requested video track exceeds current playback capabilities"
            } else {
                "the requested video track is unsupported by the active playback path"
            }
        throw VesperFixedTrackSelectionException(
            code = rejectionCode,
            trackId = trackId,
            expectedCatalogRevision = expectedCatalogRevision,
            actualCatalogRevision = actualCatalogRevision,
            message = statusMessage,
            extraDetails =
                mapOf(
                    "reason" to
                        (support.reasonRawValue
                            ?: NativeTrackSupportReason.entries
                                .getOrNull(support.reasonOrdinal)
                                ?.toWireName()
                            ?: support.reasonOrdinal.toString()),
                    "formatSupportRawValue" to support.formatSupportRawValue,
                ),
        )
    }
    if (runtimeTrackRejection != null &&
        sourceEpoch != null &&
        runtimeTrackRejection.sourceEpoch == sourceEpoch &&
        runtimeTrackRejection.trackId == trackId
    ) {
        throw VesperFixedTrackSelectionException(
            code = runtimeTrackRejection.code,
            trackId = trackId,
            expectedCatalogRevision = expectedCatalogRevision,
            actualCatalogRevision = actualCatalogRevision,
            message = "the requested video track was rejected by the active playback session",
            extraDetails =
                runtimeTrackRejection.details -
                    setOf(
                        "domain",
                        "code",
                        "trackId",
                        "expectedCatalogRevision",
                        "actualCatalogRevision",
                        "message",
                    ),
        )
    }
    return location
}

private fun NativeTrackSupportReason.toWireName(): String =
    when (this) {
        NativeTrackSupportReason.None -> "none"
        NativeTrackSupportReason.FormatExceedsCapabilities -> "formatExceedsCapabilities"
        NativeTrackSupportReason.UnsupportedType -> "unsupportedType"
        NativeTrackSupportReason.UnsupportedSubtype -> "unsupportedSubtype"
        NativeTrackSupportReason.UnsupportedDrm -> "unsupportedDrm"
        NativeTrackSupportReason.RouteUnavailable -> "routeUnavailable"
        NativeTrackSupportReason.PresentationUnavailable -> "presentationUnavailable"
        NativeTrackSupportReason.RuntimeFailure -> "runtimeFailure"
        NativeTrackSupportReason.PlatformUnknown -> "platformUnknown"
        NativeTrackSupportReason.Unknown -> "unknown"
    }

internal fun resetAbrConstraints(builder: TrackSelectionParameters.Builder) {
    builder.setForceLowestBitrate(false)
    builder.setForceHighestSupportedBitrate(false)
    builder.setMaxVideoBitrate(Int.MAX_VALUE)
    builder.setMaxVideoSize(Int.MAX_VALUE, Int.MAX_VALUE)
}

internal fun findTrackOverride(
    tracks: Tracks,
    trackType: Int,
    trackId: String,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
): TrackSelectionOverride? {
    return findTrackLocation(
        tracks,
        trackType,
        trackId,
        sourceProtocol,
        externalSubtitleIds,
        unavailableExternalSubtitleIds,
    )?.let { location ->
        TrackSelectionOverride(location.group.mediaTrackGroup, location.trackIndex)
    }
}

internal fun isSubtitleTrackSelectable(
    tracks: Tracks,
    trackId: String,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
): Boolean {
    val location =
        findTrackLocation(
            tracks = tracks,
            trackType = C.TRACK_TYPE_TEXT,
            trackId = trackId,
            sourceProtocol = sourceProtocol,
            externalSubtitleIds = externalSubtitleIds,
            unavailableExternalSubtitleIds = unavailableExternalSubtitleIds,
        ) ?: return false
    return location.group.isTrackSupported(location.trackIndex, true)
}

internal data class NativeTrackLocation(
    val group: Tracks.Group,
    val trackIndex: Int,
)

internal fun findTrackLocation(
    tracks: Tracks,
    trackType: Int,
    trackId: String,
    sourceProtocol: VesperPlayerSourceProtocol? = null,
    externalSubtitleIds: List<String> = emptyList(),
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
): NativeTrackLocation? {
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash
    var match: NativeTrackLocation? = null
    var matchCount = 0
    for (group in tracks.groups) {
        if (group.type != trackType) continue
        for (trackIndex in 0 until group.length) {
            val format = group.getTrackFormat(trackIndex)
            if (trackType == C.TRACK_TYPE_TEXT) {
                val stableId = subtitleTrackId(format, isDashSource, externalSubtitleIds)
                if (stableId.isNotEmpty() &&
                    stableId !in unavailableExternalSubtitleIds &&
                    stableId == trackId
                ) {
                    match = NativeTrackLocation(group, trackIndex)
                    matchCount += 1
                }
            } else if (nativeTrackId(group.mediaTrackGroup, trackIndex, format) == trackId) {
                match = NativeTrackLocation(group, trackIndex)
                matchCount += 1
            }
        }
    }
    return match.takeIf { matchCount == 1 }
}

internal fun findAutomaticSubtitleOverride(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
    sourceProtocol: VesperPlayerSourceProtocol?,
    externalSubtitleIds: List<String>,
    unavailableExternalSubtitleIds: Set<String> = emptySet(),
): AutomaticSubtitleOverrideResult {
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash
    data class Candidate(
        val id: String,
        val override: TrackSelectionOverride,
        val languageRank: Int,
        val isDefault: Boolean,
        val isForced: Boolean,
    )
    val candidates = mutableListOf<Candidate>()
    tracks.groups.forEach { group ->
        if (group.type != C.TRACK_TYPE_TEXT) return@forEach
        for (index in 0 until group.length) {
            if (!group.isTrackSupported(index, true)) continue
            val format = group.getTrackFormat(index)
            val id = subtitleTrackId(format, isDashSource, externalSubtitleIds)
            if (id.isBlank() || id in unavailableExternalSubtitleIds) continue
            val languageRank =
                parameters.preferredTextLanguages.indexOfFirst { preferred ->
                    preferred.equals(format.language, ignoreCase = true) ||
                        preferred.substringBefore('-')
                            .equals(format.language?.substringBefore('-'), ignoreCase = true)
                }.let { if (it < 0) Int.MAX_VALUE else it }
            candidates +=
                Candidate(
                    id = id,
                    override = TrackSelectionOverride(group.mediaTrackGroup, index),
                    languageRank = languageRank,
                    isDefault = (format.selectionFlags and C.SELECTION_FLAG_DEFAULT) != 0,
                    isForced = (format.selectionFlags and C.SELECTION_FLAG_FORCED) != 0,
                )
        }
    }
    val duplicateId =
        candidates
            .groupingBy { it.id }
            .eachCount()
            .entries
            .firstOrNull { it.value > 1 }
            ?.key
    if (duplicateId != null) {
        return AutomaticSubtitleOverrideResult(
            failure = NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = duplicateId,
                code = "subtitle_track_identity_ambiguous",
                phase = "identity",
                message = "multiple selectable subtitle tracks resolve to the same identity",
                advertisedTrackCount = candidates.size,
            ),
        )
    }

    val override = candidates
        .sortedWith(
            compareBy<Candidate> { it.languageRank }
                .thenByDescending { it.isDefault }
                .thenBy { it.isForced }
                .thenBy { it.id },
        )
        .firstOrNull()
        ?.override
    return AutomaticSubtitleOverrideResult(override = override)
}

internal fun media3TrackType(kind: NativeTrackKind): Int =
    when (kind) {
        NativeTrackKind.Video -> C.TRACK_TYPE_VIDEO
        NativeTrackKind.Audio -> C.TRACK_TYPE_AUDIO
        NativeTrackKind.Subtitle -> C.TRACK_TYPE_TEXT
    }

internal fun Long.clampToIntMax(): Int =
    coerceAtLeast(0L).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
