package io.github.umbrella22.vesper.player.android

import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer

internal fun VesperNativeJniBindings.schedulePlaybackStallObservation(exoPlayer: ExoPlayer, generation: Long) {
    playbackStallRunnable?.let(mainHandler::removeCallbacks)
    val token = playbackDiagnosticsTracker.capture()
    val runnable = object : Runnable {
        override fun run() {
            if (!isCurrentSystemPlaybackCallback(generation) || player !== exoPlayer ||
                !playbackDiagnosticsTracker.isCurrent(token)) return
            val state = exoPlayer.playbackState
            val captured = playbackDiagnosticsTracker.sampleStall(
                token, exoPlayer.currentPosition,
                eligible = exoPlayer.playWhenReady && exoPlayer.playerError == null &&
                    exoPlayer.playbackSuppressionReason == Player.PLAYBACK_SUPPRESSION_REASON_NONE &&
                    pendingSeekCommandId == null && !terminalErrorReportedForCurrentSource &&
                    (state == Player.STATE_READY || state == Player.STATE_BUFFERING),
                buffering = state == Player.STATE_BUFFERING,
            )
            // Publishing the retained observation can synchronously replace/dispose playback.
            if (!isCurrentSystemPlaybackCallback(generation) || player !== exoPlayer ||
                !playbackDiagnosticsTracker.isCurrent(token)) return
            if (captured != null) {
                addLocalBridgeEvent(NativeBridgeEvent.Warning(VesperRuntimeWarning(
                    domain = "playback", payload = captured.toMap(),
                )))
                notifyNativeUpdate()
            }
            if (isCurrentSystemPlaybackCallback(generation) && player === exoPlayer &&
                playbackDiagnosticsTracker.isCurrent(token)) {
                mainHandler.postDelayed(this, VesperPlaybackStallDetector.SAMPLE_INTERVAL_MS)
            }
        }
    }
    playbackStallRunnable = runnable
    mainHandler.postDelayed(runnable, VesperPlaybackStallDetector.SAMPLE_INTERVAL_MS)
}
