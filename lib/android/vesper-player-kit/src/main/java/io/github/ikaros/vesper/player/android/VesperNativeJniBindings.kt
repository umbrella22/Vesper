package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer

class VesperNativeJniBindings(
    context: Context,
) : VesperNativeBindings {
    private val appContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())

    private var sessionHandle: Long? = null
    private var player: ExoPlayer? = null
    private var playerListener: Player.Listener? = null
    private var attachedSurface: Surface? = null
    private var updateListener: (() -> Unit)? = null
    private var currentTrackCatalogState: VesperTrackCatalog = VesperTrackCatalog.Empty
    private var currentTrackSelectionState: VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
    private var currentVideoLayoutState: NativeVideoLayoutInfo? = null

    override fun initialize(source: VesperPlayerSource): NativeBridgeStartup {
        Log.i(TAG, "initialize source=${source.uri} kind=${source.kind} protocol=${source.protocol}")
        dispose()
        VesperNativeLibrary.ensureLoaded()

        val handle = VesperNativeJni.createSession(source.uri)
        check(handle != 0L) { "native session handle must not be zero" }
        sessionHandle = handle

        val exoPlayer =
            ExoPlayer.Builder(appContext)
                .setLoadControl(buildLoadControl(source))
                .build()
        val listener = buildPlayerListener()
        exoPlayer.addListener(listener)
        exoPlayer.setMediaItem(buildMediaItem(source))
        attachedSurface?.let { surface ->
            Log.i(TAG, "reusing attached surface for source=${source.uri}")
            exoPlayer.setVideoSurface(surface)
        }
        exoPlayer.prepare()

        player = exoPlayer
        playerListener = listener

        pushSnapshotToRust()
        pushTrackStateToRust()
        notifyNativeUpdate()

        return NativeBridgeStartup(
            subtitle = sourceSubtitle(source),
        )
    }

    override fun dispose() {
        Log.i(TAG, "dispose")
        detachSurface()
        playerListener?.let { listener ->
            player?.removeListener(listener)
        }
        playerListener = null
        player?.release()
        player = null
        sessionHandle?.let(VesperNativeJni::disposeSession)
        sessionHandle = null
        currentTrackCatalogState = VesperTrackCatalog.Empty
        currentTrackSelectionState = VesperTrackSelectionSnapshot()
        currentVideoLayoutState = null
    }

    override fun refreshSnapshot() {
        pushSnapshotToRust()
    }

    override fun currentTrackCatalog(): VesperTrackCatalog = currentTrackCatalogState

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = currentTrackSelectionState

    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = currentVideoLayoutState

    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) {
        updateListener = listener
    }

    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) {
        Log.i(TAG, "attachSurface kind=$surfaceKind")
        attachedSurface = surface
        player?.setVideoSurface(surface)
        sessionHandle?.let { handle ->
            VesperNativeJni.attachSurface(handle, surface, surfaceKind.ordinal)
        }
        pushSnapshotToRust()
        notifyNativeUpdate()
    }

    override fun detachSurface() {
        Log.i(TAG, "detachSurface")
        player?.clearVideoSurface()
        attachedSurface = null
        sessionHandle?.let(VesperNativeJni::detachSurface)
        notifyNativeUpdate()
    }

    override fun pollSnapshot(): NativeBridgeSnapshot? =
        sessionHandle?.let(VesperNativeJni::pollSnapshot)

    override fun drainEvents(): List<NativeBridgeEvent> =
        sessionHandle?.let { VesperNativeJni.drainEvents(it).toList() } ?: emptyList()

    override fun play() {
        Log.i(TAG, "play")
        dispatchRustCommand { handle -> VesperNativeJni.play(handle) }
    }

    override fun pause() {
        Log.i(TAG, "pause")
        dispatchRustCommand { handle -> VesperNativeJni.pause(handle) }
    }

    override fun stop() {
        Log.i(TAG, "stop")
        dispatchRustCommand { handle -> VesperNativeJni.stop(handle) }
    }

    override fun seekTo(positionMs: Long) {
        Log.i(TAG, "seekTo positionMs=$positionMs")
        dispatchRustCommand { handle -> VesperNativeJni.seekTo(handle, positionMs) }
    }

    override fun setPlaybackRate(rate: Float) {
        Log.i(TAG, "setPlaybackRate rate=$rate")
        dispatchRustCommand { handle -> VesperNativeJni.setPlaybackRate(handle, rate) }
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        Log.i(TAG, "setVideoTrackSelection mode=${selection.mode} trackId=${selection.trackId}")
        dispatchRustCommand { handle ->
            VesperNativeJni.setVideoTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        Log.i(TAG, "setAudioTrackSelection mode=${selection.mode} trackId=${selection.trackId}")
        dispatchRustCommand { handle ->
            VesperNativeJni.setAudioTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        Log.i(
            TAG,
            "setSubtitleTrackSelection mode=${selection.mode} trackId=${selection.trackId}",
        )
        dispatchRustCommand { handle ->
            VesperNativeJni.setSubtitleTrackSelection(handle, selection.toNativePayload())
        }
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        Log.i(
            TAG,
            "setAbrPolicy mode=${policy.mode} trackId=${policy.trackId} maxBitRate=${policy.maxBitRate} maxWidth=${policy.maxWidth} maxHeight=${policy.maxHeight}",
        )
        dispatchRustCommand { handle ->
            VesperNativeJni.setAbrPolicy(handle, policy.toNativePayload())
        }
    }

    private fun dispatchRustCommand(action: (Long) -> Unit) {
        val handle = sessionHandle ?: return
        action(handle)
        drainAndApplyNativeCommands()
        pushSnapshotToRust()
        pushTrackStateToRust()
        notifyNativeUpdate()
    }

    private fun drainAndApplyNativeCommands() {
        val handle = sessionHandle ?: return
        val exoPlayer = player ?: return

        VesperNativeJni.drainNativeCommands(handle).forEach { command ->
            when (command) {
                NativePlayerCommand.Play -> {
                    Log.d(TAG, "apply native command: Play")
                    exoPlayer.play()
                }
                NativePlayerCommand.Pause -> {
                    Log.d(TAG, "apply native command: Pause")
                    exoPlayer.pause()
                }
                is NativePlayerCommand.SeekTo -> {
                    Log.d(TAG, "apply native command: SeekTo positionMs=${command.positionMs}")
                    exoPlayer.seekTo(command.positionMs)
                }
                NativePlayerCommand.Stop -> {
                    Log.d(TAG, "apply native command: Stop")
                    exoPlayer.pause()
                    exoPlayer.seekTo(0L)
                }
                is NativePlayerCommand.SetPlaybackRate -> {
                    Log.d(TAG, "apply native command: SetPlaybackRate rate=${command.rate}")
                    exoPlayer.setPlaybackParameters(PlaybackParameters(command.rate))
                }
                is NativePlayerCommand.SetVideoTrackSelection -> {
                    Log.d(
                        TAG,
                        "apply native command: SetVideoTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                    )
                    applyTrackSelectionCommand(
                        exoPlayer = exoPlayer,
                        kind = NativeTrackKind.Video,
                        selection = command.selection,
                    )
                }
                is NativePlayerCommand.SetAudioTrackSelection -> {
                    Log.d(
                        TAG,
                        "apply native command: SetAudioTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                    )
                    applyTrackSelectionCommand(
                        exoPlayer = exoPlayer,
                        kind = NativeTrackKind.Audio,
                        selection = command.selection,
                    )
                }
                is NativePlayerCommand.SetSubtitleTrackSelection -> {
                    Log.d(
                        TAG,
                        "apply native command: SetSubtitleTrackSelection mode=${command.selection.modeOrdinal} trackId=${command.selection.trackId}",
                    )
                    applyTrackSelectionCommand(
                        exoPlayer = exoPlayer,
                        kind = NativeTrackKind.Subtitle,
                        selection = command.selection,
                    )
                }
                is NativePlayerCommand.SetAbrPolicy -> {
                    Log.d(
                        TAG,
                        "apply native command: SetAbrPolicy mode=${command.policy.modeOrdinal} trackId=${command.policy.trackId}",
                    )
                    applyAbrPolicyCommand(exoPlayer, command.policy)
                }
            }
        }
    }

    private fun buildPlayerListener(): Player.Listener =
        object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) {
                Log.d(
                    TAG,
                    "onPlaybackStateChanged state=${exoPlaybackStateName(playbackState)} playWhenReady=${player?.playWhenReady}",
                )
                pushSnapshotToRust()
                notifyNativeUpdate()
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                Log.d(TAG, "onPlayWhenReadyChanged playWhenReady=$playWhenReady reason=$reason")
                pushSnapshotToRust()
                notifyNativeUpdate()
            }

            override fun onPlaybackParametersChanged(playbackParameters: PlaybackParameters) {
                Log.d(TAG, "onPlaybackParametersChanged speed=${playbackParameters.speed}")
                pushSnapshotToRust()
                pushTrackStateToRust()
                notifyNativeUpdate()
            }

            override fun onTracksChanged(tracks: Tracks) {
                Log.d(TAG, "onTracksChanged groups=${tracks.groups.size}")
                pushTrackStateToRust()
                notifyNativeUpdate()
            }

            override fun onTrackSelectionParametersChanged(parameters: TrackSelectionParameters) {
                Log.d(TAG, "onTrackSelectionParametersChanged overrides=${parameters.overrides.size}")
                pushTrackStateToRust()
                notifyNativeUpdate()
            }

            override fun onVideoSizeChanged(videoSize: VideoSize) {
                Log.d(
                    TAG,
                    "onVideoSizeChanged width=${videoSize.width} height=${videoSize.height} pixelRatio=${videoSize.pixelWidthHeightRatio}",
                )
                currentVideoLayoutState = videoSize.toNativeVideoLayoutInfo()
                notifyNativeUpdate()
            }

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int,
            ) {
                if (reason == Player.DISCONTINUITY_REASON_SEEK) {
                    sessionHandle?.let { handle ->
                        VesperNativeJni.reportSeekCompleted(handle, newPosition.positionMs)
                    }
                }
                Log.d(
                    TAG,
                    "onPositionDiscontinuity reason=$reason positionMs=${newPosition.positionMs}",
                )
                pushSnapshotToRust()
                notifyNativeUpdate()
            }

            override fun onPlayerError(error: PlaybackException) {
                Log.e(TAG, "onPlayerError ${error.errorCodeName}: ${error.message}", error)
                sessionHandle?.let { handle ->
                    VesperNativeJni.reportError(
                        handle,
                        BACKEND_FAILURE_ORDINAL,
                        error.message ?: error.errorCodeName,
                    )
                }
                pushSnapshotToRust()
                notifyNativeUpdate()
            }
        }

    private fun pushSnapshotToRust() {
        val handle = sessionHandle ?: return
        val exoPlayer = player ?: return
        val durationMs = exoPlayer.duration.normalizedDurationMs()
        val isLive = exoPlayer.isCurrentMediaItemLive
        val isSeekable = exoPlayer.isCurrentMediaItemSeekable
        val seekableEndMs = if (isLive && isSeekable && durationMs >= 0L) {
            durationMs
        } else {
            C.TIME_UNSET
        }
        val liveEdgeMs = when {
            !isLive -> C.TIME_UNSET
            seekableEndMs >= 0L -> seekableEndMs
            else -> exoPlayer.currentLiveOffset.normalizedOptionalMs()?.let {
                (exoPlayer.currentPosition.coerceAtLeast(0L) + it).coerceAtLeast(0L)
            } ?: C.TIME_UNSET
        }
        Log.d(
            TAG,
            "pushSnapshotToRust state=${exoPlaybackStateName(exoPlayer.playbackState)} live=$isLive seekable=$isSeekable positionMs=${exoPlayer.currentPosition} durationMs=$durationMs liveEdgeMs=$liveEdgeMs",
        )
        VesperNativeJni.applyExoSnapshot(
            handle,
            exoPlaybackStateOrdinal(exoPlayer.playbackState),
            exoPlayer.playWhenReady,
            exoPlayer.playbackParameters.speed,
            exoPlayer.currentPosition.coerceAtLeast(0L),
            durationMs,
            isLive,
            isSeekable,
            if (seekableEndMs >= 0L) 0L else C.TIME_UNSET,
            seekableEndMs,
            liveEdgeMs,
        )
    }

    private fun pushTrackStateToRust() {
        val handle = sessionHandle ?: return
        val exoPlayer = player ?: return
        val trackCatalog = collectTrackCatalog(exoPlayer.currentTracks)
        val trackSelection =
            collectTrackSelection(exoPlayer.currentTracks, exoPlayer.trackSelectionParameters)
        currentTrackCatalogState = trackCatalog.toPublicTrackCatalog()
        currentTrackSelectionState = trackSelection.toPublicTrackSelectionSnapshot()
        Log.d(
            TAG,
            "pushTrackStateToRust tracks=${trackCatalog.tracks.size} adaptiveVideo=${trackCatalog.adaptiveVideo} adaptiveAudio=${trackCatalog.adaptiveAudio} videoMode=${trackSelection.video.modeOrdinal} audioMode=${trackSelection.audio.modeOrdinal} subtitleMode=${trackSelection.subtitle.modeOrdinal} abrMode=${trackSelection.abrPolicy.modeOrdinal}",
        )
        VesperNativeJni.applyTrackState(handle, trackCatalog, trackSelection)
    }

    private fun notifyNativeUpdate() {
        val listener = updateListener ?: return
        if (Looper.myLooper() == Looper.getMainLooper()) {
            listener.invoke()
        } else {
            mainHandler.post { listener.invoke() }
        }
    }
}

