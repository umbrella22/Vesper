package io.github.ikaros.vesper.player.android

import kotlin.jvm.JvmField

internal enum class NativeVideoSurfaceKind {
    TextureView,
    SurfaceView,
}

internal enum class NativeTrackKind {
    Video,
    Audio,
    Subtitle,
}

internal enum class NativeTrackSelectionMode {
    Auto,
    Disabled,
    Track,
}

internal enum class NativeAbrMode {
    Auto,
    Constrained,
    FixedTrack,
}

internal enum class NativeErrorCategory {
    Input,
    Source,
    Network,
    Decode,
    AudioOutput,
    Playback,
    Capability,
    Platform,
}

internal data class NativeBridgeStartup(
    val subtitle: String? = null,
    val pluginDiagnostics: List<Map<String, Any?>> = emptyList(),
)

internal data class NativeBridgeSnapshot(
    val playbackState: PlaybackStateUi,
    val playbackRate: Float,
    val isBuffering: Boolean,
    val isInterrupted: Boolean,
    val timeline: TimelineUiState,
)

internal class NativeBufferingPolicy(
    @JvmField val presetOrdinal: Int,
    @JvmField val hasMinBufferMs: Boolean,
    @JvmField val minBufferMs: Int,
    @JvmField val hasMaxBufferMs: Boolean,
    @JvmField val maxBufferMs: Int,
    @JvmField val hasBufferForPlaybackMs: Boolean,
    @JvmField val bufferForPlaybackMs: Int,
    @JvmField val hasBufferForPlaybackAfterRebufferMs: Boolean,
    @JvmField val bufferForPlaybackAfterRebufferMs: Int,
)

internal class NativeRetryPolicy(
    @JvmField val usesDefaultMaxAttempts: Boolean,
    @JvmField val hasMaxAttempts: Boolean,
    @JvmField val maxAttempts: Int,
    @JvmField val hasBaseDelayMs: Boolean,
    @JvmField val baseDelayMs: Long,
    @JvmField val hasMaxDelayMs: Boolean,
    @JvmField val maxDelayMs: Long,
    @JvmField val hasBackoff: Boolean,
    @JvmField val backoffOrdinal: Int,
)

internal class NativeCachePolicy(
    @JvmField val presetOrdinal: Int,
    @JvmField val hasMaxMemoryBytes: Boolean,
    @JvmField val maxMemoryBytes: Long,
    @JvmField val hasMaxDiskBytes: Boolean,
    @JvmField val maxDiskBytes: Long,
)

internal class NativeResolvedResiliencePolicy(
    @JvmField val buffering: NativeBufferingPolicy,
    @JvmField val retry: NativeRetryPolicy,
    @JvmField val cache: NativeCachePolicy,
)

internal class NativeResolvedPreloadBudgetPolicy(
    @JvmField val maxConcurrentTasks: Int,
    @JvmField val maxMemoryBytes: Long,
    @JvmField val maxDiskBytes: Long,
    @JvmField val warmupWindowMs: Long,
)

internal class NativeDownloadConfig(
    @JvmField val autoStart: Boolean,
    @JvmField val runPostProcessorsOnCompletion: Boolean,
    @JvmField val pluginLibraryPaths: Array<String> = emptyArray(),
)

internal class NativePlaylistConfig(
    @JvmField val playlistId: String,
    @JvmField val neighborPrevious: Int,
    @JvmField val neighborNext: Int,
    @JvmField val preloadNearVisible: Int,
    @JvmField val preloadPrefetchOnly: Int,
    @JvmField val autoAdvance: Boolean,
    @JvmField val repeatModeOrdinal: Int,
    @JvmField val failureStrategyOrdinal: Int,
)

internal class NativeTrackPreferencePolicy(
    @JvmField val preferredAudioLanguage: String?,
    @JvmField val preferredSubtitleLanguage: String?,
    @JvmField val selectSubtitlesByDefault: Boolean,
    @JvmField val selectUndeterminedSubtitleLanguage: Boolean,
    @JvmField val audioSelection: NativeTrackSelectionPayload,
    @JvmField val subtitleSelection: NativeTrackSelectionPayload,
    @JvmField val abrPolicy: NativeAbrPolicyPayload,
)

