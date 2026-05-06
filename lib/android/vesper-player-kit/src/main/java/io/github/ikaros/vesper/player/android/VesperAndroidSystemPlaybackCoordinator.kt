package io.github.ikaros.vesper.player.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Log
import androidx.annotation.OptIn
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionCommands
import androidx.media3.session.SessionResult
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

@OptIn(UnstableApi::class)
class VesperSystemPlaybackService : MediaSessionService() {
    private var registeredSession: MediaSession? = null

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? =
        VesperSystemPlaybackRegistry.activeSession

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val result = super.onStartCommand(intent, flags, startId)
        if (intent?.action == ACTION_START_SYSTEM_PLAYBACK_SERVICE) {
            attachActiveSessionForForegroundPlayback()
        }
        return result
    }

    override fun onDestroy() {
        registeredSession?.let(::removeRegisteredSession)
        registeredSession = null
        super.onDestroy()
    }

    private fun attachActiveSessionForForegroundPlayback() {
        val activeSession = VesperSystemPlaybackRegistry.activeSession
        if (activeSession == null) {
            stopSelf()
            return
        }
        if (!activeSession.player.isPlaying) {
            stopSelf()
            return
        }

        if (registeredSession !== activeSession) {
            registeredSession?.let(::removeRegisteredSession)
            runCatching {
                if (!isSessionAdded(activeSession)) {
                    addSession(activeSession)
                }
                registeredSession = activeSession
            }.onFailure { error ->
                Log.w(TAG, "failed to attach media session to playback service", error)
                stopSelf()
                return
            }
        }

        runCatching {
            triggerNotificationUpdate()
        }.onFailure { error ->
            Log.w(TAG, "failed to promote media playback service", error)
            stopSelf()
        }
    }

    private fun removeRegisteredSession(session: MediaSession) {
        runCatching {
            if (isSessionAdded(session)) {
                removeSession(session)
            }
        }.onFailure { error ->
            Log.w(TAG, "failed to detach media session from playback service", error)
        }
    }
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
    private var serviceSession: MediaSession? = null
    private val sessionId = "vesper-player-system-playback-${System.identityHashCode(this)}"

    fun attachPlayer(player: ExoPlayer?) {
        if (this.player === player) {
            refreshFromPlayer()
            return
        }

        releaseSession()
        this.player = player
        if (player != null && configuration?.shouldExposeSystemControls == true) {
            ensureSession()
            updatePlayerMetadata()
            refreshMediaButtonPreferences()
        }
        refreshFromPlayer()
    }

    fun configure(configuration: VesperSystemPlaybackConfiguration) {
        val shouldRebuildSession =
            this.configuration?.showSeekActions != null &&
                (
                    this.configuration?.showSeekActions != configuration.showSeekActions ||
                        this.configuration?.showSystemControls != configuration.showSystemControls
                )
        this.configuration = configuration
        configuration.metadata?.let { metadata = it }
        if (!configuration.shouldExposeSystemControls) {
            releaseSession()
            stopServiceIfNeeded()
            return
        }

        if (shouldRebuildSession) {
            releaseSession()
        }
        ensureSession()
        updatePlayerMetadata()
        refreshMediaButtonPreferences()
        refreshFromPlayer()
    }

    fun updateMetadata(metadata: VesperSystemPlaybackMetadata) {
        this.metadata = metadata
        updatePlayerMetadata()
        refreshMediaButtonPreferences()
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
        if (!config.shouldExposeSystemControls) {
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
                .setMediaButtonPreferences(buildMediaButtonPreferences())
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
        val activeSession = session
        if (serviceStarted && serviceSession === activeSession) {
            return
        }
        runCatching {
            ContextCompat.startForegroundService(appContext, serviceIntent())
            serviceStarted = true
            serviceSession = activeSession
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
        serviceSession = null
    }

    private fun serviceIntent(): Intent =
        Intent(appContext, VesperSystemPlaybackService::class.java)
            .setAction(ACTION_START_SYSTEM_PLAYBACK_SERVICE)

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

    private fun refreshMediaButtonPreferences() {
        val currentSession = session ?: return
        runCatching {
            currentSession.setMediaButtonPreferences(buildMediaButtonPreferences())
        }.onFailure { error ->
            Log.w(TAG, "failed to update media button preferences", error)
        }
    }

    private fun buildMediaButtonPreferences(): List<CommandButton> {
        val config = configuration ?: return emptyList()
        if (!config.shouldExposeSystemControls) {
            return emptyList()
        }
        val controls = config.controls.normalized(showSeekActions = config.showSeekActions)
        return controls.compactButtons.mapNotNull { button ->
            when (button.kind) {
                VesperSystemPlaybackControlKind.PlayPause -> null
                VesperSystemPlaybackControlKind.SeekBack ->
                    seekButton(
                        button = button,
                        command = SYSTEM_SEEK_BACK_COMMAND,
                        slot = CommandButton.SLOT_BACK,
                        displayPrefix = "Seek back",
                    )
                VesperSystemPlaybackControlKind.SeekForward ->
                    seekButton(
                        button = button,
                        command = SYSTEM_SEEK_FORWARD_COMMAND,
                        slot = CommandButton.SLOT_FORWARD,
                        displayPrefix = "Seek forward",
                    )
            }
        }
    }

    private fun seekButton(
        button: VesperSystemPlaybackControlButton,
        command: SessionCommand,
        slot: Int,
        displayPrefix: String,
    ): CommandButton {
        val offsetMs = button.normalizedSeekOffsetMs
        val seconds = offsetMs / 1000L
        val icon =
            when (button.kind) {
                VesperSystemPlaybackControlKind.SeekBack ->
                    if (offsetMs == DEFAULT_SYSTEM_SEEK_BUTTON_OFFSET_MS) {
                        CommandButton.ICON_SKIP_BACK_10
                    } else {
                        CommandButton.ICON_SKIP_BACK
                    }
                VesperSystemPlaybackControlKind.SeekForward ->
                    if (offsetMs == DEFAULT_SYSTEM_SEEK_BUTTON_OFFSET_MS) {
                        CommandButton.ICON_SKIP_FORWARD_10
                    } else {
                        CommandButton.ICON_SKIP_FORWARD
                    }
                VesperSystemPlaybackControlKind.PlayPause -> CommandButton.ICON_PLAY
            }
        return CommandButton
            .Builder(icon)
            .setSessionCommand(command)
            .setDisplayName("$displayPrefix $seconds seconds")
            .setSlots(slot)
            .build()
    }

    private fun systemPlaybackSessionCallback(): MediaSession.Callback =
        object : MediaSession.Callback {
            override fun onConnect(
                session: MediaSession,
                controllerInfo: MediaSession.ControllerInfo,
            ): MediaSession.ConnectionResult {
                val sessionCommands =
                    MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS
                        .buildUpon()
                        .addSeekCommands(configuration)
                        .build()
                val playerCommands =
                    MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS
                        .buildUpon()
                        .apply {
                            if (configuration?.showSeekActions != true) {
                                removeSeekCommands()
                            }
                        }
                        .build()
                return MediaSession.ConnectionResult
                    .AcceptedResultBuilder(session)
                    .setAvailableSessionCommands(sessionCommands)
                    .setAvailablePlayerCommands(playerCommands)
                    .setMediaButtonPreferences(buildMediaButtonPreferences())
                    .build()
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

            override fun onCustomCommand(
                session: MediaSession,
                controllerInfo: MediaSession.ControllerInfo,
                customCommand: SessionCommand,
                args: Bundle,
            ): ListenableFuture<SessionResult> {
                val result =
                    when (customCommand.customAction) {
                        ACTION_SYSTEM_SEEK_BACK ->
                            handleSystemSeek(
                                VesperSystemPlaybackControlKind.SeekBack,
                                direction = -1L,
                            )
                        ACTION_SYSTEM_SEEK_FORWARD ->
                            handleSystemSeek(
                                VesperSystemPlaybackControlKind.SeekForward,
                                direction = 1L,
                            )
                        else -> SessionResult.RESULT_ERROR_NOT_SUPPORTED
                    }
                return Futures.immediateFuture(SessionResult(result))
            }
        }

    private fun handleSystemSeek(
        kind: VesperSystemPlaybackControlKind,
        direction: Long,
    ): Int {
        val exoPlayer = player ?: return SessionResult.RESULT_ERROR_INVALID_STATE
        val config = configuration ?: return SessionResult.RESULT_ERROR_INVALID_STATE
        if (!config.showSeekActions || !config.shouldExposeSystemControls) {
            return SessionResult.RESULT_ERROR_PERMISSION_DENIED
        }
        val offsetMs =
            config.controls
                .normalized(showSeekActions = config.showSeekActions)
                .seekOffsetMs(kind)
                ?: return SessionResult.RESULT_ERROR_NOT_SUPPORTED
        val currentPosition = exoPlayer.currentPosition.coerceAtLeast(0L)
        val unclampedTarget = currentPosition + (offsetMs * direction)
        val duration = exoPlayer.duration
        val target =
            if (duration != C.TIME_UNSET && duration > 0L) {
                unclampedTarget.coerceIn(0L, duration)
            } else {
                unclampedTarget.coerceAtLeast(0L)
            }
        exoPlayer.seekTo(target)
        return SessionResult.RESULT_SUCCESS
    }
}

private fun SessionCommands.Builder.addSeekCommands(
    configuration: VesperSystemPlaybackConfiguration?,
): SessionCommands.Builder {
    val config = configuration ?: return this
    if (!config.shouldExposeSystemControls) {
        return this
    }
    val controls = config.controls.normalized(showSeekActions = config.showSeekActions)
    if (controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekBack) != null) {
        add(SYSTEM_SEEK_BACK_COMMAND)
    }
    if (controls.seekOffsetMs(VesperSystemPlaybackControlKind.SeekForward) != null) {
        add(SYSTEM_SEEK_FORWARD_COMMAND)
    }
    return this
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

private val VesperSystemPlaybackConfiguration.shouldExposeSystemControls: Boolean
    get() = enabled && showSystemControls

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

private const val ACTION_START_SYSTEM_PLAYBACK_SERVICE =
    "io.github.ikaros.vesper.player.android.action.START_SYSTEM_PLAYBACK_SERVICE"
private const val ACTION_SYSTEM_SEEK_BACK = "io.github.ikaros.vesper.system.seek_back"
private const val ACTION_SYSTEM_SEEK_FORWARD = "io.github.ikaros.vesper.system.seek_forward"
private const val DEFAULT_SYSTEM_SEEK_BUTTON_OFFSET_MS = 10_000L
private val SYSTEM_SEEK_BACK_COMMAND = SessionCommand(ACTION_SYSTEM_SEEK_BACK, Bundle.EMPTY)
private val SYSTEM_SEEK_FORWARD_COMMAND = SessionCommand(ACTION_SYSTEM_SEEK_FORWARD, Bundle.EMPTY)
private const val TAG = "VesperSystemPlayback"
