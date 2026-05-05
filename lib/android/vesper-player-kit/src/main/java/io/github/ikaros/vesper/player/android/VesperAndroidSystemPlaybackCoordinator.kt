package io.github.ikaros.vesper.player.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.SessionResult

class VesperSystemPlaybackService : MediaSessionService() {
    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? =
        VesperSystemPlaybackRegistry.activeSession
}

internal class VesperAndroidSystemPlaybackCoordinator(
    context: Context,
) {
    private val appContext = context.applicationContext
    private var configuration: VesperSystemPlaybackConfiguration? = null
    private var metadata: VesperSystemPlaybackMetadata? = null
    private var player: ExoPlayer? = null
    private var session: MediaSession? = null
    private var serviceStarted = false
    private val sessionId = "vesper-player-system-playback-${System.identityHashCode(this)}"

    fun attachPlayer(player: ExoPlayer?) {
        if (this.player === player) {
            refreshFromPlayer()
            return
        }

        releaseSession()
        this.player = player
        if (player != null && configuration?.enabled == true) {
            ensureSession()
            updatePlayerMetadata()
        }
        refreshFromPlayer()
    }

    fun configure(configuration: VesperSystemPlaybackConfiguration) {
        val shouldRebuildSession =
            this.configuration?.showSeekActions != null &&
                this.configuration?.showSeekActions != configuration.showSeekActions
        this.configuration = configuration
        configuration.metadata?.let { metadata = it }
        if (!configuration.enabled) {
            releaseSession()
            stopServiceIfNeeded()
            return
        }

        if (shouldRebuildSession) {
            releaseSession()
        }
        ensureSession()
        updatePlayerMetadata()
        refreshFromPlayer()
    }

    fun updateMetadata(metadata: VesperSystemPlaybackMetadata) {
        this.metadata = metadata
        updatePlayerMetadata()
        refreshFromPlayer()
    }

    fun clear() {
        configuration = null
        metadata = null
        releaseSession()
        stopServiceIfNeeded()
    }

    fun refreshFromPlayer() {
        val config = configuration ?: return stopServiceIfNeeded()
        if (!config.enabled) {
            stopServiceIfNeeded()
            return
        }

        if (player != null && session == null) {
            ensureSession()
        }

        val shouldRunService =
            config.backgroundMode == VesperBackgroundPlaybackMode.ContinueAudio &&
                player?.isPlaying == true
        if (shouldRunService) {
            startServiceIfNeeded()
        } else {
            stopServiceIfNeeded()
        }
    }

    private fun ensureSession() {
        val exoPlayer = player ?: return
        if (session != null) {
            VesperSystemPlaybackRegistry.claim(session)
            return
        }

        session =
            MediaSession.Builder(appContext, exoPlayer)
                .setId(sessionId)
                .setCallback(systemPlaybackSessionCallback())
                .build()
                .also(VesperSystemPlaybackRegistry::claim)
    }

    private fun releaseSession() {
        session?.let { currentSession ->
            VesperSystemPlaybackRegistry.release(currentSession)
            currentSession.release()
        }
        session = null
    }

    private fun startServiceIfNeeded() {
        if (serviceStarted) {
            return
        }
        runCatching {
            ContextCompat.startForegroundService(appContext, serviceIntent())
            serviceStarted = true
        }.onFailure { error ->
            Log.w(TAG, "failed to start media playback service", error)
        }
    }

    private fun stopServiceIfNeeded() {
        if (!serviceStarted) {
            return
        }
        runCatching {
            appContext.stopService(serviceIntent())
        }.onFailure { error ->
            Log.w(TAG, "failed to stop media playback service", error)
        }
        serviceStarted = false
    }

    private fun serviceIntent(): Intent =
        Intent(appContext, VesperSystemPlaybackService::class.java)

    private fun updatePlayerMetadata() {
        val exoPlayer = player ?: return
        val mediaMetadata = metadata?.toMediaMetadata() ?: return
        val currentItem = exoPlayer.currentMediaItem ?: return
        val mediaItem =
            currentItem
                .buildUpon()
                .setMediaMetadata(mediaMetadata)
                .build()
        replaceCurrentMediaItem(exoPlayer, mediaItem)
    }

    private fun replaceCurrentMediaItem(exoPlayer: ExoPlayer, mediaItem: MediaItem) {
        if (exoPlayer.mediaItemCount <= 0) {
            return
        }
        val index = exoPlayer.currentMediaItemIndex.coerceIn(0, exoPlayer.mediaItemCount - 1)
        runCatching {
            exoPlayer.replaceMediaItem(index, mediaItem)
        }.onFailure { error ->
            Log.w(TAG, "failed to update media session metadata", error)
        }
    }

    private fun systemPlaybackSessionCallback(): MediaSession.Callback =
        object : MediaSession.Callback {
            override fun onConnect(
                session: MediaSession,
                controllerInfo: MediaSession.ControllerInfo,
            ): MediaSession.ConnectionResult {
                val playerCommands =
                    MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS
                        .buildUpon()
                        .apply {
                            if (configuration?.showSeekActions != true) {
                                removeSeekCommands()
                            }
                        }
                        .build()
                return MediaSession.ConnectionResult.accept(
                    MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS,
                    playerCommands,
                )
            }

            @Suppress("OVERRIDE_DEPRECATION")
            override fun onPlayerCommandRequest(
                session: MediaSession,
                controllerInfo: MediaSession.ControllerInfo,
                playerCommand: Int,
            ): Int {
                if (configuration?.showSeekActions != true && playerCommand.isSeekCommand()) {
                    return SessionResult.RESULT_ERROR_PERMISSION_DENIED
                }
                return SessionResult.RESULT_SUCCESS
            }
        }
}

private fun Player.Commands.Builder.removeSeekCommands() {
    removeAll(
        Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
        Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS,
        Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_NEXT,
        Player.COMMAND_SEEK_TO_MEDIA_ITEM,
        Player.COMMAND_SEEK_BACK,
        Player.COMMAND_SEEK_FORWARD,
    )
}

private fun Int.isSeekCommand(): Boolean =
    when (this) {
        Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
        Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS,
        Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_NEXT,
        Player.COMMAND_SEEK_TO_MEDIA_ITEM,
        Player.COMMAND_SEEK_BACK,
        Player.COMMAND_SEEK_FORWARD,
        -> true
        else -> false
    }

private object VesperSystemPlaybackRegistry {
    @Volatile
    var activeSession: MediaSession? = null
        private set

    fun claim(session: MediaSession?) {
        activeSession = session
    }

    fun release(session: MediaSession) {
        if (activeSession === session) {
            activeSession = null
        }
    }
}

private fun VesperSystemPlaybackMetadata.toMediaMetadata(): MediaMetadata {
    val extras =
        Bundle().apply {
            putBoolean("io.github.ikaros.vesper.player.IS_LIVE", isLive)
            durationMs?.let { putLong("io.github.ikaros.vesper.player.DURATION_MS", it) }
            contentUri?.let { putString("io.github.ikaros.vesper.player.CONTENT_URI", it) }
        }

    val builder =
        MediaMetadata.Builder()
            .setTitle(title)
            .setDisplayTitle(title)
            .setIsPlayable(true)
            .setExtras(extras)

    artist?.let(builder::setArtist)
    albumTitle?.let(builder::setAlbumTitle)
    artworkUri?.let { uri ->
        runCatching { Uri.parse(uri) }
            .getOrNull()
            ?.let(builder::setArtworkUri)
    }

    return builder.build()
}

private const val TAG = "VesperSystemPlayback"
