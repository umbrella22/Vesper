package io.github.ikaros.vesper.player.android

import android.content.Context
import android.net.Uri
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultDataSource
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.Executors
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking

typealias VesperDownloadAssetId = String
typealias VesperDownloadTaskId = Long

enum class VesperDownloadContentFormat {
    HlsSegments,
    DashSegments,
    SingleFile,
    Unknown,
}

data class VesperDownloadConfiguration(
    val autoStart: Boolean = true,
    val baseDirectory: File? = null,
    val pluginLibraryPaths: List<String> = emptyList(),
)

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
                VesperPlayerSourceProtocol.Unknown -> VesperDownloadContentFormat.Unknown
            }
    }
}

data class VesperDownloadProfile(
    val variantId: String? = null,
    val preferredAudioLanguage: String? = null,
    val preferredSubtitleLanguage: String? = null,
    val selectedTrackIds: List<String> = emptyList(),
    val targetDirectory: String? = null,
    val allowMeteredNetwork: Boolean = false,
)

data class VesperDownloadResourceRecord(
    val resourceId: String,
    val uri: String,
    val relativePath: String? = null,
    val sizeBytes: Long? = null,
    val etag: String? = null,
    val checksum: String? = null,
)

data class VesperDownloadSegmentRecord(
    val segmentId: String,
    val uri: String,
    val relativePath: String? = null,
    val sequence: Long? = null,
    val sizeBytes: Long? = null,
    val checksum: String? = null,
)

