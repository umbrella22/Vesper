package io.github.ikaros.vesper.player.android

import android.view.Surface

object VesperNativeJni {
    external fun createSession(sourceUri: String): Long
    external fun disposeSession(sessionHandle: Long)
    external fun attachSurface(
        sessionHandle: Long,
        surface: Surface,
        surfaceKindOrdinal: Int,
    )
    external fun detachSurface(sessionHandle: Long)
    external fun pollSnapshot(sessionHandle: Long): NativeBridgeSnapshot?
    external fun drainEvents(sessionHandle: Long): Array<NativeBridgeEvent>
    external fun drainNativeCommands(sessionHandle: Long): Array<NativePlayerCommand>
    external fun applyExoSnapshot(
        sessionHandle: Long,
        playbackStateOrdinal: Int,
        playWhenReady: Boolean,
        playbackRate: Float,
        positionMs: Long,
        durationMs: Long,
        isLive: Boolean,
        isSeekable: Boolean,
        seekableStartMs: Long,
        seekableEndMs: Long,
        liveEdgeMs: Long,
    )
    external fun applyTrackState(
        sessionHandle: Long,
        trackCatalog: NativeTrackCatalog,
        trackSelection: NativeTrackSelectionSnapshotPayload,
    )
    external fun reportSeekCompleted(sessionHandle: Long, positionMs: Long)
    external fun reportRetryScheduled(sessionHandle: Long, attempt: Int, delayMs: Long)
    external fun reportError(
        sessionHandle: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
    )
    external fun play(sessionHandle: Long)
    external fun pause(sessionHandle: Long)
    external fun stop(sessionHandle: Long)
    external fun seekTo(sessionHandle: Long, positionMs: Long)
    external fun setPlaybackRate(sessionHandle: Long, rate: Float)
    external fun setVideoTrackSelection(
        sessionHandle: Long,
        selection: NativeTrackSelectionPayload,
    )
    external fun setAudioTrackSelection(
        sessionHandle: Long,
        selection: NativeTrackSelectionPayload,
    )
    external fun setSubtitleTrackSelection(
        sessionHandle: Long,
        selection: NativeTrackSelectionPayload,
    )
    external fun setAbrPolicy(sessionHandle: Long, policy: NativeAbrPolicyPayload)
}
