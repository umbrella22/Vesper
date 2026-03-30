package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
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

    override fun initialize(source: VesperPlayerSource): NativeBridgeStartup {
        Log.i(TAG, "initialize source=${source.uri} kind=${source.kind} protocol=${source.protocol}")
        dispose()
        VesperNativeLibrary.ensureLoaded()

        val handle = VesperNativeJni.createSession(source.uri)
        check(handle != 0L) { "native session handle must not be zero" }
        sessionHandle = handle

        val exoPlayer = ExoPlayer.Builder(appContext).build()
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
    }

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

    private fun dispatchRustCommand(action: (Long) -> Unit) {
        val handle = sessionHandle ?: return
        action(handle)
        drainAndApplyNativeCommands()
        pushSnapshotToRust()
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

    private fun notifyNativeUpdate() {
        val listener = updateListener ?: return
        if (Looper.myLooper() == Looper.getMainLooper()) {
            listener.invoke()
        } else {
            mainHandler.post { listener.invoke() }
        }
    }
}

private fun exoPlaybackStateOrdinal(playbackState: Int): Int =
    when (playbackState) {
        Player.STATE_BUFFERING -> 1
        Player.STATE_READY -> 2
        Player.STATE_ENDED -> 3
        else -> 0
    }

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

private fun exoPlaybackStateName(playbackState: Int): String =
    when (playbackState) {
        Player.STATE_IDLE -> "IDLE"
        Player.STATE_BUFFERING -> "BUFFERING"
        Player.STATE_READY -> "READY"
        Player.STATE_ENDED -> "ENDED"
        else -> "UNKNOWN($playbackState)"
    }
