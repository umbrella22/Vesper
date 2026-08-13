package io.github.umbrella22.vesper.player.android

import android.view.Surface

internal object VesperNativeJni {
    init {
        VesperNativeLibrary.ensureLoaded()
    }

    external fun createSession(
        sourceUri: String,
        pipelineEventHookConfig: NativePipelineEventHookConfig,
    ): Long
    external fun createPreloadSession(preloadBudget: NativeResolvedPreloadBudgetPolicy): Long
    external fun createDownloadSession(config: NativeDownloadConfig): Long
    external fun createEmbeddedPluginRegistry(
        registryFragments: Array<String>,
        referencesJson: String,
        nativeLibraryDir: String,
        packagePaths: Array<String>,
        runtimeApiLevel: Int,
    ): Long
    external fun disposeEmbeddedPluginRegistry(registryHandle: Long)
    external fun createBenchmarkSinkSessionFromRegistry(
        registryHandle: Long,
        referencesJson: String,
    ): Long
    external fun probeMobilePlugins(
        sourceUri: String,
        sourceModeOrdinal: Int,
        sourcePluginArtifactsJson: String,
        runtimeProfile: String?,
        frameModeOrdinal: Int,
        framePluginArtifactsJson: String,
    ): String
    external fun openSourceNormalizerResource(
        sourceUri: String,
        sourceModeOrdinal: Int,
        sourcePluginArtifactsJson: String,
        runtimeProfile: String?,
        outputRoot: String,
        forceNormalized: Boolean,
    ): String?
    external fun pollSourceNormalizerResource(sessionHandle: Long): String?
    external fun disposeSourceNormalizerResource(sessionHandle: Long)
    external fun openNativeFramePipeline(
        sourceUri: String,
        sourceModeOrdinal: Int,
        sourcePluginArtifactsJson: String,
        runtimeProfile: String?,
        nativeFrameMode: String,
        decoderPluginArtifactsJson: String,
        avcDecoderImplementationName: String?,
        hevcDecoderImplementationName: String?,
        frameProcessorPluginArtifactsJson: String,
        maxInFlightFrames: Int,
        presenterProfile: String,
    ): String?
    external fun advanceNativeFramePipeline(sessionHandle: Long): String?
    external fun releaseNativeFramePipelineFrame(
        sessionHandle: Long,
        frameHandle: Long,
        presented: Boolean,
    ): String?
    external fun attachNativeFramePipelineSurface(
        sessionHandle: Long,
        surface: Surface,
        surfaceKind: String,
    ): String?
    external fun detachNativeFramePipelineSurface(sessionHandle: Long): String?
    external fun flushNativeFramePipeline(sessionHandle: Long): String?
    external fun seekNativeFramePipeline(sessionHandle: Long, positionMs: Long): String?
    external fun closeNativeFramePipeline(sessionHandle: Long)
    external fun createPlaylistSession(
        config: NativePlaylistConfig,
        preloadBudget: NativeResolvedPreloadBudgetPolicy,
    ): Long
    external fun createSequenceSession(configJson: String): Long
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
    external fun disposeBenchmarkSinkSession(sessionHandle: Long)
    external fun disposePlaylistSession(sessionHandle: Long)
    external fun disposeSequenceSession(sessionHandle: Long)
    external fun executeSequenceCommand(
        sessionHandle: Long,
        commandJson: String,
        wallEpochMs: Long,
    ): String
    external fun sequenceSnapshot(sessionHandle: Long): String
    external fun drainSequenceEvents(sessionHandle: Long, maxCount: Int): String
    external fun sequencePreloadIntents(sessionHandle: Long, wallEpochMs: Long): String
    external fun submitBenchmarkSinkEvents(sessionHandle: Long, batchJson: String): String
    external fun flushBenchmarkSinkSession(sessionHandle: Long): String
    external fun attachSurface(
        sessionHandle: Long,
        surface: Surface,
        surfaceKindOrdinal: Int,
    )
    external fun detachSurface(sessionHandle: Long)
    external fun pollSnapshot(sessionHandle: Long): NativeBridgeSnapshot?
    external fun sampleTimeline(
        sessionHandle: Long,
        positionMs: Long,
        durationMs: Long,
        isLive: Boolean,
        isSeekable: Boolean,
        seekableStartMs: Long,
        seekableEndMs: Long,
        liveEdgeMs: Long,
    ): TimelineUiState?
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
    external fun restoreDownloadTasks(
        sessionHandle: Long,
        tasks: Array<NativeDownloadTask>,
        nowEpochMs: Long,
    ): Boolean
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
    external fun completeDownloadPreparation(
        sessionHandle: Long,
        taskId: Long,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean
    external fun replaceDownloadTaskPlan(
        sessionHandle: Long,
        taskId: Long,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean
    external fun exportDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        outputPath: String,
        progressCallback: NativeDownloadExportProgressCallback?,
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
    external fun drainDownloadEvents(sessionHandle: Long): NativeDownloadEventBatch
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
    external fun reportFirstFrame(
        sessionHandle: Long,
        presentationTimeMs: Long,
        width: Int,
        height: Int,
    )
    external fun flushPipelineEventHooks(sessionHandle: Long, timeoutMs: Long): Boolean
    external fun closePipelineEventHooks(sessionHandle: Long): Boolean
    external fun drainPipelineEventHookReports(sessionHandle: Long): String?
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
    ): String?
    external fun setAbrPolicy(
        sessionHandle: Long,
        policy: NativeAbrPolicyPayload,
        expectedCatalogRevision: Long?,
    ): String?
}