data class VesperDownloadAssetIndex(
    val contentFormat: VesperDownloadContentFormat = VesperDownloadContentFormat.Unknown,
    val version: String? = null,
    val etag: String? = null,
    val checksum: String? = null,
    val totalSizeBytes: Long? = null,
    val resources: List<VesperDownloadResourceRecord> = emptyList(),
    val segments: List<VesperDownloadSegmentRecord> = emptyList(),
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
    val codeOrdinal: Int,
    val categoryOrdinal: Int,
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

sealed interface VesperDownloadEvent {
    data class Created(val task: VesperDownloadTaskSnapshot) : VesperDownloadEvent

    data class StateChanged(val task: VesperDownloadTaskSnapshot) : VesperDownloadEvent

    data class ProgressUpdated(val task: VesperDownloadTaskSnapshot) : VesperDownloadEvent
}

interface VesperDownloadExecutionReporter {
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

interface VesperDownloadExecutor {
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

class VesperDownloadManager internal constructor(
    configuration: VesperDownloadConfiguration,
    private val executor: VesperDownloadExecutor,
    private val bindings: DownloadBindings,
    private val runtimeDispatcher: CoroutineDispatcher =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "VesperDownloadManagerRuntime").apply { isDaemon = true }
        }.asCoroutineDispatcher(),
) {
    private val runtimeScope = CoroutineScope(SupervisorJob() + runtimeDispatcher)
    private val eventBufferLock = Any()
    private val eventBuffer = mutableListOf<VesperDownloadEvent>()
    private val _snapshot = MutableStateFlow(VesperDownloadSnapshot(emptyList()))
    private var sessionHandle: Long = bindings.createDownloadSession(configuration.toNativePayload())

    val snapshot: StateFlow<VesperDownloadSnapshot> = _snapshot.asStateFlow()

    public constructor(
        context: Context,
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: VesperDownloadExecutor? = null,
    ) : this(
        configuration = configuration,
        executor =
            executor ?: VesperForegroundDownloadExecutor(
                context = context.applicationContext,
                baseDirectory = configuration.baseDirectory,
            ),
        bindings = NativeDownloadBindings,
    )

    init {
        check(sessionHandle != 0L) { "native download session handle must not be zero" }
        refresh()
    }

    fun dispose() {
        executor.dispose()
        if (sessionHandle != 0L) {
            onRuntimeThread {
                bindings.disposeDownloadSession(sessionHandle)
            }
            sessionHandle = 0L
        }
        runtimeScope.cancel()
        (runtimeDispatcher as? AutoCloseable)?.close()
    }

    fun refresh() {
        syncRuntimeState(processCommands = true)
    }

    fun drainEvents(): List<VesperDownloadEvent> =
        synchronized(eventBufferLock) {
            eventBuffer.toList().also { eventBuffer.clear() }
        }

    fun task(taskId: VesperDownloadTaskId): VesperDownloadTaskSnapshot? =
        snapshot.value.tasks.firstOrNull { it.taskId == taskId }

    fun tasksForAsset(assetId: VesperDownloadAssetId): List<VesperDownloadTaskSnapshot> =
        snapshot.value.tasks.filter { it.assetId == assetId }

    fun createTask(
        assetId: VesperDownloadAssetId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile = VesperDownloadProfile(),
        assetIndex: VesperDownloadAssetIndex = VesperDownloadAssetIndex(),
    ): VesperDownloadTaskId? {
        val taskId =
            onRuntimeThread {
                bindings.createDownloadTask(
                    sessionHandle = sessionHandle,
                    assetId = assetId,
                    source = source.toNativePayload(),
                    profile = profile.toNativePayload(),
                    assetIndex = assetIndex.toNativePayload(),
                    nowEpochMs = System.currentTimeMillis(),
                )
            }
        syncRuntimeState(processCommands = true)
        return taskId.takeIf { it != 0L }
    }

    fun startTask(taskId: VesperDownloadTaskId): Boolean {
        val started =
            onRuntimeThread {
                bindings.startDownloadTask(sessionHandle, taskId, System.currentTimeMillis())
            }
        if (started) {
            syncRuntimeState(processCommands = true)
        }
        return started
    }

    fun pauseTask(taskId: VesperDownloadTaskId): Boolean {
        val paused =
            onRuntimeThread {
                bindings.pauseDownloadTask(sessionHandle, taskId, System.currentTimeMillis())
            }
        if (paused) {
            syncRuntimeState(processCommands = true)
        }
        return paused
    }

    fun resumeTask(taskId: VesperDownloadTaskId): Boolean {
        val resumed =
            onRuntimeThread {
                bindings.resumeDownloadTask(sessionHandle, taskId, System.currentTimeMillis())
            }
        if (resumed) {
            syncRuntimeState(processCommands = true)
        }
        return resumed
    }

    fun removeTask(taskId: VesperDownloadTaskId): Boolean {
        val removed =
            onRuntimeThread {
                bindings.removeDownloadTask(sessionHandle, taskId, System.currentTimeMillis())
            }
        if (removed) {
            syncRuntimeState(processCommands = true)
        }
        return removed
    }

    private fun syncRuntimeState(processCommands: Boolean) {
        if (sessionHandle == 0L) {
            _snapshot.value = VesperDownloadSnapshot(emptyList())
            synchronized(eventBufferLock) {
                eventBuffer.clear()
            }
            return
        }

        val nativeSnapshot = onRuntimeThread { bindings.pollDownloadSnapshot(sessionHandle) }
        _snapshot.value = nativeSnapshot?.toPublic() ?: VesperDownloadSnapshot(emptyList())

        val events = onRuntimeThread { bindings.drainDownloadEvents(sessionHandle).toList() }
            .map(NativeDownloadEvent::toPublic)
        if (events.isNotEmpty()) {
            synchronized(eventBufferLock) {
                eventBuffer += events
            }
        }

        if (!processCommands) {
            return
        }

        val commands = onRuntimeThread { bindings.drainDownloadCommands(sessionHandle).toList() }
        commands.forEach(::applyCommand)
    }

    private fun applyCommand(command: NativeDownloadCommand) {
        when (command) {
            is NativeDownloadCommand.Start -> executor.start(command.task.toPublic(), runtimeReporter)
            is NativeDownloadCommand.Resume -> executor.resume(
                command.task.toPublic(),
                runtimeReporter,
            )
            is NativeDownloadCommand.Pause -> executor.pause(command.taskId)
            is NativeDownloadCommand.Remove -> executor.remove(task(command.taskId))
        }
    }

    private fun <T> onRuntimeThread(block: () -> T): T = runBlocking(runtimeDispatcher) { block() }

    private val runtimeReporter =
        object : VesperDownloadExecutionReporter {
            override fun updateProgress(
                taskId: VesperDownloadTaskId,
                receivedBytes: Long,
                receivedSegments: Int,
            ) {
                if (sessionHandle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.updateDownloadTaskProgress(
                        sessionHandle = sessionHandle,
                        taskId = taskId,
                        receivedBytes = receivedBytes,
                        receivedSegments = receivedSegments,
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }

            override fun complete(taskId: VesperDownloadTaskId, completedPath: String?) {
                if (sessionHandle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.completeDownloadTask(
                        sessionHandle = sessionHandle,
                        taskId = taskId,
                        completedPath = completedPath.orEmpty(),
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }

            override fun fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
                if (sessionHandle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.failDownloadTask(
                        sessionHandle = sessionHandle,
                        taskId = taskId,
                        codeOrdinal = error.codeOrdinal,
                        categoryOrdinal = error.categoryOrdinal,
                        retriable = error.retriable,
                        message = error.message,
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }
        }

    internal interface DownloadBindings {
        fun createDownloadSession(config: NativeDownloadConfig): Long

        fun disposeDownloadSession(sessionHandle: Long)

        fun createDownloadTask(
            sessionHandle: Long,
            assetId: String,
            source: NativeDownloadSource,
            profile: NativeDownloadProfile,
            assetIndex: NativeDownloadAssetIndex,
            nowEpochMs: Long,
        ): Long

        fun startDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean

        fun pauseDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean

        fun resumeDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean

        fun updateDownloadTaskProgress(
            sessionHandle: Long,
            taskId: Long,
            receivedBytes: Long,
            receivedSegments: Int,
            nowEpochMs: Long,
        ): Boolean

        fun completeDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            completedPath: String,
            nowEpochMs: Long,
        ): Boolean

        fun failDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            codeOrdinal: Int,
            categoryOrdinal: Int,
            retriable: Boolean,
            message: String,
            nowEpochMs: Long,
        ): Boolean

        fun removeDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean

        fun pollDownloadSnapshot(sessionHandle: Long): NativeDownloadSnapshot?

        fun drainDownloadCommands(sessionHandle: Long): Array<NativeDownloadCommand>

        fun drainDownloadEvents(sessionHandle: Long): Array<NativeDownloadEvent>
    }

    internal object NativeDownloadBindings : DownloadBindings {
        override fun createDownloadSession(config: NativeDownloadConfig): Long =
            VesperNativeJni.createDownloadSession(config)

        override fun disposeDownloadSession(sessionHandle: Long) =
            VesperNativeJni.disposeDownloadSession(sessionHandle)

        override fun createDownloadTask(
            sessionHandle: Long,
            assetId: String,
            source: NativeDownloadSource,
            profile: NativeDownloadProfile,
            assetIndex: NativeDownloadAssetIndex,
            nowEpochMs: Long,
        ): Long =
            VesperNativeJni.createDownloadTask(
                sessionHandle = sessionHandle,
                assetId = assetId,
                source = source,
                profile = profile,
                assetIndex = assetIndex,
                nowEpochMs = nowEpochMs,
            )

        override fun startDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean = VesperNativeJni.startDownloadTask(sessionHandle, taskId, nowEpochMs)

        override fun pauseDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean = VesperNativeJni.pauseDownloadTask(sessionHandle, taskId, nowEpochMs)

        override fun resumeDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean = VesperNativeJni.resumeDownloadTask(sessionHandle, taskId, nowEpochMs)

        override fun updateDownloadTaskProgress(
            sessionHandle: Long,
            taskId: Long,
            receivedBytes: Long,
            receivedSegments: Int,
            nowEpochMs: Long,
        ): Boolean =
            VesperNativeJni.updateDownloadTaskProgress(
                sessionHandle = sessionHandle,
                taskId = taskId,
                receivedBytes = receivedBytes,
                receivedSegments = receivedSegments,
                nowEpochMs = nowEpochMs,
            )

        override fun completeDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            completedPath: String,
            nowEpochMs: Long,
        ): Boolean =
            VesperNativeJni.completeDownloadTask(
                sessionHandle = sessionHandle,
                taskId = taskId,
                completedPath = completedPath,
                nowEpochMs = nowEpochMs,
            )

        override fun failDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            codeOrdinal: Int,
            categoryOrdinal: Int,
            retriable: Boolean,
            message: String,
            nowEpochMs: Long,
        ): Boolean =
            VesperNativeJni.failDownloadTask(
                sessionHandle = sessionHandle,
                taskId = taskId,
                codeOrdinal = codeOrdinal,
                categoryOrdinal = categoryOrdinal,
                retriable = retriable,
                message = message,
                nowEpochMs = nowEpochMs,
            )

        override fun removeDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            nowEpochMs: Long,
        ): Boolean = VesperNativeJni.removeDownloadTask(sessionHandle, taskId, nowEpochMs)

        override fun pollDownloadSnapshot(sessionHandle: Long): NativeDownloadSnapshot? =
            VesperNativeJni.pollDownloadSnapshot(sessionHandle)

        override fun drainDownloadCommands(sessionHandle: Long): Array<NativeDownloadCommand> =
            VesperNativeJni.drainDownloadCommands(sessionHandle)

        override fun drainDownloadEvents(sessionHandle: Long): Array<NativeDownloadEvent> =
            VesperNativeJni.drainDownloadEvents(sessionHandle)
    }
}

private class VesperForegroundDownloadExecutor(
    context: Context,
    private val baseDirectory: File?,
) : VesperDownloadExecutor {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobsLock = Any()
    private val jobs = mutableMapOf<VesperDownloadTaskId, Job>()
    private val dataSourceFactory = DefaultDataSource.Factory(appContext)

    override fun start(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(task, reporter)
    }

    override fun resume(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(task, reporter)
    }

    override fun pause(taskId: VesperDownloadTaskId) {
        synchronized(jobsLock) {
            jobs.remove(taskId)
        }?.cancel()
    }

    override fun remove(task: VesperDownloadTaskSnapshot?) {
        if (task != null) {
            pause(task.taskId)
            val completedPath = task.assetIndex.completedPath?.let(::File)
            when {
                completedPath?.isFile == true -> completedPath.delete()
                completedPath?.isDirectory == true -> completedPath.deleteRecursively()
                task.profile.targetDirectory != null -> File(task.profile.targetDirectory).deleteRecursively()
                else -> resolveDefaultAssetDirectory(task).deleteRecursively()
            }
            return
        }
    }

    override fun dispose() {
        val activeJobs =
            synchronized(jobsLock) {
                jobs.values.toList().also { jobs.clear() }
            }
        activeJobs.forEach(Job::cancel)
        scope.cancel()
    }

    private fun launchDownload(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        pause(task.taskId)
        val job =
            scope.launch {
                try {
                    val plan = buildExecutionPlan(task)
                    var receivedBytes = 0L
                    var receivedSegments = 0
                    val trackSegments = task.assetIndex.segments.isNotEmpty()

                    plan.forEachIndexed { index, entry ->
                        val outputFile = resolveOutputFile(task, entry, index)
                        outputFile.parentFile?.mkdirs()
                        if (outputFile.exists()) {
                            outputFile.delete()
                        }

                        val bytesWritten =
                            copyUriToFile(
                                sourceUri = entry.uri,
                                destination = outputFile,
                            ) { writtenBytes ->
                                val nextBytes = receivedBytes + writtenBytes
                                reporter.updateProgress(task.taskId, nextBytes, receivedSegments)
                            }

                        receivedBytes += bytesWritten
                        if (trackSegments && entry.isSegment) {
                            receivedSegments += 1
                        }
                        reporter.updateProgress(task.taskId, receivedBytes, receivedSegments)
                    }

                    reporter.complete(task.taskId, resolveCompletedPath(task, plan))
                } catch (_: CancellationException) {
                    return@launch
                } catch (error: Exception) {
                    reporter.fail(
                        task.taskId,
                        VesperDownloadError(
                            codeOrdinal = ANDROID_DOWNLOAD_BACKEND_FAILURE_ORDINAL,
                            categoryOrdinal = ANDROID_DOWNLOAD_NETWORK_CATEGORY_ORDINAL,
                            retriable = false,
                            message = error.message ?: "android foreground download failed",
                        ),
                    )
                } finally {
                    synchronized(jobsLock) {
                        jobs.remove(task.taskId)
                    }
                }
            }

        synchronized(jobsLock) {
            jobs[task.taskId] = job
        }
    }

    private fun buildExecutionPlan(task: VesperDownloadTaskSnapshot): List<ForegroundDownloadEntry> {
        val resources =
            task.assetIndex.resources.map { resource ->
                ForegroundDownloadEntry(
                    uri = resource.uri,
                    relativePath = resource.relativePath,
                    fallbackName = resource.resourceId.ifBlank { "resource" },
                    isSegment = false,
                )
            }
        if (resources.isNotEmpty()) {
            return resources
        }

        val segments =
            task.assetIndex.segments.mapIndexed { index, segment ->
                ForegroundDownloadEntry(
                    uri = segment.uri,
                    relativePath = segment.relativePath,
                    fallbackName =
                        segment.segmentId.ifBlank {
                            "segment-${segment.sequence ?: (index + 1).toLong()}"
                        },
                    isSegment = true,
                )
            }
        if (segments.isNotEmpty()) {
            return segments
        }

        val fallbackUri = task.source.manifestUri ?: task.source.source.uri
        return listOf(
            ForegroundDownloadEntry(
                uri = fallbackUri,
                relativePath = null,
                fallbackName = task.assetId.ifBlank { "download-${task.taskId}" },
                isSegment = false,
            ),
        )
    }

    private fun resolveOutputFile(
        task: VesperDownloadTaskSnapshot,
        entry: ForegroundDownloadEntry,
        index: Int,
    ): File {
        val baseDirectory = resolveBaseDirectory(task)
        val relativePath = entry.relativePath?.takeIf { it.isNotBlank() }
        if (relativePath != null) {
            val candidate = File(relativePath)
            return if (candidate.isAbsolute) candidate else File(baseDirectory, relativePath)
        }

        val inferredName =
            Uri.parse(entry.uri).lastPathSegment
                ?.substringAfterLast('/')
                ?.takeIf { it.isNotBlank() }
                ?: "${entry.fallbackName}-${index + 1}.bin"
        return File(baseDirectory, inferredName)
    }

    private fun resolveCompletedPath(
        task: VesperDownloadTaskSnapshot,
        plan: List<ForegroundDownloadEntry>,
    ): String =
        if (plan.size == 1) {
            resolveOutputFile(task, plan.single(), 0).absolutePath
        } else {
            resolveBaseDirectory(task).absolutePath
        }

    private fun resolveBaseDirectory(task: VesperDownloadTaskSnapshot): File =
        task.profile.targetDirectory
            ?.takeIf { it.isNotBlank() }
            ?.let(::File)
            ?: resolveDefaultAssetDirectory(task)

    private fun resolveDefaultAssetDirectory(task: VesperDownloadTaskSnapshot): File =
        File(
            baseDirectory ?: File(appContext.filesDir, "vesper-downloads"),
            task.assetId.ifBlank { task.taskId.toString() },
        )

    private fun copyUriToFile(
        sourceUri: String,
        destination: File,
        onProgress: (Long) -> Unit,
    ): Long {
        val dataSource = dataSourceFactory.createDataSource()
        val dataSpec = DataSpec.Builder().setUri(sourceUri).build()
        var totalWritten = 0L
        var reportedBytes = 0L
        FileOutputStream(destination).use { output ->
            try {
                dataSource.open(dataSpec)
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val read = dataSource.read(buffer, 0, buffer.size)
                    if (read == -1) {
                        break
                    }
                    output.write(buffer, 0, read)
                    totalWritten += read.toLong()
                    if (totalWritten - reportedBytes >= ANDROID_DOWNLOAD_PROGRESS_STEP_BYTES) {
                        reportedBytes = totalWritten
                        onProgress(totalWritten)
                    }
                }
            } finally {
                runCatching { dataSource.close() }
            }
        }
        return totalWritten
    }
}