private fun buildLoadControl(source: VesperPlayerSource): DefaultLoadControl {
    val builder = DefaultLoadControl.Builder()
    return when (source.kind) {
        VesperPlayerSourceKind.Local -> builder.build()
        VesperPlayerSourceKind.Remote ->
            when (source.protocol) {
                VesperPlayerSourceProtocol.Hls,
                VesperPlayerSourceProtocol.Dash ->
                    builder
                    .setBufferDurationsMs(
                        /* minBufferMs = */ 20_000,
                        /* maxBufferMs = */ 50_000,
                        /* bufferForPlaybackMs = */ 1_500,
                        /* bufferForPlaybackAfterRebufferMs = */ 3_000,
                    )
                    .build()

                VesperPlayerSourceProtocol.Progressive ->
                    builder
                    .setBufferDurationsMs(
                        /* minBufferMs = */ 12_000,
                        /* maxBufferMs = */ 30_000,
                        /* bufferForPlaybackMs = */ 1_000,
                        /* bufferForPlaybackAfterRebufferMs = */ 2_000,
                    )
                    .build()

                else -> builder.build()
            }
    }
}

private fun VideoSize.toNativeVideoLayoutInfo(): NativeVideoLayoutInfo? {
    if (width <= 0 || height <= 0) {
        return null
    }

    return NativeVideoLayoutInfo(
        width = width,
        height = height,
        pixelWidthHeightRatio = pixelWidthHeightRatio.takeIf { it > 0f } ?: 1.0f,
    )
}

