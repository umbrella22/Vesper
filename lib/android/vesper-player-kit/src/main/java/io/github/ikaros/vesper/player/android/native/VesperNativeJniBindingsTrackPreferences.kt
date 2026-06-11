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
) {
    val trackType = media3TrackType(kind)
    val builder = exoPlayer.trackSelectionParameters.buildUpon()
    builder.clearOverridesOfType(trackType)

    when (selection.modeOrdinal) {
        NativeTrackSelectionMode.Auto.ordinal -> {
            builder.setTrackTypeDisabled(trackType, false)
        }
        NativeTrackSelectionMode.Disabled.ordinal -> {
            builder.setTrackTypeDisabled(trackType, true)
        }
        NativeTrackSelectionMode.Track.ordinal -> {
            val trackId = selection.trackId
            val override = trackId?.let { findTrackOverride(exoPlayer.currentTracks, trackType, it) }
            if (override == null) {
                Log.w(NATIVE_JNI_BINDINGS_TAG, "failed to find $kind track for id=${selection.trackId}")
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
): TrackSelectionOverride? {
    tracks.groups.forEach { group ->
        if (group.type != trackType) return@forEach
        for (trackIndex in 0 until group.length) {
            val format = group.getTrackFormat(trackIndex)
            if (nativeTrackId(group.mediaTrackGroup, trackIndex, format) == trackId) {
                return TrackSelectionOverride(group.mediaTrackGroup, trackIndex)
            }
        }
    }
    return null
}

internal fun media3TrackType(kind: NativeTrackKind): Int =
    when (kind) {
        NativeTrackKind.Video -> C.TRACK_TYPE_VIDEO
        NativeTrackKind.Audio -> C.TRACK_TYPE_AUDIO
        NativeTrackKind.Subtitle -> C.TRACK_TYPE_TEXT
    }

internal fun Long.clampToIntMax(): Int =
    coerceAtLeast(0L).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