private data class ForegroundDownloadEntry(
    val uri: String,
    val relativePath: String?,
    val fallbackName: String,
    val isSegment: Boolean,
)

private fun VesperDownloadConfiguration.toNativePayload(): NativeDownloadConfig =
    NativeDownloadConfig(
        autoStart = autoStart,
        pluginLibraryPaths = pluginLibraryPaths.toTypedArray(),
    )

private fun VesperDownloadSource.toNativePayload(): NativeDownloadSource =
    NativeDownloadSource(
        sourceUri = source.uri,
        contentFormatOrdinal = contentFormat.ordinal,
        manifestUri = manifestUri,
    )

private fun VesperDownloadProfile.toNativePayload(): NativeDownloadProfile =
    NativeDownloadProfile(
        variantId = variantId,
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectedTrackIds = selectedTrackIds.toTypedArray(),
        targetDirectory = targetDirectory,
        allowMeteredNetwork = allowMeteredNetwork,
    )

private fun VesperDownloadAssetIndex.toNativePayload(): NativeDownloadAssetIndex =
    NativeDownloadAssetIndex(
        contentFormatOrdinal = contentFormat.ordinal,
        version = version,
        etag = etag,
        checksum = checksum,
        hasTotalSizeBytes = totalSizeBytes != null,
        totalSizeBytes = totalSizeBytes ?: 0L,
        resources = resources.map(VesperDownloadResourceRecord::toNativePayload).toTypedArray(),
        segments = segments.map(VesperDownloadSegmentRecord::toNativePayload).toTypedArray(),
        completedPath = completedPath,
    )