private fun exoPlaybackStateOrdinal(playbackState: Int): Int =
    when (playbackState) {
        Player.STATE_BUFFERING -> 1
        Player.STATE_READY -> 2
        Player.STATE_ENDED -> 3
        else -> 0
    }

private fun VesperTrackSelection.toNativePayload(): NativeTrackSelectionPayload =
    NativeTrackSelectionPayload(
        modeOrdinal =
            when (mode) {
                VesperTrackSelectionMode.Auto -> NativeTrackSelectionMode.Auto.ordinal
                VesperTrackSelectionMode.Disabled -> NativeTrackSelectionMode.Disabled.ordinal
                VesperTrackSelectionMode.Track -> NativeTrackSelectionMode.Track.ordinal
            },
        trackId = trackId,
    )

private fun NativeTrackKind.toPublicKind(): VesperMediaTrackKind =
    when (this) {
        NativeTrackKind.Video -> VesperMediaTrackKind.Video
        NativeTrackKind.Audio -> VesperMediaTrackKind.Audio
        NativeTrackKind.Subtitle -> VesperMediaTrackKind.Subtitle
    }

private fun NativeTrackInfo.toPublicTrack(): VesperMediaTrack? {
    val kind = NativeTrackKind.entries.getOrNull(kindOrdinal)?.toPublicKind() ?: return null
    return VesperMediaTrack(
        id = id,
        kind = kind,
        label = label,
        language = language,
        codec = codec,
        bitRate = bitRate.takeIf { hasBitRate },
        width = width.takeIf { hasWidth },
        height = height.takeIf { hasHeight },
        frameRate = frameRate.takeIf { hasFrameRate },
        channels = channels.takeIf { hasChannels },
        sampleRate = sampleRate.takeIf { hasSampleRate },
        isDefault = isDefault,
        isForced = isForced,
    )
}

