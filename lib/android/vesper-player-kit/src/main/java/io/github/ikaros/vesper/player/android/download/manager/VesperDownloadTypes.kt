package io.github.ikaros.vesper.player.android

import java.io.File

typealias VesperDownloadAssetId = String
typealias VesperDownloadTaskId = Long

enum class VesperDownloadContentFormat {
    HlsSegments,
    DashSegments,
    FlvSegments,
    SingleFile,
    Unknown,
}

enum class VesperDownloadOutputFormat {
    Mp4,
    Mkv,
    Original,
}

data class VesperDownloadConfiguration(
    val autoStart: Boolean = true,
    val runPostProcessorsOnCompletion: Boolean = true,
    val resumePartialDownloads: Boolean = true,
    val restoreTasksOnStartup: Boolean = true,
    val baseDirectory: File? = null,
    val pluginLibraryPaths: List<String> = emptyList(),
    val rangeChunkBytes: Long? = null,
    val minProgressBytes: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_BYTES,
    val minProgressIntervalMs: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_INTERVAL_MS,
)

enum class VesperDownloadStaleResourcePhase {
    Prepare,
    Download,
}

enum class VesperDownloadPublicCollection {
    Downloads,
    Movies,
}

data class VesperDownloadStaleResource(
    val taskId: VesperDownloadTaskId,
    val resourceId: String? = null,
    val segmentId: String? = null,
    val uri: String? = null,
    val phase: VesperDownloadStaleResourcePhase = VesperDownloadStaleResourcePhase.Prepare,
    val statusCode: Int? = null,
    val receivedBytes: Long = 0L,
    val message: String,
)

data class VesperDownloadRecoveredTaskPlan(
    val source: VesperDownloadSource,
    val profile: VesperDownloadProfile,
    val assetIndex: VesperDownloadAssetIndex,
)

@Deprecated("Use VesperDownloadStaleResourcePlanRecoverer to refresh source, profile, and asset index together.")
interface VesperDownloadStaleResourceRecoverer {
    suspend fun recoverSource(
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource,
    ): VesperDownloadSource?
}

interface VesperDownloadStaleResourcePlanRecoverer {
    suspend fun recoverPlan(
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource,
    ): VesperDownloadRecoveredTaskPlan?
}

@Suppress("DEPRECATION")
internal fun VesperDownloadStaleResourceRecoverer.asPlanRecoverer(): VesperDownloadStaleResourcePlanRecoverer =
    object : VesperDownloadStaleResourcePlanRecoverer {
        override suspend fun recoverPlan(
            task: VesperDownloadTaskSnapshot,
            staleResource: VesperDownloadStaleResource,
        ): VesperDownloadRecoveredTaskPlan? {
            val recoveredSource = recoverSource(task, staleResource) ?: return null
            return VesperDownloadRecoveredTaskPlan(
                source = recoveredSource,
                profile = task.profile,
                assetIndex = VesperDownloadAssetIndex(),
            )
        }
    }

data class VesperDownloadSource(
    val source: VesperPlayerSource,
    val contentFormat: VesperDownloadContentFormat = inferContentFormat(source),
    val manifestUri: String? = null,
) {
    companion object {
        private fun inferContentFormat(source: VesperPlayerSource): VesperDownloadContentFormat =
            when (source.protocol) {
                VesperPlayerSourceProtocol.Hls -> VesperDownloadContentFormat.HlsSegments
                VesperPlayerSourceProtocol.Dash -> VesperDownloadContentFormat.DashSegments
                VesperPlayerSourceProtocol.Progressive,
                VesperPlayerSourceProtocol.File,
                VesperPlayerSourceProtocol.Content,
                -> VesperDownloadContentFormat.SingleFile
                VesperPlayerSourceProtocol.Unknown,
                // Live streaming protocols (RTMP/RTSP/FLV) are continuous
                // streams, not segment-based downloads. The planner treats them
                // as Unknown and rejects download attempts with a capability
                // error rather than silently producing an empty task.
                VesperPlayerSourceProtocol.Rtmp,
                VesperPlayerSourceProtocol.Rtsp,
                VesperPlayerSourceProtocol.Flv,
                -> VesperDownloadContentFormat.Unknown
            }
    }
}

data class VesperDownloadProfile(
    val variantId: String? = null,
    val preferredAudioLanguage: String? = null,
    val preferredSubtitleLanguage: String? = null,
    val selectedTrackIds: List<String> = emptyList(),
    val targetOutputFormat: VesperDownloadOutputFormat? = null,
    val targetDirectory: String? = null,
    val allowMeteredNetwork: Boolean = false,
)

data class VesperDownloadByteRange(
    val offset: Long,
    val length: Long,
)

data class VesperDownloadResourceRecord(
    val resourceId: String,
    val uri: String,
    val relativePath: String? = null,
    val byteRange: VesperDownloadByteRange? = null,
    val generatedText: String? = null,
    val sizeBytes: Long? = null,
    val etag: String? = null,
    val checksum: String? = null,
)