internal class NativeTrackInfo(
    @JvmField val id: String,
    @JvmField val kindOrdinal: Int,
    @JvmField val label: String?,
    @JvmField val language: String?,
    @JvmField val codec: String?,
    @JvmField val hasBitRate: Boolean,
    @JvmField val bitRate: Long,
    @JvmField val hasWidth: Boolean,
    @JvmField val width: Int,
    @JvmField val hasHeight: Boolean,
    @JvmField val height: Int,
    @JvmField val hasFrameRate: Boolean,
    @JvmField val frameRate: Float,
    @JvmField val hasChannels: Boolean,
    @JvmField val channels: Int,
    @JvmField val hasSampleRate: Boolean,
    @JvmField val sampleRate: Int,
    @JvmField val isDefault: Boolean,
    @JvmField val isForced: Boolean,
)

internal class NativeTrackCatalog(
    @JvmField val tracks: Array<NativeTrackInfo>,
    @JvmField val adaptiveVideo: Boolean,
    @JvmField val adaptiveAudio: Boolean,
)

internal class NativeTrackSelectionPayload(
    @JvmField val modeOrdinal: Int,
    @JvmField val trackId: String?,
)

internal class NativeAbrPolicyPayload(
    @JvmField val modeOrdinal: Int,
    @JvmField val trackId: String?,
    @JvmField val hasMaxBitRate: Boolean,
    @JvmField val maxBitRate: Long,
    @JvmField val hasMaxWidth: Boolean,
    @JvmField val maxWidth: Int,
    @JvmField val hasMaxHeight: Boolean,
    @JvmField val maxHeight: Int,
)

internal class NativeTrackSelectionSnapshotPayload(
    @JvmField val video: NativeTrackSelectionPayload,
    @JvmField val audio: NativeTrackSelectionPayload,
    @JvmField val subtitle: NativeTrackSelectionPayload,
    @JvmField val abrPolicy: NativeAbrPolicyPayload,
)

internal class NativePreloadBudget(
    @JvmField val hasMaxConcurrentTasks: Boolean,
    @JvmField val maxConcurrentTasks: Int,
    @JvmField val hasMaxMemoryBytes: Boolean,
    @JvmField val maxMemoryBytes: Long,
    @JvmField val hasMaxDiskBytes: Boolean,
    @JvmField val maxDiskBytes: Long,
    @JvmField val hasWarmupWindowMs: Boolean,
    @JvmField val warmupWindowMs: Long,
)

internal class NativePreloadCandidate(
    @JvmField val sourceUri: String,
    @JvmField val scopeKindOrdinal: Int,
    @JvmField val scopeId: String?,
    @JvmField val kindOrdinal: Int,
    @JvmField val selectionHintOrdinal: Int,
    @JvmField val priorityOrdinal: Int,
    @JvmField val expectedMemoryBytes: Long,
    @JvmField val expectedDiskBytes: Long,
    @JvmField val hasTtlMs: Boolean,
    @JvmField val ttlMs: Long,
    @JvmField val hasWarmupWindowMs: Boolean,
    @JvmField val warmupWindowMs: Long,
)

internal class NativePlaylistQueueItem(
    @JvmField val itemId: String,
    @JvmField val sourceUri: String,
    @JvmField val expectedMemoryBytes: Long,
    @JvmField val expectedDiskBytes: Long,
    @JvmField val hasTtlMs: Boolean,
    @JvmField val ttlMs: Long,
    @JvmField val hasWarmupWindowMs: Boolean,
    @JvmField val warmupWindowMs: Long,
)

internal class NativePlaylistViewportHint(
    @JvmField val itemId: String,
    @JvmField val kindOrdinal: Int,
    @JvmField val order: Int,
)

internal class NativePlaylistActiveItem(
    @JvmField val itemId: String,
    @JvmField val index: Int,
)