private fun NativeTrackCatalog.toPublicTrackCatalog(): VesperTrackCatalog =
    VesperTrackCatalog(
        tracks = tracks.mapNotNull { it.toPublicTrack() },
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
    )

private fun NativeTrackSelectionPayload.toPublicTrackSelection(): VesperTrackSelection {
    val mode = NativeTrackSelectionMode.entries.getOrNull(modeOrdinal) ?: NativeTrackSelectionMode.Auto
    return when (mode) {
        NativeTrackSelectionMode.Auto -> VesperTrackSelection.auto()
        NativeTrackSelectionMode.Disabled -> VesperTrackSelection.disabled()
        NativeTrackSelectionMode.Track -> trackId?.let(VesperTrackSelection::track) ?: VesperTrackSelection.auto()
    }
}

private fun NativeAbrPolicyPayload.toPublicAbrPolicy(): VesperAbrPolicy {
    val mode = NativeAbrMode.entries.getOrNull(modeOrdinal) ?: NativeAbrMode.Auto
    return when (mode) {
        NativeAbrMode.Auto -> VesperAbrPolicy.auto()
        NativeAbrMode.Constrained ->
            VesperAbrPolicy.constrained(
                maxBitRate = maxBitRate.takeIf { hasMaxBitRate },
                maxWidth = maxWidth.takeIf { hasMaxWidth },
                maxHeight = maxHeight.takeIf { hasMaxHeight },
            )
        NativeAbrMode.FixedTrack ->
            trackId?.let(VesperAbrPolicy::fixedTrack) ?: VesperAbrPolicy.auto()
    }
}

