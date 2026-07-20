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
    val advertisedTrackCount: Int? = null,
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
        val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_TEXT, it) }
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
) {
    val trackType = media3TrackType(kind)
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    builder.clearOverridesOfType(trackType)

    when (selection.modeOrdinal) {
        NativeTrackSelectionMode.Auto.ordinal -> {
            if (kind == NativeTrackKind.Subtitle &&
                exoPlayer.currentTracks.groups.none { group ->
                    group.type == trackType &&
                        (0 until group.length).any { index -> group.isTrackSupported(index, true) }
                }
            ) {
                val failure = NativeTrackSelectionFailure(
                    kind = kind,
                    trackId = null,
                    code = "subtitle_auto_candidate_unavailable",
                    phase = "selection",
                    message = "no selectable subtitle candidate is available",
                )
                Log.w(NATIVE_JNI_BINDINGS_TAG, failure.message)
                onTrackSelectionFailure?.invoke(failure)
                return
            }
            builder.setTrackTypeDisabled(trackType, false)
        }
        NativeTrackSelectionMode.Disabled.ordinal -> {
            builder.setTrackTypeDisabled(trackType, true)
        }
        NativeTrackSelectionMode.Track.ordinal -> {
            val trackId = selection.trackId
            val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, trackType, it, sourceProtocol) }
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
                return
            }
            builder.setTrackTypeDisabled(trackType, false)
            if (kind == NativeTrackKind.Video) {
                resetAbrConstraints(builder)
            }
            builder.setOverrideForType(override)
        }
        else -> return
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
}

internal fun applyAbrPolicyCommand(
    exoPlayer: ExoPlayer,
    policy: NativeAbrPolicyPayload,
) {
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
            val trackId = policy.trackId
            val override =
                trackId?.let { findTrackOverride(exoPlayer.currentTracks, C.TRACK_TYPE_VIDEO, it) }
            if (override == null) {
                Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to find fixed ABR video track for id=${policy.trackId}")
                return
            }
            builder.setOverrideForType(override)
        }
        else -> return
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
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
): TrackSelectionOverride? {
    val isDashSource = sourceProtocol == VesperPlayerSourceProtocol.Dash
    var match: TrackSelectionOverride? = null
    var matchCount = 0
    for (group in tracks.groups) {
        if (group.type != trackType) continue
        for (trackIndex in 0 until group.length) {
            val format = group.getTrackFormat(trackIndex)
            // Subtitle lookups prefer the stable id (`subtitle:dash:<rep id>`)
            // so source-refresh / track-reorder do not break an existing
            // selection. The stable id is only computed for DASH sources to
            // match the catalog/selection gating; non-DASH subtitle tracks
            // and all video/audio tracks use only the positional
            // `nativeTrackId`. A DASH subtitle without a stable id is an
            // identity failure and must not be selected positionally.
            if (trackType == C.TRACK_TYPE_TEXT && isDashSource) {
                val stableId = subtitleStableTrackId(format)
                if (stableId.isNotEmpty() && stableId == trackId) {
                    match = TrackSelectionOverride(group.mediaTrackGroup, trackIndex)
                    matchCount += 1
                }
            } else if (nativeTrackId(group.mediaTrackGroup, trackIndex, format) == trackId) {
                match = TrackSelectionOverride(group.mediaTrackGroup, trackIndex)
                matchCount += 1
            }
        }
    }
    return match.takeIf { matchCount == 1 }
}

internal fun media3TrackType(kind: NativeTrackKind): Int =
    when (kind) {
        NativeTrackKind.Video -> C.TRACK_TYPE_VIDEO
        NativeTrackKind.Audio -> C.TRACK_TYPE_AUDIO
        NativeTrackKind.Subtitle -> C.TRACK_TYPE_TEXT
    }

internal fun Long.clampToIntMax(): Int =
    coerceAtLeast(0L).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