internal class NativeDownloadSource(
    @JvmField val sourceUri: String,
    @JvmField val contentFormatOrdinal: Int,
    @JvmField val manifestUri: String?,
    @JvmField val headerNames: Array<String>,
    @JvmField val headerValues: Array<String>,
)

internal class NativeDownloadProfile(
    @JvmField val variantId: String?,
    @JvmField val preferredAudioLanguage: String?,
    @JvmField val preferredSubtitleLanguage: String?,
    @JvmField val selectedTrackIds: Array<String>,
    @JvmField val targetOutputFormatOrdinal: Int,
    @JvmField val targetDirectory: String?,
    @JvmField val allowMeteredNetwork: Boolean,
)

internal class NativeDownloadByteRange(
    @JvmField val offset: Long,
    @JvmField val length: Long,
)

internal class NativeDownloadResourceRecord(
    @JvmField val resourceId: String,
    @JvmField val uri: String,
    @JvmField val relativePath: String?,
    @JvmField val byteRange: NativeDownloadByteRange?,
    @JvmField val generatedText: String?,
    @JvmField val hasSizeBytes: Boolean,
    @JvmField val sizeBytes: Long,
    @JvmField val etag: String?,
    @JvmField val checksum: String?,
)

internal class NativeDownloadSegmentRecord(
    @JvmField val segmentId: String,
    @JvmField val uri: String,
    @JvmField val relativePath: String?,
    @JvmField val hasSequence: Boolean,
    @JvmField val sequence: Long,
    @JvmField val byteRange: NativeDownloadByteRange?,
    @JvmField val hasSizeBytes: Boolean,
    @JvmField val sizeBytes: Long,
    @JvmField val checksum: String?,
)

internal class NativeDownloadAssetStream(
    @JvmField val streamId: String,
    @JvmField val kindOrdinal: Int,
    @JvmField val language: String?,
    @JvmField val codec: String?,
    @JvmField val label: String?,
    @JvmField val hasQualityRank: Boolean,
    @JvmField val qualityRank: Int,
    @JvmField val resourceIds: Array<String>,
    @JvmField val segmentIds: Array<String>,
    @JvmField val metadataKeys: Array<String>,
    @JvmField val metadataValues: Array<String>,
)

internal class NativeDownloadAssetIndex(
    @JvmField val contentFormatOrdinal: Int,
    @JvmField val version: String?,
    @JvmField val etag: String?,
    @JvmField val checksum: String?,
    @JvmField val hasTotalSizeBytes: Boolean,
    @JvmField val totalSizeBytes: Long,
    @JvmField val resources: Array<NativeDownloadResourceRecord>,
    @JvmField val segments: Array<NativeDownloadSegmentRecord>,
    @JvmField val streams: Array<NativeDownloadAssetStream>,
    @JvmField val completedPath: String?,
)

internal class NativeDownloadProgress(
    @JvmField val receivedBytes: Long,
    @JvmField val hasTotalBytes: Boolean,
    @JvmField val totalBytes: Long,
    @JvmField val receivedSegments: Int,
    @JvmField val hasTotalSegments: Boolean,
    @JvmField val totalSegments: Int,
)

internal class NativeDownloadTask(
    @JvmField val taskId: Long,
    @JvmField val assetId: String,
    @JvmField val source: NativeDownloadSource,
    @JvmField val profile: NativeDownloadProfile,
    @JvmField val statusOrdinal: Int,
    @JvmField val progress: NativeDownloadProgress,
    @JvmField val assetIndex: NativeDownloadAssetIndex,
    @JvmField val hasError: Boolean,
    @JvmField val errorCodeOrdinal: Int,
    @JvmField val errorCategoryOrdinal: Int,
    @JvmField val errorRetriable: Boolean,
    @JvmField val errorMessage: String?,
)

internal class NativeDownloadSnapshot(
    @JvmField val tasks: Array<NativeDownloadTask>,
)