private fun NativeTrackSelectionSnapshotPayload.toPublicTrackSelectionSnapshot():
    VesperTrackSelectionSnapshot =
    VesperTrackSelectionSnapshot(
        video = video.toPublicTrackSelection(),
        audio = audio.toPublicTrackSelection(),
        subtitle = subtitle.toPublicTrackSelection(),
        abrPolicy = abrPolicy.toPublicAbrPolicy(),
    )

private fun VesperAbrPolicy.toNativePayload(): NativeAbrPolicyPayload =
    NativeAbrPolicyPayload(
        modeOrdinal =
            when (mode) {
                VesperAbrMode.Auto -> NativeAbrMode.Auto.ordinal
                VesperAbrMode.Constrained -> NativeAbrMode.Constrained.ordinal
                VesperAbrMode.FixedTrack -> NativeAbrMode.FixedTrack.ordinal
            },
        trackId = trackId,
        hasMaxBitRate = maxBitRate != null,
        maxBitRate = maxBitRate ?: 0L,
        hasMaxWidth = maxWidth != null,
        maxWidth = maxWidth ?: 0,
        hasMaxHeight = maxHeight != null,
        maxHeight = maxHeight ?: 0,
    )

private fun applyTrackSelectionCommand(
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
                Log.w(TAG, "failed to find $kind track for id=${selection.trackId}")
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

private fun applyAbrPolicyCommand(
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
                Log.w(TAG, "failed to find fixed ABR video track for id=${policy.trackId}")
                return
            }
            builder.setOverrideForType(override)
        }
        else -> return
    }

    exoPlayer.setTrackSelectionParameters(builder.build())
}

private fun resetAbrConstraints(builder: TrackSelectionParameters.Builder) {
    builder.setForceLowestBitrate(false)
    builder.setForceHighestSupportedBitrate(false)
    builder.setMaxVideoBitrate(Int.MAX_VALUE)
    builder.setMaxVideoSize(Int.MAX_VALUE, Int.MAX_VALUE)
}

