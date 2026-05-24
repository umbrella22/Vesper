package io.github.ikaros.vesper.player.android

import android.content.Context
import android.net.Uri
import java.io.File
import java.util.concurrent.Executors
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext

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
private fun VesperDownloadStaleResourceRecoverer.asPlanRecoverer(): VesperDownloadStaleResourcePlanRecoverer =
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
                VesperPlayerSourceProtocol.Unknown -> VesperDownloadContentFormat.Unknown
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

private val VesperDownloadEvent.isRemovedStatePatch: Boolean
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

class VesperDownloadManager internal constructor(
    private val configuration: VesperDownloadConfiguration,
    private val executor: VesperDownloadExecutor,
    private val bindings: DownloadBindings,
    private val stateStore: VesperDownloadStatePersistence? = null,
    private val defaultBaseDirectory: File? = configuration.baseDirectory,
    private val runtimeDispatcher: CoroutineDispatcher =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "VesperDownloadManagerRuntime").apply { isDaemon = true }
        }.asCoroutineDispatcher(),
) {
    private val runtimeScope = CoroutineScope(SupervisorJob() + runtimeDispatcher)
    private val eventBufferLock = Any()
    private val eventBuffer = mutableListOf<VesperDownloadEvent>()
    private val taskStore = DownloadTaskStore()
    private val lastProgressPersistence = mutableMapOf<VesperDownloadTaskId, ProgressPersistenceCheckpoint>()
    private val _snapshot = MutableStateFlow(VesperDownloadSnapshot(emptyList()))
    @Volatile
    private var sessionHandle: Long = bindings.createDownloadSession(configuration.toNativePayload())

    val snapshot: StateFlow<VesperDownloadSnapshot> = _snapshot.asStateFlow()

    @Suppress("DEPRECATION")
    public constructor(
        context: Context,
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: VesperDownloadExecutor? = null,
        staleResourceRecoverer: VesperDownloadStaleResourceRecoverer? = null,
        staleResourcePlanRecoverer: VesperDownloadStaleResourcePlanRecoverer? = null,
    ) : this(
        configuration = configuration,
        executor =
            executor ?: VesperForegroundDownloadExecutor(
                context = context.applicationContext,
                baseDirectory = configuration.baseDirectory,
                resumePartialDownloads = configuration.resumePartialDownloads,
                rangeChunkBytes = configuration.rangeChunkBytes,
                minProgressBytes = configuration.minProgressBytes,
                minProgressIntervalMs = configuration.minProgressIntervalMs,
                staleResourcePlanRecoverer =
                    staleResourcePlanRecoverer ?: staleResourceRecoverer?.asPlanRecoverer(),
            ),
        bindings = NativeDownloadBindings,
        stateStore =
            configuration
                .takeIf { it.restoreTasksOnStartup }
                ?.let {
                    VesperDownloadStateStore(
                        File(
                            vesperDefaultDownloadBaseDirectory(context.applicationContext.filesDir, it.baseDirectory),
                            "download-state.json",
                        ),
                    )
                },
        defaultBaseDirectory = vesperDefaultDownloadBaseDirectory(
            context.applicationContext.filesDir,
            configuration.baseDirectory,
        ),
    )

    fun dispose() {
        snapshot.value.tasks
            .filter {
                it.state == VesperDownloadState.Preparing ||
                    it.state == VesperDownloadState.Downloading
            }
            .forEach { pauseTask(it.taskId) }
        persistSnapshot(snapshot.value)
        executor.dispose()
        if (sessionHandle != 0L) {
            onRuntimeThread {
                bindings.disposeDownloadSession(sessionHandle)
            }
            sessionHandle = 0L
        }
        runtimeScope.cancel()
        (runtimeDispatcher as? AutoCloseable)?.close()
        taskStore.replaceAll(VesperDownloadSnapshot(emptyList()))
        lastProgressPersistence.clear()
    }

    fun refresh() {
        syncRuntimeState(processCommands = true)
    }

    fun forceFullSync() {
        forceFullSync(processCommands = true)
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
        val normalizedAssetIndex =
            runCatching {
                generatedResourceMaterializer().materialize(
                    assetId = assetId,
                    taskId = null,
                    profile = profile,
                    assetIndex = assetIndex,
                )
            }.getOrElse {
                return null
            }
        val taskId =
            onRuntimeThread {
                bindings.createDownloadTask(
                    sessionHandle = sessionHandle,
                    assetId = assetId,
                    source = source.toNativePayload(),
                    profile = profile.toNativePayload(),
                    assetIndex = normalizedAssetIndex.toNativePayload(),
                    nowEpochMs = System.currentTimeMillis(),
                )
            }
        syncRuntimeState(processCommands = true)
        return taskId.takeIf { it != 0L }
    }

    fun restoreTasks(tasks: List<VesperDownloadTaskSnapshot>): Boolean {
        if (tasks.isEmpty()) {
            return true
        }
        val normalizedTasks =
            runCatching {
                tasks.map { task ->
                    task.copy(
                        assetIndex =
                            generatedResourceMaterializer().materialize(
                                assetId = task.assetId,
                                taskId = task.taskId,
                                profile = task.profile,
                                assetIndex = task.assetIndex,
                            ),
                    )
                }
            }.getOrElse {
                return false
            }
        val restored =
            onRuntimeThread {
                bindings.restoreDownloadTasks(
                    sessionHandle = sessionHandle,
                    tasks = normalizedTasks.map(VesperDownloadTaskSnapshot::toNativePayload).toTypedArray(),
                    nowEpochMs = System.currentTimeMillis(),
                )
            }
        if (restored) {
            forceFullSync(processCommands = true)
        }
        return restored
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

    suspend fun exportTaskOutput(
        taskId: VesperDownloadTaskId,
        outputPath: String,
        onProgress: (Float) -> Unit = {},
        isCancelled: () -> Boolean = { false },
    ) {
        check(sessionHandle != 0L) { "native download session handle must not be zero" }
        withContext(runtimeDispatcher) {
            val exported =
                bindings.exportDownloadTask(
                    sessionHandle = sessionHandle,
                    taskId = taskId,
                    outputPath = outputPath,
                    progressCallback =
                        object : NativeDownloadExportProgressCallback {
                            override fun onProgress(ratio: Float) {
                                onProgress(ratio.coerceIn(0f, 1f))
                            }

                            override fun isCancelled(): Boolean = isCancelled()
                        },
                )
            check(exported) { "download export failed for task $taskId" }
        }
    }

    fun shareTaskOutput(
        context: Context,
        taskId: VesperDownloadTaskId,
        fileName: String? = null,
        mimeType: String? = null,
        authority: String = "${context.packageName}.vesper.player.fileprovider",
    ) {
        shareDownloadTaskOutput(
            context = context,
            source = outputFileForTask(taskId),
            fileName = fileName,
            mimeType = mimeType,
            authority = authority,
        )
    }

    fun saveTaskOutput(
        context: Context,
        taskId: VesperDownloadTaskId,
        fileName: String? = null,
        collection: VesperDownloadPublicCollection = VesperDownloadPublicCollection.Downloads,
    ): Uri = saveDownloadTaskOutput(
        context = context,
        source = outputFileForTask(taskId),
        fileName = fileName,
        collection = collection,
    )

    private fun syncRuntimeState(processCommands: Boolean) {
        if (sessionHandle == 0L) {
            taskStore.replaceAll(VesperDownloadSnapshot(emptyList()))
            _snapshot.value = VesperDownloadSnapshot(emptyList())
            lastProgressPersistence.clear()
            synchronized(eventBufferLock) {
                eventBuffer.clear()
            }
            return
        }

        val events = onRuntimeThread { bindings.drainDownloadEvents(sessionHandle).toList() }
            .map(NativeDownloadEvent::toPublic)
        if (events.isNotEmpty()) {
            synchronized(eventBufferLock) {
                eventBuffer += events
            }
            val immediateEvents = events.filterNot { it.isRemovedStatePatch }
            if (immediateEvents.isNotEmpty()) {
                val updatedSnapshot = taskStore.apply(immediateEvents)
                _snapshot.value = updatedSnapshot
            }
        }

        if (processCommands) {
            val commands = onRuntimeThread { bindings.drainDownloadCommands(sessionHandle).toList() }
            commands.forEach(::applyCommand)
        }

        if (events.isNotEmpty()) {
            val removalEvents = events.filter { it.isRemovedStatePatch }
            if (removalEvents.isNotEmpty()) {
                val updatedSnapshot = taskStore.apply(removalEvents)
                _snapshot.value = updatedSnapshot
            }
            if (shouldPersistSnapshot(events)) {
                persistSnapshot(_snapshot.value)
            }
        }
    }

    private fun forceFullSync(processCommands: Boolean) {
        if (sessionHandle == 0L) {
            taskStore.replaceAll(VesperDownloadSnapshot(emptyList()))
            _snapshot.value = VesperDownloadSnapshot(emptyList())
            lastProgressPersistence.clear()
            synchronized(eventBufferLock) {
                eventBuffer.clear()
            }
            return
        }

        val fullSnapshot =
            onRuntimeThread { bindings.pollDownloadSnapshot(sessionHandle) }?.toPublic()
                ?: VesperDownloadSnapshot(emptyList())
        taskStore.replaceAll(fullSnapshot)
        val activeSnapshot = taskStore.snapshot()
        _snapshot.value = activeSnapshot
        persistSnapshot(activeSnapshot)
        syncRuntimeState(processCommands = processCommands)
    }

    private fun shouldPersistSnapshot(events: List<VesperDownloadEvent>): Boolean {
        var shouldPersist = false
        events.forEach { event ->
            when (event) {
                is VesperDownloadEvent.Created,
                is VesperDownloadEvent.AssetIndexUpdated -> shouldPersist = true
                is VesperDownloadEvent.StateChanged -> {
                    shouldPersist = true
                    lastProgressPersistence[event.patch.taskId] =
                        ProgressPersistenceCheckpoint(
                            bytes = event.patch.progress.receivedBytes,
                            epochMs = System.currentTimeMillis(),
                        )
                }
                is VesperDownloadEvent.ProgressUpdated -> {
                    if (shouldPersistProgressCheckpoint(event.patch)) {
                        shouldPersist = true
                    }
                }
            }
        }
        return shouldPersist
    }

    private fun shouldPersistProgressCheckpoint(patch: VesperDownloadTaskProgressPatch): Boolean {
        val now = System.currentTimeMillis()
        val previous = lastProgressPersistence[patch.taskId]
        if (previous == null) {
            lastProgressPersistence[patch.taskId] =
                ProgressPersistenceCheckpoint(patch.progress.receivedBytes, now)
            return true
        }
        val byteDelta =
            if (patch.progress.receivedBytes >= previous.bytes) {
                patch.progress.receivedBytes - previous.bytes
            } else {
                0L
            }
        val elapsedMs = now - previous.epochMs
        if (byteDelta < configuration.minProgressBytes ||
            elapsedMs < configuration.minProgressIntervalMs
        ) {
            return false
        }
        lastProgressPersistence[patch.taskId] =
            ProgressPersistenceCheckpoint(patch.progress.receivedBytes, now)
        return true
    }

    private fun applyCommand(command: NativeDownloadCommand) {
        when (command) {
            is NativeDownloadCommand.Prepare -> executor.prepare(command.task.toPublic(), runtimeReporter)
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

    private fun restorePersistedTasks() {
        val storedTasks = stateStore?.load()?.tasks.orEmpty()
        if (storedTasks.isEmpty()) {
            return
        }
        val restorable = storedTasks.filter { it.state != VesperDownloadState.Removed }
        if (restorable.isEmpty()) {
            return
        }
        val activeTaskIds =
            restorable
                .filter {
                    it.state == VesperDownloadState.Preparing ||
                        it.state == VesperDownloadState.Downloading
                }
                .map { it.taskId }
        val queuedTaskIds =
            restorable
                .filter { it.state == VesperDownloadState.Queued }
                .map { it.taskId }
        val normalizedRestorable =
            runCatching {
                restorable.map { task ->
                    task.copy(
                        assetIndex =
                            generatedResourceMaterializer().materialize(
                                assetId = task.assetId,
                                taskId = task.taskId,
                                profile = task.profile,
                                assetIndex = task.assetIndex,
                            ),
                    )
                }
            }.getOrElse {
                return
            }
        val restored =
            onRuntimeThread {
                bindings.restoreDownloadTasks(
                    sessionHandle = sessionHandle,
                    tasks = normalizedRestorable.map(VesperDownloadTaskSnapshot::toNativePayload).toTypedArray(),
                    nowEpochMs = System.currentTimeMillis(),
                )
            }
        if (!restored) {
            return
        }
        forceFullSync(processCommands = true)
        if (!configuration.autoStart) {
            return
        }
        activeTaskIds.forEach { taskId ->
            onRuntimeThread {
                bindings.resumeDownloadTask(sessionHandle, taskId, System.currentTimeMillis())
            }
        }
        queuedTaskIds.forEach { taskId ->
            onRuntimeThread {
                bindings.startDownloadTask(sessionHandle, taskId, System.currentTimeMillis())
            }
        }
    }

    private fun persistSnapshot(snapshot: VesperDownloadSnapshot) {
        stateStore?.save(snapshot.compactedForPersistence())
    }

    private fun generatedResourceMaterializer(): VesperGeneratedDownloadResourceMaterializer =
        VesperGeneratedDownloadResourceMaterializer(
            baseDirectory = configuration.baseDirectory,
            fallbackBaseDirectory = defaultBaseDirectory,
        )

    private fun outputFileForTask(taskId: VesperDownloadTaskId): File {
        val task = task(taskId)
            ?: error("download task $taskId was not found")
        check(task.state == VesperDownloadState.Completed) {
            "download task $taskId must be completed before sharing or saving"
        }
        val completedPath = task.assetIndex.completedPath
        check(!completedPath.isNullOrBlank()) {
            "download task $taskId does not have an output file"
        }
        val uri = Uri.parse(completedPath)
        val file =
            if (uri?.scheme.equals("file", ignoreCase = true)) {
                File(checkNotNull(uri?.path) { "download task output file URI is invalid" })
            } else {
                File(completedPath)
            }
        check(file.isFile) {
            "download task output file does not exist: ${file.absolutePath}"
        }
        return file
    }

    private fun preparedShareFile(
        context: Context,
        source: File,
        fileName: String?,
    ): File {
        val safeFileName = fileName?.takeIf { it.isNotBlank() }?.let(::sanitizedOutputFileName)
            ?: source.name
        if (safeFileName == source.name && source.absolutePath.startsWith(context.filesDir.absolutePath)) {
            return source
        }
        val directory = File(context.cacheDir, "vesper-download-share")
        directory.mkdirs()
        val target = File(directory, safeFileName)
        if (target.absolutePath != source.absolutePath) {
            source.copyTo(target, overwrite = true)
        }
        return target
    }

    private val runtimeReporter =
        object : VesperDownloadExecutionReporter {
            override fun completePreparation(
                taskId: VesperDownloadTaskId,
                assetIndex: VesperDownloadAssetIndex,
            ) {
                if (sessionHandle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.completeDownloadPreparation(
                        sessionHandle = sessionHandle,
                        taskId = taskId,
                        assetIndex = assetIndex.toNativePayload(),
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = true)
            }

            override fun replaceTaskPlan(
                taskId: VesperDownloadTaskId,
                source: VesperDownloadSource,
                profile: VesperDownloadProfile,
                assetIndex: VesperDownloadAssetIndex,
            ) {
                if (sessionHandle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.replaceDownloadTaskPlan(
                        sessionHandle = sessionHandle,
                        taskId = taskId,
                        source = source.toNativePayload(),
                        profile = profile.toNativePayload(),
                        assetIndex = assetIndex.toNativePayload(),
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }

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
                        codeOrdinal = error.code.jniOrdinal,
                        categoryOrdinal = error.category.jniOrdinal,
                        retriable = error.retriable,
                        message = error.message,
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }
        }

    init {
        check(sessionHandle != 0L) { "native download session handle must not be zero" }
        restorePersistedTasks()
        forceFullSync()
    }

}