private fun VesperDownloadResourceRecord.toNativePayload(): NativeDownloadResourceRecord =
    NativeDownloadResourceRecord(
        resourceId = resourceId,
        uri = uri,
        relativePath = relativePath,
        hasSizeBytes = sizeBytes != null,
        sizeBytes = sizeBytes ?: 0L,
        etag = etag,
        checksum = checksum,
    )

private fun VesperDownloadSegmentRecord.toNativePayload(): NativeDownloadSegmentRecord =
    NativeDownloadSegmentRecord(
        segmentId = segmentId,
        uri = uri,
        relativePath = relativePath,
        hasSequence = sequence != null,
        sequence = sequence ?: 0L,
        hasSizeBytes = sizeBytes != null,
        sizeBytes = sizeBytes ?: 0L,
        checksum = checksum,
    )

private fun NativeDownloadSnapshot.toPublic(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(tasks = tasks.map(NativeDownloadTask::toPublic))

private fun NativeDownloadTask.toPublic(): VesperDownloadTaskSnapshot =
    VesperDownloadTaskSnapshot(
        taskId = taskId,
        assetId = assetId,
        source = source.toPublic(),
        profile = profile.toPublic(),
        state =
            when (statusOrdinal) {
                0 -> VesperDownloadState.Queued
                1 -> VesperDownloadState.Preparing
                2 -> VesperDownloadState.Downloading
                3 -> VesperDownloadState.Paused
                4 -> VesperDownloadState.Completed
                5 -> VesperDownloadState.Failed
                6 -> VesperDownloadState.Removed
                else -> VesperDownloadState.Queued
            },
        progress = progress.toPublic(),
        assetIndex = assetIndex.toPublic(),
        error =
            if (hasError) {
                VesperDownloadError(
                    codeOrdinal = errorCodeOrdinal,
                    categoryOrdinal = errorCategoryOrdinal,
                    retriable = errorRetriable,
                    message = errorMessage ?: "download failed",
                )
            } else {
                null
            },
    )

private fun NativeDownloadSource.toPublic(): VesperDownloadSource =
    VesperDownloadSource(
        source =
            when {
                sourceUri.startsWith("content://", ignoreCase = true) ||
                    sourceUri.startsWith("file://", ignoreCase = true) -> {
                    VesperPlayerSource.local(
                        uri = sourceUri,
                        label = Uri.parse(sourceUri).lastPathSegment ?: sourceUri,
                    )
                }
                else -> {
                    VesperPlayerSource.remote(uri = sourceUri, label = sourceUri)
                }
            },
        contentFormat =
            when (contentFormatOrdinal) {
                0 -> VesperDownloadContentFormat.HlsSegments
                1 -> VesperDownloadContentFormat.DashSegments
                2 -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        manifestUri = manifestUri,
    )

private fun NativeDownloadProfile.toPublic(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = variantId,
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectedTrackIds = selectedTrackIds.toList(),
        targetDirectory = targetDirectory,
        allowMeteredNetwork = allowMeteredNetwork,
    )

private fun NativeDownloadAssetIndex.toPublic(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat =
            when (contentFormatOrdinal) {
                0 -> VesperDownloadContentFormat.HlsSegments
                1 -> VesperDownloadContentFormat.DashSegments
                2 -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        version = version,
        etag = etag,
        checksum = checksum,
        totalSizeBytes = if (hasTotalSizeBytes) totalSizeBytes else null,
        resources = resources.map(NativeDownloadResourceRecord::toPublic),
        segments = segments.map(NativeDownloadSegmentRecord::toPublic),
        completedPath = completedPath,
    )

private fun NativeDownloadResourceRecord.toPublic(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = resourceId,
        uri = uri,
        relativePath = relativePath,
        sizeBytes = if (hasSizeBytes) sizeBytes else null,
        etag = etag,
        checksum = checksum,
    )

private fun NativeDownloadSegmentRecord.toPublic(): VesperDownloadSegmentRecord =
    VesperDownloadSegmentRecord(
        segmentId = segmentId,
        uri = uri,
        relativePath = relativePath,
        sequence = if (hasSequence) sequence else null,
        sizeBytes = if (hasSizeBytes) sizeBytes else null,
        checksum = checksum,
    )

private fun NativeDownloadProgress.toPublic(): VesperDownloadProgressSnapshot =
    VesperDownloadProgressSnapshot(
        receivedBytes = receivedBytes,
        totalBytes = if (hasTotalBytes) totalBytes else null,
        receivedSegments = receivedSegments,
        totalSegments = if (hasTotalSegments) totalSegments else null,
    )

private fun NativeDownloadEvent.toPublic(): VesperDownloadEvent =
    when (this) {
        is NativeDownloadEvent.Created -> VesperDownloadEvent.Created(task.toPublic())
        is NativeDownloadEvent.StateChanged -> VesperDownloadEvent.StateChanged(task.toPublic())
        is NativeDownloadEvent.ProgressUpdated -> VesperDownloadEvent.ProgressUpdated(task.toPublic())
    }

private const val ANDROID_DOWNLOAD_BACKEND_FAILURE_ORDINAL = 3
private const val ANDROID_DOWNLOAD_NETWORK_CATEGORY_ORDINAL = 2
private const val ANDROID_DOWNLOAD_PROGRESS_STEP_BYTES = 64L * 1024L