private fun findTrackOverride(
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

private fun media3TrackType(kind: NativeTrackKind): Int =
    when (kind) {
        NativeTrackKind.Video -> C.TRACK_TYPE_VIDEO
        NativeTrackKind.Audio -> C.TRACK_TYPE_AUDIO
        NativeTrackKind.Subtitle -> C.TRACK_TYPE_TEXT
    }

private fun Long.clampToIntMax(): Int =
    coerceAtLeast(0L).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

private fun collectTrackCatalog(tracks: Tracks): NativeTrackCatalog {
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

private fun collectTrackSelection(
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): NativeTrackSelectionSnapshotPayload =
    NativeTrackSelectionSnapshotPayload(
        video = collectTrackSelectionForType(C.TRACK_TYPE_VIDEO, tracks, parameters),
        audio = collectTrackSelectionForType(C.TRACK_TYPE_AUDIO, tracks, parameters),
        subtitle = collectTrackSelectionForType(C.TRACK_TYPE_TEXT, tracks, parameters),
        abrPolicy = collectAbrPolicy(tracks, parameters),
    )

private fun collectTrackSelectionForType(
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

private fun collectAbrPolicy(
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

private fun currentOverrideForType(
    trackType: Int,
    tracks: Tracks,
    parameters: TrackSelectionParameters,
): TrackSelectionOverride? =
    parameters.overrides.values.firstOrNull { override ->
        override.type == trackType && currentTracksContainGroup(tracks, override.mediaTrackGroup)
    }

private fun currentSelectedTrackId(trackType: Int, tracks: Tracks): String? {
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

private fun currentTracksContainGroup(tracks: Tracks, trackGroup: TrackGroup): Boolean =
    tracks.groups.any { group -> group.mediaTrackGroup == trackGroup }

private fun nativeTrackKind(trackType: Int): NativeTrackKind? =
    when (trackType) {
        C.TRACK_TYPE_VIDEO -> NativeTrackKind.Video
        C.TRACK_TYPE_AUDIO -> NativeTrackKind.Audio
        C.TRACK_TYPE_TEXT -> NativeTrackKind.Subtitle
        else -> null
    }

private fun nativeTrackId(trackGroup: TrackGroup, trackIndex: Int, format: Format): String {
    val groupId =
        trackGroup.id.takeIf { it.isNotBlank() }
            ?: "type${trackGroup.type}"
    val formatId = format.id?.takeIf { it.isNotBlank() } ?: "track$trackIndex"
    return "$groupId:$formatId:$trackIndex"
}

private fun nativeTrackCodec(format: Format): String? =
    format.codecs ?: format.sampleMimeType ?: format.containerMimeType

private fun buildMediaItem(source: VesperPlayerSource): MediaItem {
    val builder = MediaItem.Builder()
        .setUri(source.uri)

    when (source.protocol) {
        VesperPlayerSourceProtocol.Hls -> builder.setMimeType(MimeTypes.APPLICATION_M3U8)
        VesperPlayerSourceProtocol.Dash -> builder.setMimeType(MimeTypes.APPLICATION_MPD)
        else -> Unit
    }

    return builder.build()
}

private fun Long.normalizedOptionalMs(): Long? =
    if (this == C.TIME_UNSET || this < 0L) {
        null
    } else {
        this
    }

private fun sourceSubtitle(source: VesperPlayerSource): String =
    when (source.kind) {
        VesperPlayerSourceKind.Local -> "Android JNI + ExoPlayer ready (local source)"
        VesperPlayerSourceKind.Remote ->
            "Android JNI + ExoPlayer ready (${source.protocol.name.lowercase()} remote source)"
    }

private fun Long.normalizedDurationMs(): Long =
    if (this == C.TIME_UNSET || this < 0L) {
        -1L
    } else {
        this
    }

private const val BACKEND_FAILURE_ORDINAL = 3
private const val TAG = "VesperPlayerAndroidHost"
private val FORMAT_NO_VALUE_FLOAT = Format.NO_VALUE.toFloat()

private fun exoPlaybackStateName(playbackState: Int): String =
    when (playbackState) {
        Player.STATE_IDLE -> "IDLE"
        Player.STATE_BUFFERING -> "BUFFERING"
        Player.STATE_READY -> "READY"
        Player.STATE_ENDED -> "ENDED"
        else -> "UNKNOWN($playbackState)"
    }