data class VesperDownloadSegmentRecord(
    val segmentId: String,
    val uri: String,
    val relativePath: String? = null,
    val sequence: Long? = null,
    val byteRange: VesperDownloadByteRange? = null,
    val sizeBytes: Long? = null,
    val checksum: String? = null,
)

enum class VesperDownloadStreamKind {
    Combined,
    Video,
    Audio,
    SecondaryAudio,
    Subtitle,
    Auxiliary,
}

data class VesperDownloadAssetStream(
    val streamId: String,
    val kind: VesperDownloadStreamKind = VesperDownloadStreamKind.Combined,
    val language: String? = null,
    val codec: String? = null,
    val label: String? = null,
    val qualityRank: Int? = null,
    val resourceIds: List<String> = emptyList(),
    val segmentIds: List<String> = emptyList(),
    val metadata: Map<String, String> = emptyMap(),
)

data class VesperDownloadAssetIndex(
    val contentFormat: VesperDownloadContentFormat = VesperDownloadContentFormat.Unknown,
    val version: String? = null,
    val etag: String? = null,
    val checksum: String? = null,
    val totalSizeBytes: Long? = null,
    val resources: List<VesperDownloadResourceRecord> = emptyList(),
    val segments: List<VesperDownloadSegmentRecord> = emptyList(),
    val streams: List<VesperDownloadAssetStream> = emptyList(),
    val completedPath: String? = null,
)

data class VesperDownloadProgressSnapshot(
    val receivedBytes: Long = 0L,
    val totalBytes: Long? = null,
    val receivedSegments: Int = 0,
    val totalSegments: Int? = null,
) {
    val completionRatio: Float?
        get() = totalBytes
            ?.takeIf { it > 0L }
            ?.let { receivedBytes.toFloat() / it.toFloat() }
}

enum class VesperDownloadState {
    Queued,
    Preparing,
    Downloading,
    Paused,
    Completed,
    Failed,
    Removed,
}

data class VesperDownloadError(
    val code: VesperPlayerErrorCode,
    val category: VesperPlayerErrorCategory,
    val retriable: Boolean,
    val message: String,
)

data class VesperDownloadTaskSnapshot(
    val taskId: VesperDownloadTaskId,
    val assetId: VesperDownloadAssetId,
    val source: VesperDownloadSource,
    val profile: VesperDownloadProfile,
    val state: VesperDownloadState,
    val progress: VesperDownloadProgressSnapshot,
    val assetIndex: VesperDownloadAssetIndex,
    val error: VesperDownloadError? = null,
)

data class VesperDownloadSnapshot(
    val tasks: List<VesperDownloadTaskSnapshot>,
)

data class VesperDownloadTaskStatePatch(
    val taskId: VesperDownloadTaskId,
    val state: VesperDownloadState,
    val progress: VesperDownloadProgressSnapshot,
    val error: VesperDownloadError? = null,
    val completedPath: String? = null,
)

data class VesperDownloadTaskProgressPatch(
    val taskId: VesperDownloadTaskId,
    val progress: VesperDownloadProgressSnapshot,
)

sealed interface VesperDownloadEvent {
    data class Created(val task: VesperDownloadTaskSnapshot) : VesperDownloadEvent

    data class StateChanged(val patch: VesperDownloadTaskStatePatch) : VesperDownloadEvent

    data class AssetIndexUpdated(val task: VesperDownloadTaskSnapshot) : VesperDownloadEvent

    data class ProgressUpdated(val patch: VesperDownloadTaskProgressPatch) : VesperDownloadEvent
}

internal val VesperDownloadEvent.isRemovedStatePatch: Boolean
    get() = this is VesperDownloadEvent.StateChanged && patch.state == VesperDownloadState.Removed

internal fun vesperDefaultDownloadBaseDirectory(
    filesDir: File,
    configuredBaseDirectory: File?,
): File = configuredBaseDirectory ?: File(filesDir, "vesper-downloads")

interface VesperDownloadExecutionReporter {
    fun completePreparation(
        taskId: VesperDownloadTaskId,
        assetIndex: VesperDownloadAssetIndex,
    )

    fun replaceTaskPlan(
        taskId: VesperDownloadTaskId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex,
    ) = Unit

    fun updateProgress(
        taskId: VesperDownloadTaskId,
        receivedBytes: Long,
        receivedSegments: Int,
    )

    fun complete(
        taskId: VesperDownloadTaskId,
        completedPath: String? = null,
    )

    fun fail(
        taskId: VesperDownloadTaskId,
        error: VesperDownloadError,
    )
}

internal interface NativeDownloadExportProgressCallback {
    fun onProgress(ratio: Float)

    fun isCancelled(): Boolean = false
}

interface VesperDownloadExecutor {
    fun prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        reporter.completePreparation(task.taskId, task.assetIndex)
    }

    fun start(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    )

    fun resume(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) = start(task, reporter)

    fun pause(taskId: VesperDownloadTaskId) = Unit

    fun remove(task: VesperDownloadTaskSnapshot?) = Unit

    fun dispose() = Unit
}
