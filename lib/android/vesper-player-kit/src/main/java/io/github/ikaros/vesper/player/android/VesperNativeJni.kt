package io.github.ikaros.vesper.player.android

import android.view.Surface

object VesperNativeJni {
    init {
        VesperNativeLibrary.ensureLoaded()
    }

    external fun createSession(sourceUri: String): Long
    external fun createPreloadSession(preloadBudget: NativeResolvedPreloadBudgetPolicy): Long
    external fun createDownloadSession(config: NativeDownloadConfig): Long
    external fun createPlaylistSession(
        config: NativePlaylistConfig,
        preloadBudget: NativeResolvedPreloadBudgetPolicy,
    ): Long
    external fun resolveResiliencePolicy(
        sourceKindOrdinal: Int,
        sourceProtocolOrdinal: Int,
        bufferingPolicy: NativeBufferingPolicy,
        retryPolicy: NativeRetryPolicy,
        cachePolicy: NativeCachePolicy,
    ): NativeResolvedResiliencePolicy
    external fun resolvePreloadBudget(preloadBudget: NativePreloadBudget): NativeResolvedPreloadBudgetPolicy
    external fun resolveTrackPreferences(
        trackPreferences: NativeTrackPreferencePolicy,
    ): NativeTrackPreferencePolicy
    external fun disposeSession(sessionHandle: Long)
    external fun disposePreloadSession(sessionHandle: Long)
    external fun disposeDownloadSession(sessionHandle: Long)
    external fun disposePlaylistSession(sessionHandle: Long)
    external fun attachSurface(
        sessionHandle: Long,
        surface: Surface,
        surfaceKindOrdinal: Int,
    )
    external fun detachSurface(sessionHandle: Long)
    external fun pollSnapshot(sessionHandle: Long): NativeBridgeSnapshot?
    external fun drainEvents(sessionHandle: Long): Array<NativeBridgeEvent>
    external fun drainNativeCommands(sessionHandle: Long): Array<NativePlayerCommand>
    external fun planPreloadCandidates(
        sessionHandle: Long,
        candidates: Array<NativePreloadCandidate>,
        nowEpochMs: Long,
    ): Array<Long>
    external fun drainPreloadCommands(sessionHandle: Long): Array<NativePreloadCommand>
    external fun createDownloadTask(
        sessionHandle: Long,
        assetId: String,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Long
    external fun startDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean
    external fun pauseDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean
    external fun resumeDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean
    external fun updateDownloadTaskProgress(
        sessionHandle: Long,
        taskId: Long,
        receivedBytes: Long,
        receivedSegments: Int,
        nowEpochMs: Long,
    ): Boolean
    external fun completeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        completedPath: String,
        nowEpochMs: Long,
    ): Boolean
    external fun failDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
        nowEpochMs: Long,
    ): Boolean
    external fun removeDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean
    external fun pollDownloadSnapshot(sessionHandle: Long): NativeDownloadSnapshot?
    external fun drainDownloadCommands(sessionHandle: Long): Array<NativeDownloadCommand>
    external fun drainDownloadEvents(sessionHandle: Long): Array<NativeDownloadEvent>
    external fun drainPlaylistPreloadCommands(sessionHandle: Long): Array<NativePreloadCommand>
    external fun completePreloadTask(sessionHandle: Long, taskId: Long): Boolean
    external fun completePlaylistPreloadTask(sessionHandle: Long, taskId: Long): Boolean
    external fun failPreloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
    ): Boolean
    external fun failPlaylistPreloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
    ): Boolean
    external fun replacePlaylistQueue(
        sessionHandle: Long,
        queue: Array<NativePlaylistQueueItem>,
        nowEpochMs: Long,
    ): Boolean
    external fun updatePlaylistViewportHints(
        sessionHandle: Long,
        hints: Array<NativePlaylistViewportHint>,
        nowEpochMs: Long,
    ): Boolean
    external fun clearPlaylistViewportHints(sessionHandle: Long, nowEpochMs: Long): Boolean
    external fun advancePlaylistToNext(sessionHandle: Long, nowEpochMs: Long): Boolean
    external fun advancePlaylistToPrevious(sessionHandle: Long, nowEpochMs: Long): Boolean
    external fun handlePlaylistPlaybackCompleted(sessionHandle: Long, nowEpochMs: Long): Boolean
    external fun handlePlaylistPlaybackFailed(sessionHandle: Long, nowEpochMs: Long): Boolean
    external fun currentPlaylistActiveItem(sessionHandle: Long): NativePlaylistActiveItem?
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