internal class NativePreloadTask(
    @JvmField val taskId: Long,
    @JvmField val sourceUri: String,
    @JvmField val sourceIdentity: String,
    @JvmField val cacheKey: String,
    @JvmField val scopeKindOrdinal: Int,
    @JvmField val scopeId: String?,
    @JvmField val kindOrdinal: Int,
    @JvmField val selectionHintOrdinal: Int,
    @JvmField val priorityOrdinal: Int,
    @JvmField val expectedMemoryBytes: Long,
    @JvmField val expectedDiskBytes: Long,
    @JvmField val warmupWindowMs: Long,
    @JvmField val hasExpiresInMs: Boolean,
    @JvmField val expiresInMs: Long,
    @JvmField val statusOrdinal: Int,
    @JvmField val errorCodeOrdinal: Int,
    @JvmField val errorMessage: String?,
)

internal data class NativeVideoLayoutInfo(
    val width: Int,
    val height: Int,
    val pixelWidthHeightRatio: Float = 1.0f,
)

internal sealed interface NativeBridgeEvent {
    data class PlaybackStateChanged(val state: PlaybackStateUi) : NativeBridgeEvent
    data class PlaybackRateChanged(val rate: Float) : NativeBridgeEvent
    data class BufferingChanged(val isBuffering: Boolean) : NativeBridgeEvent
    data class InterruptionChanged(val isInterrupted: Boolean) : NativeBridgeEvent
    data class VideoSurfaceChanged(val attached: Boolean) : NativeBridgeEvent
    data class SeekCompleted(val positionMs: Long) : NativeBridgeEvent
    data class RetryScheduled(val attempt: Int, val delayMs: Long) : NativeBridgeEvent
    data class Ended(val ended: Boolean = true) : NativeBridgeEvent
    data class Error(
        val message: String,
        val codeOrdinal: Int,
        val categoryOrdinal: Int,
        val retriable: Boolean,
    ) : NativeBridgeEvent
}

internal sealed interface NativePlayerCommand {
    data object Play : NativePlayerCommand
    data object Pause : NativePlayerCommand
    data class SeekTo(val positionMs: Long) : NativePlayerCommand
    data object Stop : NativePlayerCommand
    data class SetPlaybackRate(val rate: Float) : NativePlayerCommand
    data class SetVideoTrackSelection(val selection: NativeTrackSelectionPayload) : NativePlayerCommand
    data class SetAudioTrackSelection(val selection: NativeTrackSelectionPayload) : NativePlayerCommand
    data class SetSubtitleTrackSelection(val selection: NativeTrackSelectionPayload) : NativePlayerCommand
    data class SetAbrPolicy(val policy: NativeAbrPolicyPayload) : NativePlayerCommand
}

internal sealed interface NativePreloadCommand {
    data class Start(val task: NativePreloadTask) : NativePreloadCommand
    data class Cancel(val taskId: Long) : NativePreloadCommand
}

internal sealed interface NativeDownloadCommand {
    data class Prepare(val task: NativeDownloadTask) : NativeDownloadCommand
    data class Start(val task: NativeDownloadTask) : NativeDownloadCommand
    data class Pause(val taskId: Long) : NativeDownloadCommand
    data class Resume(val task: NativeDownloadTask) : NativeDownloadCommand
    data class Remove(val taskId: Long) : NativeDownloadCommand
}

internal sealed interface NativeDownloadEvent {
    data class Created(val task: NativeDownloadTask) : NativeDownloadEvent
    data class StateChanged(
        val taskId: Long,
        val statusOrdinal: Int,
        val progress: NativeDownloadProgress,
        val hasError: Boolean,
        val errorCodeOrdinal: Int,
        val errorCategoryOrdinal: Int,
        val errorRetriable: Boolean,
        val errorMessage: String?,
        val completedPath: String?,
    ) : NativeDownloadEvent
    data class AssetIndexUpdated(val task: NativeDownloadTask) : NativeDownloadEvent
    data class ProgressUpdated(
        val taskId: Long,
        val progress: NativeDownloadProgress,
    ) : NativeDownloadEvent
}
