package io.github.ikaros.vesper.player.android

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultDataSource
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.StringReader
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import javax.xml.parsers.DocumentBuilderFactory
import java.util.concurrent.Executors
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import org.w3c.dom.Element
import org.xml.sax.InputSource

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

private data class ProgressPersistenceCheckpoint(
    val bytes: Long,
    val epochMs: Long,
)

private class DownloadTaskStore {
    private val tasksById = linkedMapOf<VesperDownloadTaskId, VesperDownloadTaskSnapshot>()

    fun replaceAll(snapshot: VesperDownloadSnapshot) {
        tasksById.clear()
        snapshot.tasks.filter { it.state != VesperDownloadState.Removed }.forEach { task ->
            tasksById[task.taskId] = task
        }
    }

    fun apply(events: List<VesperDownloadEvent>): VesperDownloadSnapshot {
        events.forEach { event ->
            when (event) {
                is VesperDownloadEvent.Created -> tasksById[event.task.taskId] = event.task
                is VesperDownloadEvent.AssetIndexUpdated -> tasksById[event.task.taskId] = event.task
                is VesperDownloadEvent.StateChanged -> {
                    if (event.patch.state == VesperDownloadState.Removed) {
                        tasksById.remove(event.patch.taskId)
                        return@forEach
                    }
                    val task = tasksById[event.patch.taskId] ?: return@forEach
                    tasksById[event.patch.taskId] =
                        task.copy(
                            state = event.patch.state,
                            progress = event.patch.progress,
                            assetIndex =
                                task.assetIndex.copy(
                                    completedPath = event.patch.completedPath ?: task.assetIndex.completedPath,
                                ),
                            error = event.patch.error,
                        )
                }
                is VesperDownloadEvent.ProgressUpdated -> {
                    val task = tasksById[event.patch.taskId] ?: return@forEach
                    tasksById[event.patch.taskId] = task.copy(progress = event.patch.progress)
                }
            }
        }
        return snapshot()
    }

    fun snapshot(): VesperDownloadSnapshot = VesperDownloadSnapshot(tasksById.values.toList())
}

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
        val source = outputFileForTask(taskId)
        val sharedFile = preparedShareFile(context, source, fileName)
        val uri = FileProvider.getUriForFile(context, authority, sharedFile)
        val intent =
            Intent(Intent.ACTION_SEND)
                .setType(mimeType ?: guessMimeType(sharedFile))
                .putExtra(Intent.EXTRA_STREAM, uri)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val chooser = Intent.createChooser(intent, null)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        if (context !is android.app.Activity) {
            chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        context.startActivity(chooser)
    }

    fun saveTaskOutput(
        context: Context,
        taskId: VesperDownloadTaskId,
        fileName: String? = null,
        collection: VesperDownloadPublicCollection = VesperDownloadPublicCollection.Downloads,
    ): Uri {
        check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            "saveTaskOutput requires Android 10 or newer MediaStore scoped storage"
        }
        val source = outputFileForTask(taskId)
        val displayName = sanitizedOutputFileName(fileName ?: source.name)
        val mimeType = guessMimeType(source)
        val values =
            ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, displayName)
                put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
                put(MediaStore.MediaColumns.RELATIVE_PATH, collection.relativePath)
                put(MediaStore.MediaColumns.IS_PENDING, 1)
            }
        val resolver = context.contentResolver
        val collectionUri = collection.contentUri
        val uri = checkNotNull(resolver.insert(collectionUri, values)) {
            "MediaStore did not allocate an output URI"
        }
        runCatching {
            resolver.openOutputStream(uri)?.use { output ->
                FileInputStream(source).use { input ->
                    input.copyTo(output)
                }
            } ?: error("MediaStore output stream was unavailable")
            values.clear()
            values.put(MediaStore.MediaColumns.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
        }.onFailure { error ->
            resolver.delete(uri, null, null)
            throw error
        }.getOrThrow()
        return uri
    }

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

    init {
        check(sessionHandle != 0L) { "native download session handle must not be zero" }
        restorePersistedTasks()
        forceFullSync()
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

        fun restoreDownloadTasks(
            sessionHandle: Long,
            tasks: Array<NativeDownloadTask>,
            nowEpochMs: Long,
        ): Boolean

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

        fun completeDownloadPreparation(
            sessionHandle: Long,
            taskId: Long,
            assetIndex: NativeDownloadAssetIndex,
            nowEpochMs: Long,
        ): Boolean

        fun replaceDownloadTaskPlan(
            sessionHandle: Long,
            taskId: Long,
            source: NativeDownloadSource,
            profile: NativeDownloadProfile,
            assetIndex: NativeDownloadAssetIndex,
            nowEpochMs: Long,
        ): Boolean

        fun exportDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            outputPath: String,
            progressCallback: NativeDownloadExportProgressCallback?,
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

        override fun restoreDownloadTasks(
            sessionHandle: Long,
            tasks: Array<NativeDownloadTask>,
            nowEpochMs: Long,
        ): Boolean =
            VesperNativeJni.restoreDownloadTasks(
                sessionHandle = sessionHandle,
                tasks = tasks,
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

        override fun completeDownloadPreparation(
            sessionHandle: Long,
            taskId: Long,
            assetIndex: NativeDownloadAssetIndex,
            nowEpochMs: Long,
        ): Boolean =
            VesperNativeJni.completeDownloadPreparation(
                sessionHandle = sessionHandle,
                taskId = taskId,
                assetIndex = assetIndex,
                nowEpochMs = nowEpochMs,
            )

        override fun replaceDownloadTaskPlan(
            sessionHandle: Long,
            taskId: Long,
            source: NativeDownloadSource,
            profile: NativeDownloadProfile,
            assetIndex: NativeDownloadAssetIndex,
            nowEpochMs: Long,
        ): Boolean =
            VesperNativeJni.replaceDownloadTaskPlan(
                sessionHandle = sessionHandle,
                taskId = taskId,
                source = source,
                profile = profile,
                assetIndex = assetIndex,
                nowEpochMs = nowEpochMs,
            )

        override fun exportDownloadTask(
            sessionHandle: Long,
            taskId: Long,
            outputPath: String,
            progressCallback: NativeDownloadExportProgressCallback?,
        ): Boolean =
            VesperNativeJni.exportDownloadTask(
                sessionHandle = sessionHandle,
                taskId = taskId,
                outputPath = outputPath,
                progressCallback = progressCallback,
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

internal interface VesperDownloadStatePersistence {
    fun load(): VesperDownloadSnapshot?

    fun save(snapshot: VesperDownloadSnapshot)
}

internal class VesperDownloadStateStore(private val file: File) : VesperDownloadStatePersistence {
    override fun load(): VesperDownloadSnapshot? =
        runCatching {
            if (!file.isFile) {
                return@runCatching null
            }
            JSONObject(file.readText()).toDownloadSnapshot()
        }.getOrNull()

    override fun save(snapshot: VesperDownloadSnapshot) {
        runCatching {
            val tasks = snapshot.tasks.filter { it.state != VesperDownloadState.Removed }
            if (tasks.isEmpty()) {
                file.delete()
                return@runCatching
            }
            file.parentFile?.mkdirs()
            val root =
                JSONObject().apply {
                    put("version", 1)
                    put(
                        "tasks",
                        JSONArray().apply {
                            tasks.forEach { put(it.toJson()) }
                        },
                    )
                }
            file.writeText(root.toString())
        }
    }
}

internal class VesperForegroundDownloadExecutor(
    context: Context?,
    private val baseDirectory: File?,
    private val resumePartialDownloads: Boolean = true,
    rangeChunkBytes: Long? = null,
    private val minProgressBytes: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_BYTES,
    private val minProgressIntervalMs: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_INTERVAL_MS,
    private val staleResourcePlanRecoverer: VesperDownloadStaleResourcePlanRecoverer? = null,
) : VesperDownloadExecutor {
    private val appContext = context?.applicationContext
    private val rangeChunkBytes = rangeChunkBytes?.takeIf { it > 0L }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobsLock = Any()
    private val jobs = mutableMapOf<VesperDownloadTaskId, Job>()
    private val recoveredSourcesLock = Any()
    private val recoveredSources = mutableMapOf<VesperDownloadTaskId, VesperDownloadSource>()
    private val dataSourceFactory by lazy {
        DefaultDataSource.Factory(checkNotNull(appContext) { "Android Context is required for non-HTTP downloads" })
    }

    private fun closeDataSourceQuietly(dataSource: DataSource) {
        runCatching { dataSource.close() }
    }

    private suspend fun prepareAssetIndexWithRecovery(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ): VesperDownloadAssetIndex {
        return try {
            materializeGeneratedResources(
                assetId = task.assetId,
                taskId = task.taskId,
                profile = task.profile,
                assetIndex = prepareAssetIndex(task),
            )
        } catch (error: VesperStaleDownloadResourceException) {
            val recoveredPlan =
                recoverTaskPlan(
                    task,
                    error.toStaleResource(
                        taskId = task.taskId,
                        fallbackPhase = VesperDownloadStaleResourcePhase.Prepare,
                    ),
                ) ?: throw error
            val recoveredAssetIndex =
                materializeGeneratedResources(
                    assetId = task.assetId,
                    taskId = task.taskId,
                    profile = recoveredPlan.profile,
                    assetIndex = recoveredPlan.assetIndex,
                )
            reporter.replaceTaskPlan(task.taskId, recoveredPlan.source, recoveredPlan.profile, recoveredAssetIndex)
            val recoveredTask = task.copy(
                source = recoveredPlan.source,
                profile = recoveredPlan.profile,
                assetIndex = recoveredAssetIndex,
            )
            val assetIndex =
                materializeGeneratedResources(
                    assetId = task.assetId,
                    taskId = task.taskId,
                    profile = recoveredPlan.profile,
                    assetIndex = prepareAssetIndex(recoveredTask),
                )
            synchronized(recoveredSourcesLock) {
                recoveredSources[task.taskId] = recoveredPlan.source
            }
            assetIndex
        }
    }

    private suspend fun recoverTaskPlan(
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource,
    ): VesperDownloadRecoveredTaskPlan? =
        staleResourcePlanRecoverer?.recoverPlan(task, staleResource)

    private fun materializeGeneratedResources(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex,
    ): VesperDownloadAssetIndex =
        VesperGeneratedDownloadResourceMaterializer(
            baseDirectory = baseDirectory,
            fallbackBaseDirectory = appContext?.filesDir?.let { vesperDefaultDownloadBaseDirectory(it, null) },
        ).materialize(assetId, taskId, profile, assetIndex)

    private fun VesperDownloadTaskSnapshot.withRecoveredSource(): VesperDownloadTaskSnapshot {
        val recoveredSource =
            synchronized(recoveredSourcesLock) {
                recoveredSources[taskId]
            } ?: return this
        return copy(source = recoveredSource)
    }

    private suspend fun prepareAssetIndex(task: VesperDownloadTaskSnapshot): VesperDownloadAssetIndex {
        val requestHeaders = task.source.source.headers
        if (task.assetIndex.resources.isNotEmpty() || task.assetIndex.segments.isNotEmpty()) {
            return completePreparedAssetIndex(task.source.contentFormat, task.assetIndex, requestHeaders)
        }

        return when (task.source.contentFormat) {
            VesperDownloadContentFormat.HlsSegments -> planHlsAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.DashSegments -> planDashAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.FlvSegments -> planFlvAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.SingleFile -> planSingleFileAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.Unknown -> error("download preparation cannot plan an unknown content format")
        }
    }

    private suspend fun completePreparedAssetIndex(
        contentFormat: VesperDownloadContentFormat,
        assetIndex: VesperDownloadAssetIndex,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val resources =
            assetIndex.resources.map { resource ->
                if (resource.sizeBytes != null || resource.generatedText != null) {
                    resource
                } else {
                    resource.copy(sizeBytes = probeRequiredSize(resource.uri, resource.byteRange, requestHeaders))
                }
            }
        val segments =
            assetIndex.segments.map { segment ->
                if (segment.sizeBytes != null) {
                    segment
                } else {
                    segment.copy(sizeBytes = probeRequiredSize(segment.uri, segment.byteRange, requestHeaders))
                }
            }
        val totalSizeBytes =
            assetIndex.totalSizeBytes
                ?: resources.sumOf { resource -> if (resource.generatedText == null) resource.sizeBytes ?: 0L else 0L }
                    .let { resourceBytes -> resourceBytes + segments.sumOf { it.sizeBytes ?: 0L } }
        return assetIndex.copy(
            contentFormat = contentFormat,
            totalSizeBytes = totalSizeBytes,
            resources = resources,
            segments = segments,
        )
    }

    private suspend fun planSingleFileAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val uri = task.source.manifestUri ?: task.source.source.uri
        val sizeBytes = probeRequiredSize(uri, null, requestHeaders)
        return VesperDownloadAssetIndex(
            contentFormat = task.source.contentFormat,
            totalSizeBytes = sizeBytes,
            resources =
                listOf(
                    VesperDownloadResourceRecord(
                        resourceId = "single-file",
                        uri = uri,
                        relativePath = inferredFileName(uri),
                        sizeBytes = sizeBytes,
                    ),
                ),
        )
    }

    private suspend fun planHlsAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val manifestUri = task.source.manifestUri ?: task.source.source.uri
        val manifestText = fetchText(manifestUri, requestHeaders)
        return if (manifestText.contains("#EXT-X-STREAM-INF", ignoreCase = true)) {
            planHlsMasterAssetIndex(manifestUri, manifestText, task.profile, requestHeaders)
        } else {
            val media = parseHlsMediaPlaylist(manifestUri, manifestText)
            buildHlsMediaAssetIndex("index.m3u8", listOf("media" to media), requestHeaders)
        }
    }

    private suspend fun planHlsMasterAssetIndex(
        manifestUri: String,
        manifestText: String,
        profile: VesperDownloadProfile,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val master = parseHlsMasterPlaylist(manifestUri, manifestText)
        val variant =
            profile.variantId
                ?.let { variantId ->
                    master.variants.firstOrNull { it.uri == variantId || it.attributes["NAME"] == variantId }
                }
                ?: master.variants.firstOrNull()
                ?: error("HLS master playlist did not contain a playable variant")
        val variantMedia = parseHlsMediaPlaylist(variant.uri, fetchText(variant.uri, requestHeaders))
        val media = mutableListOf("video" to variantMedia)
        val audio =
            profile.preferredAudioLanguage
                ?.let { language ->
                    master.audio.firstOrNull { it.attributes["LANGUAGE"]?.equals(language, ignoreCase = true) == true }
                }
                ?: master.audio.firstOrNull { it.attributes["DEFAULT"]?.equals("YES", ignoreCase = true) == true }
                ?: master.audio.firstOrNull()
        if (audio != null) {
            media += "audio" to parseHlsMediaPlaylist(audio.uri, fetchText(audio.uri, requestHeaders))
        }

        val planned = buildHlsMediaAssetIndex("index.m3u8", media, requestHeaders)
        val mediaResourceNames =
            planned.resources
                .mapNotNull { it.relativePath }
                .filter { it.endsWith(".m3u8") && it != "index.m3u8" }
        val masterText = rewriteHlsMaster(variant.attributes, mediaResourceNames)
        return planned.copy(
            resources =
                planned.resources.map { resource ->
                    if (resource.resourceId == "hls-master") {
                        resource.copy(generatedText = masterText)
                    } else {
                        resource
                    }
                },
        )
    }

    private suspend fun buildHlsMediaAssetIndex(
        manifestPath: String,
        mediaPlaylists: List<Pair<String, HlsMediaPlaylist>>,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val resources =
            mutableListOf(
                VesperDownloadResourceRecord(
                    resourceId = "hls-master",
                    uri = "vesper-generated://hls/$manifestPath",
                    relativePath = manifestPath,
                ),
            )
        val segments = mutableListOf<VesperDownloadSegmentRecord>()
        val seenMaps = linkedSetOf<String>()
        var totalSizeBytes = 0L

        mediaPlaylists.forEach { (mediaId, playlist) ->
            val playlistPath =
                if (mediaPlaylists.size == 1 && manifestPath == "index.m3u8") {
                    "index.m3u8"
                } else {
                    "$mediaId.m3u8"
                }
            val localMaps = linkedMapOf<String, String>()
            playlist.maps.forEachIndexed { index, map ->
                val key = "${map.uri}:${map.byteRange}"
                if (seenMaps.add(key)) {
                    val size = probeRequiredSize(map.uri, map.byteRange, requestHeaders)
                    totalSizeBytes += size
                    val relativePath = "segments/$mediaId-init-$index.${extensionFromUri(map.uri, "mp4")}"
                    resources +=
                        VesperDownloadResourceRecord(
                            resourceId = "hls-$mediaId-init-$index",
                            uri = map.uri,
                            relativePath = relativePath,
                            byteRange = map.byteRange,
                            sizeBytes = size,
                        )
                    localMaps[key] = relativePath
                }
            }

            playlist.segments.forEach { segment ->
                val size = probeRequiredSize(segment.uri, segment.byteRange, requestHeaders)
                totalSizeBytes += size
                segments +=
                    VesperDownloadSegmentRecord(
                        segmentId = "hls-$mediaId-${segment.sequence}",
                        uri = segment.uri,
                        relativePath = "segments/$mediaId-${segment.sequence.toString().padStart(5, '0')}.${extensionFromUri(segment.uri, "ts")}",
                        sequence = segment.sequence,
                        byteRange = segment.byteRange,
                        sizeBytes = size,
                    )
            }

            val mediaText = rewriteHlsMedia(mediaId, playlist, localMaps)
            resources +=
                VesperDownloadResourceRecord(
                    resourceId = "hls-$mediaId-playlist",
                    uri = "vesper-generated://hls/$playlistPath",
                    relativePath = playlistPath,
                    generatedText = mediaText,
                )
        }

        if (mediaPlaylists.size == 1 && manifestPath == "index.m3u8") {
            val mediaResource = resources.firstOrNull { it.resourceId.endsWith("-playlist") }
            if (mediaResource != null) {
                resources.remove(mediaResource)
                resources[0] = resources[0].copy(generatedText = mediaResource.generatedText)
            }
        }

        return VesperDownloadAssetIndex(
            contentFormat = VesperDownloadContentFormat.HlsSegments,
            totalSizeBytes = totalSizeBytes,
            resources = resources,
            segments = segments,
        )
    }

    private suspend fun planDashAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val manifestUri = task.source.manifestUri ?: task.source.source.uri
        val manifestText = fetchText(manifestUri, requestHeaders)
        val document = parseXmlDocument(manifestText)
        val documentType = document.documentElement.getAttribute("type")
        if (documentType.isNotBlank() && !documentType.equals("static", ignoreCase = true)) {
            error("DASH download preparation requires a static MPD")
        }
        val durationSeconds = parseIso8601DurationSeconds(document.documentElement.getAttribute("mediaPresentationDuration"))
        val plannedRepresentations = selectDashRepresentations(document, manifestUri, task.profile)
        if (plannedRepresentations.isEmpty()) {
            error("DASH MPD did not contain a supported SegmentTemplate or SegmentBase representation")
        }

        val resources = mutableListOf<VesperDownloadResourceRecord>()
        val segments = mutableListOf<VesperDownloadSegmentRecord>()
        val rewrittenAdaptationSets = mutableListOf<String>()
        var totalSizeBytes = 0L
        var globalSequence = 1L

        plannedRepresentations.forEachIndexed { index, representation ->
            val mediaId = representation.mediaId.ifBlank { "media$index" }
            if (representation.template != null) {
                val template = representation.template
                if (template.duration <= 0L) {
                    error("DASH SegmentTemplate duration must be greater than zero")
                }
                val segmentSeconds = template.duration.toDouble() / template.timescale.coerceAtLeast(1L).toDouble()
                val segmentCount =
                    durationSeconds
                        ?.let { kotlin.math.ceil(it / segmentSeconds).toLong().coerceAtLeast(1L) }
                        ?: error("DASH SegmentTemplate planning requires a finite MPD duration")
                val initLocalPath = "segments/$mediaId-init.mp4"
                template.initialization?.takeIf { it.isNotBlank() }?.let { initialization ->
                    val remote = resolveRemoteReference(representation.baseUri, expandDashTemplate(initialization, representation.id, template.startNumber))
                    val size = probeRequiredSize(remote, null, requestHeaders)
                    totalSizeBytes += size
                    resources +=
                        VesperDownloadResourceRecord(
                            resourceId = "dash-$mediaId-init",
                            uri = remote,
                            relativePath = initLocalPath,
                            sizeBytes = size,
                        )
                }
                repeat(segmentCount.toInt()) { offset ->
                    val number = template.startNumber + offset
                    val remote = resolveRemoteReference(representation.baseUri, expandDashTemplate(template.media, representation.id, number))
                    val size = probeRequiredSize(remote, null, requestHeaders)
                    totalSizeBytes += size
                    segments +=
                        VesperDownloadSegmentRecord(
                            segmentId = "dash-$mediaId-segment-$number",
                            uri = remote,
                            relativePath = "segments/$mediaId-$number.m4s",
                            sequence = globalSequence++,
                            sizeBytes = size,
                        )
                }
                rewrittenAdaptationSets += rewriteDashTemplateAdaptationSet(representation, template, mediaId, segmentCount)
            } else if (representation.baseUrl != null) {
                val remote = resolveRemoteReference(representation.baseUri, representation.baseUrl)
                val size = probeRequiredSize(remote, null, requestHeaders)
                totalSizeBytes += size
                val localName = "media-$mediaId.${extensionFromUri(remote, "mp4")}"
                resources +=
                    VesperDownloadResourceRecord(
                        resourceId = "dash-$mediaId-media",
                        uri = remote,
                        relativePath = localName,
                        sizeBytes = size,
                    )
                rewrittenAdaptationSets += rewriteDashSegmentBaseAdaptationSet(representation, localName)
            }
        }

        resources.add(
            0,
            VesperDownloadResourceRecord(
                resourceId = "dash-manifest",
                uri = "vesper-generated://dash/manifest.mpd",
                relativePath = "manifest.mpd",
                generatedText = rewriteDashMpd(document.documentElement.getAttribute("mediaPresentationDuration"), rewrittenAdaptationSets),
            ),
        )

        return VesperDownloadAssetIndex(
            contentFormat = VesperDownloadContentFormat.DashSegments,
            totalSizeBytes = totalSizeBytes,
            resources = resources,
            segments = segments,
        )
    }

    private suspend fun planFlvAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val uri = task.source.manifestUri ?: task.source.source.uri
        val clipUris =
            if (extensionFromUri(uri, "flv").equals("flv", ignoreCase = true)) {
                listOf(uri)
            } else {
                parseFlvClipManifest(uri, fetchText(uri, requestHeaders))
            }
        if (clipUris.isEmpty()) {
            error("FLV clip manifest did not contain any clip URI")
        }

        var totalSizeBytes = 0L
        val concat = StringBuilder("ffconcat version 1.0\n")
        val segments =
            clipUris.mapIndexed { index, clipUri ->
                val sequence = index + 1L
                val size = probeRequiredSize(clipUri, null, requestHeaders)
                totalSizeBytes += size
                val localPath = "clips/clip-${sequence.toString().padStart(5, '0')}.${extensionFromUri(clipUri, "flv")}"
                concat.append("file '").append(escapeFfconcatPath(localPath)).append("'\n")
                VesperDownloadSegmentRecord(
                    segmentId = "flv-clip-$sequence",
                    uri = clipUri,
                    relativePath = localPath,
                    sequence = sequence,
                    sizeBytes = size,
                )
            }

        return VesperDownloadAssetIndex(
            contentFormat = VesperDownloadContentFormat.FlvSegments,
            totalSizeBytes = totalSizeBytes,
            resources =
                listOf(
                    VesperDownloadResourceRecord(
                        resourceId = "flv-concat",
                        uri = "vesper-generated://flv/manifest.ffconcat",
                        relativePath = "manifest.ffconcat",
                        generatedText = concat.toString(),
                    ),
                ),
            segments = segments,
        )
    }

    override fun prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        scope.launch {
            try {
                reporter.completePreparation(task.taskId, prepareAssetIndexWithRecovery(task, reporter))
            } catch (error: Exception) {
                reporter.fail(
                    task.taskId,
                    VesperDownloadError(
                        codeOrdinal = ANDROID_DOWNLOAD_BACKEND_FAILURE_ORDINAL,
                        categoryOrdinal = ANDROID_DOWNLOAD_NETWORK_CATEGORY_ORDINAL,
                        retriable = false,
                        message = error.message ?: "android download preparation failed",
                    ),
                )
            }
        }
    }

    override fun start(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(task.withRecoveredSource(), reporter)
    }

    override fun resume(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(task.withRecoveredSource(), reporter)
    }

    override fun pause(taskId: VesperDownloadTaskId) {
        synchronized(jobsLock) {
            jobs.remove(taskId)
        }?.cancel()
    }

    override fun remove(task: VesperDownloadTaskSnapshot?) {
        if (task != null) {
            pause(task.taskId)
            synchronized(recoveredSourcesLock) {
                recoveredSources.remove(task.taskId)
            }
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
                var receivedBytes = 0L
                var receivedSegments = 0
                var activeEntry: ForegroundDownloadEntry? = null
                try {
                    val downloadContext = currentCoroutineContext()
                    downloadContext.ensureActive()
                    val materializedTask =
                        task.copy(
                            assetIndex =
                                materializeGeneratedResources(
                                    assetId = task.assetId,
                                    taskId = task.taskId,
                                    profile = task.profile,
                                    assetIndex = task.assetIndex,
                                ),
                        )
                    val plan = buildExecutionPlan(materializedTask)
                    val requestHeaders = materializedTask.source.source.headers
                    val trackSegments = materializedTask.assetIndex.segments.isNotEmpty()
                    val progressThrottle = DownloadProgressThrottle(minProgressBytes, minProgressIntervalMs)

                    for ((index, entry) in plan.withIndex()) {
                        downloadContext.ensureActive()
                        activeEntry = entry
                        val outputFile = resolveOutputFile(materializedTask, entry, index)
                        outputFile.parentFile?.mkdirs()

                        val bytesWritten =
                            if (entry.generatedText != null) {
                                runInterruptible {
                                    outputFile.writeText(entry.generatedText)
                                }
                                0L
                            } else {
                                val resumeFromBytes =
                                    resumableExistingBytes(
                                        destination = outputFile,
                                        expectedSizeBytes = entry.expectedSizeBytes,
                                    )
                                copyUriToFile(
                                    sourceUri = entry.uri,
                                    byteRange = entry.byteRange,
                                    requestHeaders = requestHeaders,
                                    destination = outputFile,
                                    expectedSizeBytes = entry.expectedSizeBytes,
                                    resumeFromBytes = resumeFromBytes,
                                ) { writtenBytes ->
                                    downloadContext.ensureActive()
                                    val nextBytes = receivedBytes + writtenBytes
                                    if (progressThrottle.shouldReport(nextBytes)) {
                                        reporter.updateProgress(task.taskId, nextBytes, receivedSegments)
                                    }
                                }
                            }

                        downloadContext.ensureActive()
                        receivedBytes += bytesWritten
                        if (trackSegments && entry.isSegment) {
                            receivedSegments += 1
                        }
                        progressThrottle.markReported(receivedBytes)
                        reporter.updateProgress(task.taskId, receivedBytes, receivedSegments)
                    }

                    downloadContext.ensureActive()
                    synchronized(recoveredSourcesLock) {
                        recoveredSources.remove(task.taskId)
                    }
                    reporter.complete(task.taskId, resolveCompletedPath(materializedTask, plan))
                } catch (_: CancellationException) {
                    return@launch
                } catch (error: VesperStaleDownloadResourceException) {
                    try {
                        if (
                            recoverStaleDownload(
                                task = task,
                                staleError = error,
                                activeEntry = activeEntry,
                                receivedBytes = receivedBytes,
                                reporter = reporter,
                            )
                        ) {
                            return@launch
                        }
                    } catch (recoveryError: Exception) {
                        reporter.fail(
                            task.taskId,
                            VesperDownloadError(
                                codeOrdinal = ANDROID_DOWNLOAD_BACKEND_FAILURE_ORDINAL,
                                categoryOrdinal = ANDROID_DOWNLOAD_NETWORK_CATEGORY_ORDINAL,
                                retriable = false,
                                message = recoveryError.message ?: "android download recovery failed",
                            ),
                        )
                        return@launch
                    }
                    reporter.fail(
                        task.taskId,
                        VesperDownloadError(
                            codeOrdinal = ANDROID_DOWNLOAD_BACKEND_FAILURE_ORDINAL,
                            categoryOrdinal = ANDROID_DOWNLOAD_NETWORK_CATEGORY_ORDINAL,
                            retriable = false,
                            message = error.message ?: "android foreground download failed",
                        ),
                    )
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

    private suspend fun recoverStaleDownload(
        task: VesperDownloadTaskSnapshot,
        staleError: VesperStaleDownloadResourceException,
        activeEntry: ForegroundDownloadEntry?,
        receivedBytes: Long,
        reporter: VesperDownloadExecutionReporter,
    ): Boolean {
        val staleResource =
            staleError.toStaleResource(
                taskId = task.taskId,
                fallbackResourceId = activeEntry?.resourceId,
                fallbackSegmentId = activeEntry?.segmentId,
                fallbackUri = activeEntry?.uri,
                fallbackPhase = VesperDownloadStaleResourcePhase.Download,
                fallbackReceivedBytes = receivedBytes,
            )
        val recoveredPlan = recoverTaskPlan(task, staleResource) ?: return false
        pause(task.taskId)
        runInterruptible { resolveBaseDirectory(task).deleteRecursively() }
        val recoveredAssetIndex =
            materializeGeneratedResources(
                assetId = task.assetId,
                taskId = task.taskId,
                profile = recoveredPlan.profile,
                assetIndex = recoveredPlan.assetIndex,
            )
        reporter.replaceTaskPlan(task.taskId, recoveredPlan.source, recoveredPlan.profile, recoveredAssetIndex)
        val recoveredTask =
            task.copy(
                source = recoveredPlan.source,
                profile = recoveredPlan.profile,
                state = VesperDownloadState.Preparing,
                progress = VesperDownloadProgressSnapshot(),
                assetIndex = recoveredAssetIndex,
                error = null,
            )
        val preparedAssetIndex =
            materializeGeneratedResources(
                assetId = task.assetId,
                taskId = task.taskId,
                profile = recoveredPlan.profile,
                assetIndex = prepareAssetIndex(recoveredTask),
            )
        reporter.completePreparation(task.taskId, preparedAssetIndex)
        return true
    }

    private fun buildExecutionPlan(task: VesperDownloadTaskSnapshot): List<ForegroundDownloadEntry> {
        val resources =
            task.assetIndex.resources.map { resource ->
                ForegroundDownloadEntry(
                    uri = resource.uri,
                    resourceId = resource.resourceId.ifBlank { null },
                    segmentId = null,
                    relativePath = resource.relativePath,
                    byteRange = resource.byteRange,
                    generatedText = resource.generatedText,
                    expectedSizeBytes = resource.sizeBytes,
                    fallbackName = resource.resourceId.ifBlank { "resource" },
                    isSegment = false,
                )
            }
        val segments =
            task.assetIndex.segments.mapIndexed { index, segment ->
                ForegroundDownloadEntry(
                    uri = segment.uri,
                    resourceId = null,
                    segmentId = segment.segmentId.ifBlank { null },
                    relativePath = segment.relativePath,
                    byteRange = segment.byteRange,
                    generatedText = null,
                    expectedSizeBytes = segment.sizeBytes,
                    fallbackName =
                        segment.segmentId.ifBlank {
                            "segment-${segment.sequence ?: (index + 1).toLong()}"
                        },
                    isSegment = true,
                )
            }
        if (resources.isNotEmpty() || segments.isNotEmpty()) {
            return buildList {
                addAll(resources)
                addAll(segments)
            }
        }

        val fallbackUri = task.source.manifestUri ?: task.source.source.uri
        return listOf(
            ForegroundDownloadEntry(
                uri = fallbackUri,
                resourceId = null,
                segmentId = null,
                relativePath = null,
                byteRange = null,
                generatedText = null,
                expectedSizeBytes = task.progress.totalBytes,
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
            if (candidate.isAbsolute) {
                return candidate
            }
            val parts = relativePath.split('/', '\\')
            require(parts.none { it == ".." }) { "download output path escapes the task directory: $relativePath" }
            val outputFile = File(baseDirectory, relativePath).canonicalFile
            val canonicalBase = baseDirectory.canonicalFile
            require(outputFile.path == canonicalBase.path || outputFile.path.startsWith(canonicalBase.path + File.separator)) {
                "download output path escapes the task directory: $relativePath"
            }
            return outputFile
        }

        val inferredName =
            lastPathSegmentFromUri(entry.uri)
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
            baseDirectory
                ?: vesperDefaultDownloadBaseDirectory(
                    checkNotNull(appContext) { "Android Context is required when no download base directory is configured" }.filesDir,
                    null,
                ),
            task.assetId.ifBlank { task.taskId.toString() },
        )

    private suspend fun copyUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        onProgress: (Long) -> Unit,
    ): Long {
        if (isHttpUri(sourceUri)) {
            return copyHttpUriToFile(
                sourceUri = sourceUri,
                byteRange = byteRange,
                requestHeaders = requestHeaders,
                destination = destination,
                expectedSizeBytes = expectedSizeBytes,
                resumeFromBytes = resumeFromBytes,
                allowRestartAfterRangeMismatch = true,
                onProgress = onProgress,
            )
        }
        if (uriScheme(sourceUri).equals("file", ignoreCase = true)) {
            return copyLocalFileUriToFile(
                sourceUri = sourceUri,
                byteRange = byteRange,
                destination = destination,
                expectedSizeBytes = expectedSizeBytes,
                resumeFromBytes = resumeFromBytes,
                onProgress = onProgress,
            )
        }
        return copyDataSourceUriToFile(
            sourceUri = sourceUri,
            byteRange = byteRange,
            requestHeaders = requestHeaders,
            destination = destination,
            expectedSizeBytes = expectedSizeBytes,
            resumeFromBytes = resumeFromBytes,
            onProgress = onProgress,
        )
    }

    private suspend fun copyLocalFileUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
        if (expected != null && resumeOffset >= expected) {
            return expected
        }
        val sourceFile = File(URI(sourceUri))
        val startOffset = (byteRange?.offset ?: 0L).coerceAtLeast(0L) + resumeOffset
        var remaining = byteRange?.let { (it.length.coerceAtLeast(0L) - resumeOffset).coerceAtLeast(0L) }
        var totalWritten = resumeOffset
        var reportedBytes = resumeOffset
        runInterruptible {
            sourceFile.inputStream().use { input ->
                if (startOffset > 0L) {
                    input.skip(startOffset)
                }
                FileOutputStream(destination, resumeOffset > 0L).use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (remaining == null || remaining!! > 0L) {
                        copyContext.ensureActive()
                        val limit = minOf(buffer.size.toLong(), remaining ?: buffer.size.toLong()).toInt()
                        val read = input.read(buffer, 0, limit)
                        if (read == -1) {
                            break
                        }
                        output.write(buffer, 0, read)
                        totalWritten += read.toLong()
                        remaining = remaining?.let { (it - read).coerceAtLeast(0L) }
                        if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                            reportedBytes = totalWritten
                            onProgress(totalWritten)
                        }
                    }
                }
            }
        }
        if (expected != null && totalWritten != expected) {
            error("copied $totalWritten bytes for $sourceUri, expected $expected")
        }
        return totalWritten
    }

    private suspend fun copyDataSourceUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
        if (expected != null && resumeOffset >= expected) {
            return expected
        }
        val dataSource = dataSourceFactory.createDataSource()
        val dataSpecBuilder = DataSpec.Builder()
            .setUri(sourceUri)
            .setDownloadRequestHeaders(requestHeaders)
        if (byteRange != null) {
            val remaining = byteRange.length.coerceAtLeast(0L) - resumeOffset
            dataSpecBuilder
                .setPosition(byteRange.offset.coerceAtLeast(0L) + resumeOffset)
                .setLength(remaining.coerceAtLeast(0L))
        } else if (resumeOffset > 0L) {
            dataSpecBuilder.setPosition(resumeOffset)
        }
        val dataSpec = dataSpecBuilder.build()
        var totalWritten = resumeOffset
        var reportedBytes = resumeOffset
        FileOutputStream(destination, resumeOffset > 0L).use { output ->
            try {
                copyContext.ensureActive()
                runInterruptible {
                    dataSource.open(dataSpec)
                }
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    copyContext.ensureActive()
                    val read =
                        runInterruptible {
                            dataSource.read(buffer, 0, buffer.size)
                        }
                    if (read == -1) {
                        break
                    }
                    copyContext.ensureActive()
                    runInterruptible {
                        output.write(buffer, 0, read)
                    }
                    totalWritten += read.toLong()
                    if (expected != null && totalWritten > expected) {
                        runCatching { destination.delete() }
                        error("remote server did not honor the requested resume range for $sourceUri")
                    }
                    if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                        copyContext.ensureActive()
                        reportedBytes = totalWritten
                        onProgress(totalWritten)
                    }
                }
            } finally {
                closeDataSourceQuietly(dataSource)
            }
        }
        if (expected != null && totalWritten != expected) {
            error("downloaded ${totalWritten} bytes for $sourceUri, expected $expected")
        }
        return totalWritten
    }

    private suspend fun copyHttpUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        allowRestartAfterRangeMismatch: Boolean,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
        if (expected != null && resumeOffset >= expected) {
            return expected
        }
        if (byteRange == null && expected != null && expected > 0L && rangeChunkBytes != null) {
            return copyKnownSizeHttpUriToFile(
                sourceUri = sourceUri,
                requestHeaders = requestHeaders,
                destination = destination,
                expectedSizeBytes = expected,
                resumeFromBytes = resumeOffset,
                rangeChunkBytes = rangeChunkBytes,
                allowRestartAfterRangeMismatch = allowRestartAfterRangeMismatch,
                onProgress = onProgress,
            )
        }

        val rangeHeader = requestedHttpRangeHeader(byteRange, resumeOffset)
        val requestedRangeStart = requestedHttpRangeStart(byteRange, resumeOffset)
        val connection =
            runInterruptible {
                (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                    applyDownloadRequestHeaders(requestHeaders)
                    rangeHeader?.let { setRequestProperty("Range", it) }
                    instanceFollowRedirects = true
                    connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                    readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                }
            }

        try {
            val status = runInterruptible { connection.responseCode }
            when {
                status == HttpURLConnection.HTTP_PARTIAL -> {
                    val contentRangeStart = parseHttpContentRangeStart(connection.getHeaderField("Content-Range"))
                    if (requestedRangeStart == null || contentRangeStart != requestedRangeStart) {
                        throw staleDownloadResource(
                            "remote server returned an unexpected Content-Range for $sourceUri",
                        )
                    }
                }
                status == HttpURLConnection.HTTP_OK -> {
                    if (requestedRangeStart != null) {
                        if (byteRange == null && resumeOffset > 0L && allowRestartAfterRangeMismatch) {
                            connection.disconnect()
                            runInterruptible { destination.delete() }
                            onProgress(0L)
                            return copyHttpUriToFile(
                                sourceUri = sourceUri,
                                byteRange = byteRange,
                                requestHeaders = requestHeaders,
                                destination = destination,
                                expectedSizeBytes = expectedSizeBytes,
                                resumeFromBytes = 0L,
                                allowRestartAfterRangeMismatch = false,
                                onProgress = onProgress,
                            )
                        }
                        throw staleDownloadResource(
                            "remote server did not honor the requested byte range for $sourceUri",
                        )
                    }
                }
                status == ANDROID_HTTP_RANGE_NOT_SATISFIABLE -> {
                    if (resumeOffset > 0L && allowRestartAfterRangeMismatch) {
                        connection.disconnect()
                        runInterruptible { destination.delete() }
                        onProgress(0L)
                        return copyHttpUriToFile(
                            sourceUri = sourceUri,
                            byteRange = byteRange,
                            requestHeaders = requestHeaders,
                            destination = destination,
                            expectedSizeBytes = expectedSizeBytes,
                            resumeFromBytes = 0L,
                            allowRestartAfterRangeMismatch = false,
                            onProgress = onProgress,
                        )
                    }
                    throw staleDownloadResource(
                        "remote resource rejected the requested byte range for $sourceUri",
                    )
                }
                isExpiredHttpStatus(status) -> {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                    )
                }
                status !in 200..299 -> {
                    throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
                }
            }

            var totalWritten = resumeOffset
            var reportedBytes = resumeOffset
            val append = status == HttpURLConnection.HTTP_PARTIAL && resumeOffset > 0L
            FileOutputStream(destination, append).use { output ->
                val input =
                    runInterruptible {
                        connection.inputStream
                    }
                input.use { stream ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        copyContext.ensureActive()
                        val read =
                            runInterruptible {
                                stream.read(buffer, 0, buffer.size)
                            }
                        if (read == -1) {
                            break
                        }
                        copyContext.ensureActive()
                        runInterruptible {
                            output.write(buffer, 0, read)
                        }
                        totalWritten += read.toLong()
                        if (expected != null && totalWritten > expected) {
                            runInterruptible { destination.delete() }
                            throw staleDownloadResource(
                                "remote server sent more bytes than expected for $sourceUri",
                            )
                        }
                        if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                            copyContext.ensureActive()
                            reportedBytes = totalWritten
                            onProgress(totalWritten)
                        }
                    }
                }
            }
            if (expected != null && totalWritten != expected) {
                error("downloaded ${totalWritten} bytes for $sourceUri, expected $expected")
            }
            return totalWritten
        } finally {
            connection.disconnect()
        }
    }

    private fun resumableExistingBytes(
        destination: File,
        expectedSizeBytes: Long?,
    ): Long {
        if (!destination.exists()) {
            return 0L
        }
        if (!resumePartialDownloads) {
            destination.delete()
            return 0L
        }
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        if (expected == null) {
            destination.delete()
            return 0L
        }
        val existing = destination.length().coerceAtLeast(0L)
        return when {
            existing == expected -> existing
            existing in 1 until expected -> existing
            else -> {
                destination.delete()
                0L
            }
        }
    }

    private suspend fun copyKnownSizeHttpUriToFile(
        sourceUri: String,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long,
        resumeFromBytes: Long,
        rangeChunkBytes: Long,
        allowRestartAfterRangeMismatch: Boolean,
        onProgress: (Long) -> Unit,
    ): Long {
        val expected = expectedSizeBytes.coerceAtLeast(0L)
        var offset = resumeFromBytes.coerceAtLeast(0L)
        if (offset >= expected) {
            return expected
        }
        while (offset < expected) {
            val chunkLength = minOf(rangeChunkBytes, expected - offset)
            val chunkEnd = offset + chunkLength - 1L
            val nextOffset =
                copyHttpUriRangeChunkToFile(
                    sourceUri = sourceUri,
                    requestHeaders = requestHeaders,
                    destination = destination,
                    expectedSizeBytes = expected,
                    rangeStart = offset,
                    rangeEndInclusive = chunkEnd,
                    rangeChunkBytes = rangeChunkBytes,
                    allowRestartAfterRangeMismatch = allowRestartAfterRangeMismatch,
                    onProgress = onProgress,
                )
            check(nextOffset > offset) { "download range transfer did not advance for $sourceUri" }
            offset = nextOffset
        }
        return offset
    }

    private suspend fun copyHttpUriRangeChunkToFile(
        sourceUri: String,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long,
        rangeStart: Long,
        rangeEndInclusive: Long,
        rangeChunkBytes: Long,
        allowRestartAfterRangeMismatch: Boolean,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val connection =
            runInterruptible {
                (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                    applyDownloadRequestHeaders(requestHeaders)
                    setRequestProperty("Range", "bytes=$rangeStart-$rangeEndInclusive")
                    instanceFollowRedirects = true
                    connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                    readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                }
            }

        try {
            val status = runInterruptible { connection.responseCode }
            val chunkCoversWholeResource = rangeStart == 0L && rangeEndInclusive + 1L >= expectedSizeBytes
            when {
                status == HttpURLConnection.HTTP_PARTIAL -> {
                    val contentRangeStart = parseHttpContentRangeStart(connection.getHeaderField("Content-Range"))
                    if (contentRangeStart != rangeStart) {
                        throw staleDownloadResource(
                            "remote server returned an unexpected Content-Range for $sourceUri",
                        )
                    }
                }
                status == HttpURLConnection.HTTP_OK -> {
                    if (!chunkCoversWholeResource) {
                        if (rangeStart > 0L && allowRestartAfterRangeMismatch) {
                            connection.disconnect()
                            runInterruptible { destination.delete() }
                            onProgress(0L)
                            return copyKnownSizeHttpUriToFile(
                                sourceUri = sourceUri,
                                requestHeaders = requestHeaders,
                                destination = destination,
                                expectedSizeBytes = expectedSizeBytes,
                                resumeFromBytes = 0L,
                                rangeChunkBytes = rangeChunkBytes,
                                allowRestartAfterRangeMismatch = false,
                                onProgress = onProgress,
                            )
                        }
                        throw staleDownloadResource(
                            "remote server did not honor the requested byte range for $sourceUri",
                        )
                    }
                }
                status == ANDROID_HTTP_RANGE_NOT_SATISFIABLE -> {
                    if (rangeStart > 0L && allowRestartAfterRangeMismatch) {
                        connection.disconnect()
                        runInterruptible { destination.delete() }
                        onProgress(0L)
                        return copyKnownSizeHttpUriToFile(
                            sourceUri = sourceUri,
                            requestHeaders = requestHeaders,
                            destination = destination,
                            expectedSizeBytes = expectedSizeBytes,
                            resumeFromBytes = 0L,
                            rangeChunkBytes = rangeChunkBytes,
                            allowRestartAfterRangeMismatch = false,
                            onProgress = onProgress,
                        )
                    }
                    throw staleDownloadResource(
                        "remote resource rejected the requested byte range for $sourceUri",
                    )
                }
                isExpiredHttpStatus(status) -> {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                    )
                }
                status !in 200..299 -> {
                    throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
                }
            }

            val append = status == HttpURLConnection.HTTP_PARTIAL && rangeStart > 0L
            var totalWritten = if (append) rangeStart else 0L
            var reportedBytes = totalWritten
            FileOutputStream(destination, append).use { output ->
                val input =
                    runInterruptible {
                        connection.inputStream
                    }
                input.use { stream ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        copyContext.ensureActive()
                        val read =
                            runInterruptible {
                                stream.read(buffer, 0, buffer.size)
                            }
                        if (read == -1) {
                            break
                        }
                        copyContext.ensureActive()
                        runInterruptible {
                            output.write(buffer, 0, read)
                        }
                        totalWritten += read.toLong()
                        if (totalWritten > expectedSizeBytes) {
                            runInterruptible { destination.delete() }
                            throw staleDownloadResource(
                                "remote server sent more bytes than expected for $sourceUri",
                            )
                        }
                        if (status == HttpURLConnection.HTTP_PARTIAL && totalWritten > rangeEndInclusive + 1L) {
                            runInterruptible { destination.delete() }
                            throw staleDownloadResource(
                                "remote server sent more bytes than the requested byte range for $sourceUri",
                            )
                        }
                        if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                            copyContext.ensureActive()
                            reportedBytes = totalWritten
                            onProgress(totalWritten)
                        }
                    }
                }
            }

            return if (status == HttpURLConnection.HTTP_PARTIAL) {
                val expectedNextOffset = rangeEndInclusive + 1L
                if (totalWritten != expectedNextOffset) {
                    throw staleDownloadResource(
                        "downloaded range ended at $totalWritten for $sourceUri, expected $expectedNextOffset",
                    )
                }
                totalWritten
            } else {
                if (totalWritten != expectedSizeBytes) {
                    throw staleDownloadResource(
                        "downloaded $totalWritten bytes for $sourceUri, expected $expectedSizeBytes",
                    )
                }
                totalWritten
            }
        } finally {
            connection.disconnect()
        }
    }

    private suspend fun fetchText(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): String {
        if (isHttpUri(sourceUri)) {
            return fetchHttpText(sourceUri, requestHeaders)
        }
        val dataSource = dataSourceFactory.createDataSource()
        return try {
            runInterruptible {
                dataSource.open(
                    DataSpec.Builder()
                        .setUri(sourceUri)
                        .setDownloadRequestHeaders(requestHeaders)
                        .build(),
                )
            }
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(32 * 1024)
            while (true) {
                val read =
                    runInterruptible {
                        dataSource.read(buffer, 0, buffer.size)
                    }
                if (read == -1) {
                    break
                }
                output.write(buffer, 0, read)
            }
            output.toString(Charsets.UTF_8.name())
        } finally {
            closeDataSourceQuietly(dataSource)
        }
    }

    private suspend fun probeRequiredSize(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
    ): Long {
        if (byteRange != null) {
            return byteRange.length.coerceAtLeast(0L)
        }
        return probeContentLength(sourceUri, requestHeaders)
    }

    private suspend fun probeContentLength(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): Long {
        val scheme = uriScheme(sourceUri)
        if (scheme.equals("file", ignoreCase = true)) {
            val fileSize = runCatching { URI(sourceUri).path?.let(::File)?.length() }.getOrNull() ?: 0L
            if (fileSize > 0L) {
                return fileSize
            }
        }
        if (scheme.equals("http", ignoreCase = true) || scheme.equals("https", ignoreCase = true)) {
            probeHttpContentLength(sourceUri, requestHeaders)?.let { return it }
        }

        val dataSource = dataSourceFactory.createDataSource()
        return try {
            val length =
                runInterruptible {
                    dataSource.open(
                        DataSpec.Builder()
                            .setUri(sourceUri)
                            .setDownloadRequestHeaders(requestHeaders)
                            .build(),
                    )
                }
            if (length <= 0L) {
                error("remote resource did not expose a stable content length")
            }
            length
        } finally {
            closeDataSourceQuietly(dataSource)
        }
    }

    private suspend fun fetchHttpText(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): String =
        runInterruptible {
            val connection = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
            try {
                val status = connection.responseCode
                if (isExpiredHttpStatus(status)) {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                    )
                }
                if (status !in 200..299) {
                    throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
                }
                connection.inputStream.use { input ->
                    input.readBytes().toString(Charsets.UTF_8)
                }
            } finally {
                connection.disconnect()
            }
        }

    private suspend fun probeHttpContentLength(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): Long? =
        runInterruptible {
            val head = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                requestMethod = "HEAD"
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
            val headStatus = head.responseCode
            try {
                head.inputStream.close()
            } catch (_: Exception) {
                runCatching { head.errorStream?.close() }
            }
            if (isExpiredHttpStatus(headStatus)) {
                head.disconnect()
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $headStatus) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            head.getHeaderField("Content-Length")?.toLongOrNull()?.takeIf { it > 0L }?.let {
                head.disconnect()
                return@runInterruptible it
            }
            head.disconnect()

            val range = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                setRequestProperty("Range", "bytes=0-0")
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
            val rangeStatus = range.responseCode
            try {
                range.inputStream.close()
            } catch (_: Exception) {
                runCatching { range.errorStream?.close() }
            }
            if (isExpiredHttpStatus(rangeStatus)) {
                range.disconnect()
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $rangeStatus) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            val size =
                range.getHeaderField("Content-Range")
                    ?.substringAfterLast('/', "")
                    ?.toLongOrNull()
                    ?.takeIf { it > 0L }
            range.disconnect()
            size
        }

    private fun inferredFileName(uri: String): String =
        lastPathSegmentFromUri(uri) ?: "media.bin"
}

private data class ForegroundDownloadEntry(
    val uri: String,
    val resourceId: String?,
    val segmentId: String?,
    val relativePath: String?,
    val byteRange: VesperDownloadByteRange?,
    val generatedText: String?,
    val expectedSizeBytes: Long?,
    val fallbackName: String,
    val isSegment: Boolean,
)

private data class HlsMasterPlaylist(
    val variants: List<HlsVariant>,
    val audio: List<HlsRendition>,
)

private data class HlsVariant(
    val uri: String,
    val attributes: Map<String, String>,
)

private data class HlsRendition(
    val uri: String,
    val attributes: Map<String, String>,
)

private data class HlsMediaPlaylist(
    val targetDuration: String?,
    val version: String?,
    val maps: List<HlsMap>,
    val segments: List<HlsSegment>,
)

private data class HlsMap(
    val uri: String,
    val byteRange: VesperDownloadByteRange?,
)

private data class HlsSegment(
    val uri: String,
    val duration: String?,
    val byteRange: VesperDownloadByteRange?,
    val sequence: Long,
)

private data class DashPlannedRepresentation(
    val id: String,
    val mediaId: String,
    val mimeType: String?,
    val codecs: String?,
    val bandwidth: String?,
    val baseUri: String,
    val baseUrl: String?,
    val template: DashTemplate?,
)

private data class DashTemplate(
    val media: String,
    val initialization: String?,
    val startNumber: Long,
    val timescale: Long,
    val duration: Long,
)

private fun parseHlsMasterPlaylist(
    manifestUri: String,
    manifestText: String,
): HlsMasterPlaylist {
    val variants = mutableListOf<HlsVariant>()
    val audio = mutableListOf<HlsRendition>()
    var pendingVariant: Map<String, String>? = null

    manifestText.lineSequence().map(String::trim).filter(String::isNotEmpty).forEach { line ->
        when {
            line.startsWith("#EXT-X-STREAM-INF:", ignoreCase = true) -> {
                pendingVariant = parseHlsAttributes(line.substringAfter(':', ""))
            }
            line.startsWith("#EXT-X-MEDIA:", ignoreCase = true) -> {
                val attributes = parseHlsAttributes(line.substringAfter(':', ""))
                val uri = attributes["URI"]
                if (uri != null && attributes["TYPE"]?.equals("AUDIO", ignoreCase = true) == true) {
                    audio += HlsRendition(resolveRemoteReference(manifestUri, uri), attributes)
                }
            }
            line.startsWith("#") -> Unit
            pendingVariant != null -> {
                variants += HlsVariant(resolveRemoteReference(manifestUri, line), pendingVariant.orEmpty())
                pendingVariant = null
            }
        }
    }

    return HlsMasterPlaylist(variants = variants, audio = audio)
}

private fun parseHlsMediaPlaylist(
    playlistUri: String,
    playlistText: String,
): HlsMediaPlaylist {
    var targetDuration: String? = null
    var version: String? = null
    var endList = false
    var playlistTypeVod = false
    var pendingDuration: String? = null
    var pendingByteRange: VesperDownloadByteRange? = null
    var previousRangeEnd = 0L
    var sequence = 0L
    val maps = mutableListOf<HlsMap>()
    val segments = mutableListOf<HlsSegment>()

    playlistText.lineSequence().map(String::trim).filter(String::isNotEmpty).forEach { line ->
        when {
            line.startsWith("#EXT-X-TARGETDURATION:", ignoreCase = true) -> {
                targetDuration = line.substringAfter(':').trim()
            }
            line.startsWith("#EXT-X-VERSION:", ignoreCase = true) -> {
                version = line.substringAfter(':').trim()
            }
            line.equals("#EXT-X-ENDLIST", ignoreCase = true) -> {
                endList = true
            }
            line.startsWith("#EXT-X-PLAYLIST-TYPE:", ignoreCase = true) -> {
                playlistTypeVod = line.substringAfter(':').trim().equals("VOD", ignoreCase = true)
            }
            line.startsWith("#EXT-X-MAP:", ignoreCase = true) -> {
                val attributes = parseHlsAttributes(line.substringAfter(':', ""))
                val uri = attributes["URI"] ?: error("HLS EXT-X-MAP was missing URI")
                val byteRange = attributes["BYTERANGE"]?.let { parseHlsByteRange(it, previousRangeEnd) }
                if (byteRange != null) {
                    previousRangeEnd = byteRange.offset + byteRange.length
                }
                maps += HlsMap(resolveRemoteReference(playlistUri, uri), byteRange)
            }
            line.startsWith("#EXT-X-BYTERANGE:", ignoreCase = true) -> {
                pendingByteRange = parseHlsByteRange(line.substringAfter(':').trim(), previousRangeEnd)
                pendingByteRange?.let { previousRangeEnd = it.offset + it.length }
            }
            line.startsWith("#EXTINF:", ignoreCase = true) -> {
                pendingDuration = line.substringAfter(':').substringBefore(',').trim()
            }
            line.startsWith("#") -> Unit
            else -> {
                sequence += 1
                segments +=
                    HlsSegment(
                        uri = resolveRemoteReference(playlistUri, line),
                        duration = pendingDuration,
                        byteRange = pendingByteRange,
                        sequence = sequence,
                    )
                pendingDuration = null
                pendingByteRange = null
            }
        }
    }

    if (!endList && !playlistTypeVod) {
        error("HLS download preparation requires a VOD playlist or EXT-X-ENDLIST")
    }
    if (segments.isEmpty()) {
        error("HLS media playlist did not contain any segments")
    }
    return HlsMediaPlaylist(
        targetDuration = targetDuration,
        version = version,
        maps = maps,
        segments = segments,
    )
}

private fun parseHlsAttributes(input: String): Map<String, String> {
    val result = linkedMapOf<String, String>()
    var start = 0
    var inQuotes = false
    input.forEachIndexed { index, character ->
        if (character == '"') {
            inQuotes = !inQuotes
        }
        if (character == ',' && !inQuotes) {
            parseAttributePair(input.substring(start, index))?.let { (key, value) -> result[key] = value }
            start = index + 1
        }
    }
    parseAttributePair(input.substring(start))?.let { (key, value) -> result[key] = value }
    return result
}

private fun parseAttributePair(input: String): Pair<String, String>? {
    val key = input.substringBefore('=', "").trim().takeIf { it.isNotEmpty() } ?: return null
    val value = input.substringAfter('=', "").trim().trim('"')
    return key to value
}

private fun parseHlsByteRange(
    value: String,
    previousRangeEnd: Long,
): VesperDownloadByteRange? {
    val lengthText = value.substringBefore('@').trim()
    val offsetText = value.substringAfter('@', "").trim()
    val length = lengthText.toLongOrNull() ?: return null
    val offset = offsetText.toLongOrNull() ?: previousRangeEnd
    return VesperDownloadByteRange(offset = offset, length = length)
}

private fun rewriteHlsMaster(
    variantAttributes: Map<String, String>,
    mediaResourceNames: List<String>,
): String {
    val audioPlaylist = mediaResourceNames.firstOrNull { it.startsWith("audio") }
    val videoPlaylist = mediaResourceNames.firstOrNull { it.startsWith("video") }
        ?: mediaResourceNames.firstOrNull()
        ?: "video.m3u8"
    val bandwidth = variantAttributes["BANDWIDTH"] ?: "1"
    return buildString {
        append("#EXTM3U\n#EXT-X-VERSION:3\n")
        if (audioPlaylist != null) {
            append("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"audio\",DEFAULT=YES,AUTOSELECT=YES,URI=\"")
            append(audioPlaylist)
            append("\"\n")
            append("#EXT-X-STREAM-INF:BANDWIDTH=$bandwidth,AUDIO=\"audio\"\n")
        } else {
            append("#EXT-X-STREAM-INF:BANDWIDTH=$bandwidth\n")
        }
        append(videoPlaylist)
        append('\n')
    }
}

private fun rewriteHlsMedia(
    mediaId: String,
    playlist: HlsMediaPlaylist,
    localMaps: Map<String, String>,
): String =
    buildString {
        append("#EXTM3U\n")
        append("#EXT-X-VERSION:${playlist.version ?: "3"}\n")
        append("#EXT-X-PLAYLIST-TYPE:VOD\n")
        playlist.targetDuration?.let { append("#EXT-X-TARGETDURATION:$it\n") }
        playlist.maps.lastOrNull()?.let { map ->
            localMaps["${map.uri}:${map.byteRange}"]?.let { path ->
                append("#EXT-X-MAP:URI=\"$path\"\n")
            }
        }
        playlist.segments.forEach { segment ->
            append("#EXTINF:${segment.duration ?: "0"},\n")
            append("segments/$mediaId-${segment.sequence.toString().padStart(5, '0')}.${extensionFromUri(segment.uri, "ts")}\n")
        }
        append("#EXT-X-ENDLIST\n")
    }

private fun parseXmlDocument(xmlText: String) =
    DocumentBuilderFactory
        .newInstance()
        .apply { isNamespaceAware = true }
        .newDocumentBuilder()
        .parse(InputSource(StringReader(xmlText)))

private fun selectDashRepresentations(
    document: org.w3c.dom.Document,
    manifestUri: String,
    profile: VesperDownloadProfile,
): List<DashPlannedRepresentation> {
    val mpdBase = childElementsByTagName(document.documentElement, "BaseURL")
        .firstOrNull()
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }
        ?.let { resolveRemoteReference(manifestUri, it) }
        ?: manifestUri
    val result = mutableListOf<DashPlannedRepresentation>()
    val adaptationSets = document.getElementsByTagNameNS("*", "AdaptationSet")
    for (index in 0 until adaptationSets.length) {
        val adaptation = adaptationSets.item(index) as? Element ?: continue
        val adaptationMimeType = adaptation.getAttribute("mimeType").takeIf(String::isNotBlank)
        if (adaptationMimeType != null &&
            !adaptationMimeType.startsWith("video/") &&
            !adaptationMimeType.startsWith("audio/")
        ) {
            continue
        }
        val adaptationBase = childElementsByTagName(adaptation, "BaseURL")
            .firstOrNull()
            ?.textContent
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
            ?.let { resolveRemoteReference(mpdBase, it) }
            ?: mpdBase
        val adaptationTemplate = dashTemplateFromElement(adaptation)
        val representations = childElementsByTagName(adaptation, "Representation")
        val selectedRepresentation =
            profile.variantId
                ?.let { variantId -> representations.firstOrNull { it.getAttribute("id") == variantId } }
                ?: representations.firstOrNull()
                ?: continue
        val id = selectedRepresentation.getAttribute("id").takeIf(String::isNotBlank) ?: index.toString()
        val representationBase = childElementsByTagName(selectedRepresentation, "BaseURL")
            .firstOrNull()
            ?.textContent
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
        val template = dashTemplateFromElement(selectedRepresentation) ?: adaptationTemplate
        val mimeType = selectedRepresentation.getAttribute("mimeType").takeIf(String::isNotBlank) ?: adaptationMimeType
        val mediaKind = when {
            mimeType?.startsWith("audio/") == true -> "audio"
            mimeType?.startsWith("video/") == true -> "video"
            else -> "media"
        }
        result +=
            DashPlannedRepresentation(
                id = id,
                mediaId = "$mediaKind$index",
                mimeType = mimeType,
                codecs = selectedRepresentation.getAttribute("codecs").takeIf(String::isNotBlank),
                bandwidth = selectedRepresentation.getAttribute("bandwidth").takeIf(String::isNotBlank),
                baseUri = representationBase?.let { resolveRemoteReference(adaptationBase, it) } ?: adaptationBase,
                baseUrl = if (template == null) representationBase else null,
                template = template,
            )
    }
    return result
}

private fun dashTemplateFromElement(element: Element): DashTemplate? {
    val template = childElementsByTagName(element, "SegmentTemplate").firstOrNull() ?: return null
    val media = template.getAttribute("media").takeIf(String::isNotBlank) ?: return null
    return DashTemplate(
        media = media,
        initialization = template.getAttribute("initialization").takeIf(String::isNotBlank),
        startNumber = template.getAttribute("startNumber").toLongOrNull() ?: 1L,
        timescale = template.getAttribute("timescale").toLongOrNull() ?: 1L,
        duration = template.getAttribute("duration").toLongOrNull() ?: 0L,
    )
}

private fun childElementsByTagName(
    parent: Element,
    tagName: String,
): List<Element> =
    buildList {
        val children = parent.childNodes
        for (index in 0 until children.length) {
            val child = children.item(index) as? Element ?: continue
            if (child.localName == tagName || child.tagName == tagName) {
                add(child)
            }
        }
    }

private fun parseIso8601DurationSeconds(value: String?): Double? {
    if (value.isNullOrBlank() || !value.startsWith("PT")) {
        return null
    }
    var number = ""
    var total = 0.0
    value.drop(2).forEach { character ->
        if (character.isDigit() || character == '.') {
            number += character
            return@forEach
        }
        val parsed = number.toDoubleOrNull() ?: return null
        number = ""
        when (character) {
            'H' -> total += parsed * 3600.0
            'M' -> total += parsed * 60.0
            'S' -> total += parsed
            else -> return null
        }
    }
    return total.takeIf { it > 0.0 }
}

private fun expandDashTemplate(
    template: String,
    representationId: String,
    number: Long,
): String =
    replaceDashNumberToken(template.replace("\$RepresentationID\$", representationId), number)

private fun replaceDashNumberToken(
    value: String,
    number: Long,
): String {
    var output = value.replace("\$Number\$", number.toString())
    while (true) {
        val start = output.indexOf("\$Number%")
        if (start < 0) {
            return output
        }
        val end = output.indexOf('$', start + "\$Number%".length)
        if (end < 0) {
            return output
        }
        val spec = output.substring(start + "\$Number%".length, end)
        val width = spec.removeSuffix("d").removePrefix("0").toIntOrNull() ?: 0
        output = output.replaceRange(start, end + 1, number.toString().padStart(width, '0'))
    }
}

private fun rewriteDashMpd(
    duration: String?,
    adaptationSets: List<String>,
): String =
    buildString {
        append("<MPD type=\"static\"")
        duration?.takeIf { it.isNotBlank() }?.let { append(" mediaPresentationDuration=\"").append(escapeXml(it)).append('"') }
        append(" xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period>")
        adaptationSets.forEach(::append)
        append("</Period></MPD>\n")
    }

private fun rewriteDashTemplateAdaptationSet(
    representation: DashPlannedRepresentation,
    template: DashTemplate,
    mediaId: String,
    segmentCount: Long,
): String {
    val mime = representation.mimeType?.let { " mimeType=\"${escapeXml(it)}\"" }.orEmpty()
    val codecs = representation.codecs?.let { " codecs=\"${escapeXml(it)}\"" }.orEmpty()
    val bandwidth = representation.bandwidth ?: "1"
    val initialization = template.initialization?.let { " initialization=\"segments/$mediaId-init.mp4\"" }.orEmpty()
    return "<AdaptationSet$mime><Representation id=\"${escapeXml(representation.id)}\" bandwidth=\"$bandwidth\"$codecs><SegmentTemplate timescale=\"${template.timescale}\" duration=\"${template.duration}\" startNumber=\"${template.startNumber}\"$initialization media=\"segments/$mediaId-\$Number\$.m4s\" /></Representation></AdaptationSet><!-- plannedSegments=$segmentCount -->"
}

private fun rewriteDashSegmentBaseAdaptationSet(
    representation: DashPlannedRepresentation,
    localName: String,
): String {
    val mime = representation.mimeType?.let { " mimeType=\"${escapeXml(it)}\"" }.orEmpty()
    val codecs = representation.codecs?.let { " codecs=\"${escapeXml(it)}\"" }.orEmpty()
    val bandwidth = representation.bandwidth ?: "1"
    return "<AdaptationSet$mime><Representation id=\"${escapeXml(representation.id)}\" bandwidth=\"$bandwidth\"$codecs><BaseURL>${escapeXml(localName)}</BaseURL><SegmentBase /></Representation></AdaptationSet>"
}

private fun parseFlvClipManifest(
    baseUri: String,
    manifestText: String,
): List<String> =
    manifestText
        .lineSequence()
        .map(String::trim)
        .filter { it.isNotEmpty() && !it.startsWith("#") && !it.equals("ffconcat version 1.0", ignoreCase = true) }
        .map { line ->
            line.removePrefix("file")
                .trim()
                .trim('\'', '"')
        }
        .filter(String::isNotEmpty)
        .map { resolveRemoteReference(baseUri, it) }
        .toList()

private fun resolveRemoteReference(
    baseUri: String,
    reference: String,
): String =
    runCatching {
        val ref = URI(reference)
        if (ref.isAbsolute || baseUri.isBlank()) {
            ref.toString()
        } else {
            URI(baseUri).resolve(ref).toString()
        }
    }.getOrElse { reference }

private fun extensionFromUri(
    uri: String,
    fallback: String,
): String {
    val name = lastPathSegmentFromUri(uri) ?: return fallback
    return name.substringAfterLast('.', "").takeIf { it.isNotBlank() && it != name } ?: fallback
}

private fun escapeXml(value: String): String =
    value
        .replace("&", "&amp;")
        .replace("\"", "&quot;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")

private fun escapeFfconcatPath(path: String): String = path.replace("'", "'\\''")

private class VesperGeneratedDownloadResourceMaterializer(
    private val baseDirectory: File?,
    private val fallbackBaseDirectory: File?,
) {
    fun materialize(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex,
    ): VesperDownloadAssetIndex {
        if (assetIndex.resources.none { it.generatedText != null }) {
            return assetIndex.compactedForPersistence()
        }
        val taskDirectory = taskBaseDirectory(assetId, taskId, profile)
        val generatedDirectory = File(taskDirectory, ".generated")
        check(generatedDirectory.mkdirs() || generatedDirectory.isDirectory) {
            "failed to create generated download resource directory ${generatedDirectory.absolutePath}"
        }
        val usedNames = linkedSetOf<String>()
        val resources =
            assetIndex.resources.map { resource ->
                val generatedText = resource.generatedText ?: return@map resource
                val data = generatedText.toByteArray(Charsets.UTF_8)
                val fileName = uniqueGeneratedFileName(resource, usedNames)
                val destination = File(generatedDirectory, fileName)
                runCatching {
                    destination.writeBytes(data)
                }.getOrElse { error ->
                    throw IllegalStateException(
                        "failed to persist generated download resource ${resource.resourceId}: ${error.message}",
                        error,
                    )
                }
                resource.copy(
                    uri = destination.toURI().toString(),
                    generatedText = null,
                    sizeBytes = data.size.toLong(),
                )
            }
        return assetIndex.copy(
            totalSizeBytes = recomputeTotalSizeBytes(assetIndex.totalSizeBytes, resources, assetIndex.segments),
            resources = resources,
        )
    }

    private fun taskBaseDirectory(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
    ): File =
        profile.targetDirectory
            ?.takeIf { it.isNotBlank() }
            ?.let(::File)
            ?: File(
                baseDirectory ?: fallbackBaseDirectory ?: File("vesper-downloads"),
                assetId.ifBlank { taskId?.toString() ?: "asset" },
            )

    private fun uniqueGeneratedFileName(
        resource: VesperDownloadResourceRecord,
        usedNames: MutableSet<String>,
    ): String {
        val baseName = generatedBaseName(resource)
        if (usedNames.add(baseName)) {
            return baseName
        }
        val hashed = appendStableHash(baseName, stableShortHash("${resource.resourceId}|${resource.relativePath}|${resource.uri}"))
        usedNames.add(hashed)
        return hashed
    }

    private fun generatedBaseName(resource: VesperDownloadResourceRecord): String {
        val raw = resource.relativePath?.substringAfterLast('/')?.substringAfterLast('\\') ?: resource.resourceId
        val sanitized = raw.replace(Regex("[^A-Za-z0-9._-]+"), "_").trim('.', ' ')
        return sanitized.takeIf { it.isNotBlank() && it != ".." }
            ?: "resource-${stableShortHash(resource.resourceId.ifBlank { resource.uri })}"
    }

    private fun appendStableHash(
        fileName: String,
        hash: String,
    ): String {
        val extension = fileName.substringAfterLast('.', "")
        val stem = if (extension.isBlank() || extension == fileName) fileName else fileName.removeSuffix(".$extension")
        return if (extension.isBlank() || extension == fileName) "$stem-$hash" else "$stem-$hash.$extension"
    }

    private fun stableShortHash(value: String): String {
        var hash = -3750763034362895579L
        value.toByteArray(Charsets.UTF_8).forEach { byte ->
            hash = hash xor (byte.toLong() and 0xffL)
            hash *= 1099511628211L
        }
        return java.lang.Long.toUnsignedString(hash, 16).takeLast(8)
    }

    private fun recomputeTotalSizeBytes(
        original: Long?,
        resources: List<VesperDownloadResourceRecord>,
        segments: List<VesperDownloadSegmentRecord>,
    ): Long? {
        var total = 0L
        resources.forEach { total += it.sizeBytes ?: return original }
        segments.forEach { total += it.sizeBytes ?: return original }
        return total
    }
}

private fun VesperDownloadResourceRecord.compactedForPersistence(): VesperDownloadResourceRecord =
    copy(generatedText = null)

private fun VesperDownloadAssetIndex.compactedForPersistence(): VesperDownloadAssetIndex =
    copy(resources = resources.map(VesperDownloadResourceRecord::compactedForPersistence))

private fun VesperDownloadSnapshot.compactedForPersistence(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(tasks = tasks.map { it.copy(assetIndex = it.assetIndex.compactedForPersistence()) })

private fun VesperDownloadConfiguration.toNativePayload(): NativeDownloadConfig =
    NativeDownloadConfig(
        autoStart = autoStart,
        runPostProcessorsOnCompletion = runPostProcessorsOnCompletion,
        pluginLibraryPaths = pluginLibraryPaths.toTypedArray(),
    )

private fun VesperDownloadSource.toNativePayload(): NativeDownloadSource =
    sanitizeDownloadRequestHeaders(source.headers).let { headers ->
        NativeDownloadSource(
            sourceUri = source.uri,
            contentFormatOrdinal = contentFormat.ordinal,
            manifestUri = manifestUri,
            headerNames = headers.keys.toTypedArray(),
            headerValues = headers.values.toTypedArray(),
        )
    }

private fun VesperDownloadProfile.toNativePayload(): NativeDownloadProfile =
    NativeDownloadProfile(
        variantId = variantId,
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectedTrackIds = selectedTrackIds.toTypedArray(),
        targetOutputFormatOrdinal = targetOutputFormat?.ordinal ?: -1,
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
        streams = streams.map(VesperDownloadAssetStream::toNativePayload).toTypedArray(),
        completedPath = completedPath,
    )

private fun VesperDownloadResourceRecord.toNativePayload(): NativeDownloadResourceRecord =
    NativeDownloadResourceRecord(
        resourceId = resourceId,
        uri = uri,
        relativePath = relativePath,
        byteRange = byteRange?.toNativePayload(),
        generatedText = generatedText,
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
        byteRange = byteRange?.toNativePayload(),
        hasSizeBytes = sizeBytes != null,
        sizeBytes = sizeBytes ?: 0L,
        checksum = checksum,
    )

private fun VesperDownloadAssetStream.toNativePayload(): NativeDownloadAssetStream =
    NativeDownloadAssetStream(
        streamId = streamId,
        kindOrdinal = kind.ordinal,
        language = language,
        codec = codec,
        label = label,
        hasQualityRank = qualityRank != null,
        qualityRank = qualityRank ?: 0,
        resourceIds = resourceIds.toTypedArray(),
        segmentIds = segmentIds.toTypedArray(),
        metadataKeys = metadata.entries.map { it.key }.toTypedArray(),
        metadataValues = metadata.entries.map { it.value }.toTypedArray(),
    )

private fun VesperDownloadByteRange.toNativePayload(): NativeDownloadByteRange =
    NativeDownloadByteRange(offset = offset, length = length)

private fun VesperDownloadProgressSnapshot.toNativePayload(): NativeDownloadProgress =
    NativeDownloadProgress(
        receivedBytes = receivedBytes,
        hasTotalBytes = totalBytes != null,
        totalBytes = totalBytes ?: 0L,
        receivedSegments = receivedSegments,
        hasTotalSegments = totalSegments != null,
        totalSegments = totalSegments ?: 0,
    )

private fun VesperDownloadTaskSnapshot.toNativePayload(): NativeDownloadTask =
    NativeDownloadTask(
        taskId = taskId,
        assetId = assetId,
        source = source.toNativePayload(),
        profile = profile.toNativePayload(),
        statusOrdinal = state.ordinal,
        progress = progress.toNativePayload(),
        assetIndex = assetIndex.toNativePayload(),
        hasError = error != null,
        errorCodeOrdinal = error?.codeOrdinal ?: 0,
        errorCategoryOrdinal = error?.categoryOrdinal ?: 0,
        errorRetriable = error?.retriable ?: false,
        errorMessage = error?.message,
    )

private fun NativeDownloadSnapshot.toPublic(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(tasks = tasks.map(NativeDownloadTask::toPublic))

private fun NativeDownloadTask.toPublic(): VesperDownloadTaskSnapshot =
    VesperDownloadTaskSnapshot(
        taskId = taskId,
        assetId = assetId,
        source = source.toPublic(),
        profile = profile.toPublic(),
        state = statusOrdinal.toDownloadState(),
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

private fun Int.toDownloadState(): VesperDownloadState =
    when (this) {
        0 -> VesperDownloadState.Queued
        1 -> VesperDownloadState.Preparing
        2 -> VesperDownloadState.Downloading
        3 -> VesperDownloadState.Paused
        4 -> VesperDownloadState.Completed
        5 -> VesperDownloadState.Failed
        6 -> VesperDownloadState.Removed
        else -> VesperDownloadState.Queued
    }

private fun NativeDownloadSource.toPublic(): VesperDownloadSource =
    downloadSourceHeaders().let { headers ->
        VesperDownloadSource(
            source =
                when {
                    sourceUri.startsWith("content://", ignoreCase = true) ||
                        sourceUri.startsWith("file://", ignoreCase = true) -> {
                        VesperPlayerSource.local(
                            uri = sourceUri,
                            label = Uri.parse(sourceUri).lastPathSegment ?: sourceUri,
                            headers = headers,
                        )
                    }
                    else -> {
                        VesperPlayerSource.remote(uri = sourceUri, label = sourceUri, headers = headers)
                    }
                },
            contentFormat =
                when (contentFormatOrdinal) {
                    0 -> VesperDownloadContentFormat.HlsSegments
                    1 -> VesperDownloadContentFormat.DashSegments
                    2 -> VesperDownloadContentFormat.FlvSegments
                    3 -> VesperDownloadContentFormat.SingleFile
                    else -> VesperDownloadContentFormat.Unknown
                },
            manifestUri = manifestUri,
        )
    }

private fun NativeDownloadProfile.toPublic(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = variantId,
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectedTrackIds = selectedTrackIds.toList(),
        targetOutputFormat =
            when (targetOutputFormatOrdinal) {
                0 -> VesperDownloadOutputFormat.Mp4
                1 -> VesperDownloadOutputFormat.Mkv
                2 -> VesperDownloadOutputFormat.Original
                else -> null
            },
        targetDirectory = targetDirectory,
        allowMeteredNetwork = allowMeteredNetwork,
    )

private fun NativeDownloadAssetIndex.toPublic(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat =
            when (contentFormatOrdinal) {
                0 -> VesperDownloadContentFormat.HlsSegments
                1 -> VesperDownloadContentFormat.DashSegments
                2 -> VesperDownloadContentFormat.FlvSegments
                3 -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        version = version,
        etag = etag,
        checksum = checksum,
        totalSizeBytes = if (hasTotalSizeBytes) totalSizeBytes else null,
        resources = resources.map(NativeDownloadResourceRecord::toPublic),
        segments = segments.map(NativeDownloadSegmentRecord::toPublic),
        streams = streams.map(NativeDownloadAssetStream::toPublic),
        completedPath = completedPath,
    )

private fun NativeDownloadResourceRecord.toPublic(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = resourceId,
        uri = uri,
        relativePath = relativePath,
        byteRange = byteRange?.toPublic(),
        generatedText = null,
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
        byteRange = byteRange?.toPublic(),
        sizeBytes = if (hasSizeBytes) sizeBytes else null,
        checksum = checksum,
    )

private fun NativeDownloadAssetStream.toPublic(): VesperDownloadAssetStream =
    VesperDownloadAssetStream(
        streamId = streamId,
        kind = VesperDownloadStreamKind.entries.getOrElse(kindOrdinal) { VesperDownloadStreamKind.Combined },
        language = language,
        codec = codec,
        label = label,
        qualityRank = if (hasQualityRank) qualityRank else null,
        resourceIds = resourceIds.toList(),
        segmentIds = segmentIds.toList(),
        metadata = metadataKeys.zip(metadataValues).toMap(),
    )

private fun NativeDownloadByteRange.toPublic(): VesperDownloadByteRange =
    VesperDownloadByteRange(offset = offset, length = length)

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
        is NativeDownloadEvent.StateChanged ->
            VesperDownloadEvent.StateChanged(
                VesperDownloadTaskStatePatch(
                    taskId = taskId,
                    state = statusOrdinal.toDownloadState(),
                    progress = progress.toPublic(),
                    error =
                        if (hasError) {
                            VesperDownloadError(
                                codeOrdinal = errorCodeOrdinal,
                                categoryOrdinal = errorCategoryOrdinal,
                                retriable = errorRetriable,
                                message = errorMessage.orEmpty(),
                            )
                        } else {
                            null
                        },
                    completedPath = completedPath,
                ),
            )
        is NativeDownloadEvent.AssetIndexUpdated -> VesperDownloadEvent.AssetIndexUpdated(task.toPublic())
        is NativeDownloadEvent.ProgressUpdated ->
            VesperDownloadEvent.ProgressUpdated(
                VesperDownloadTaskProgressPatch(
                    taskId = taskId,
                    progress = progress.toPublic(),
                ),
            )
    }

private fun VesperDownloadTaskSnapshot.toJson(): JSONObject =
    JSONObject().apply {
        put("taskId", taskId)
        put("assetId", assetId)
        put("source", source.toJson())
        put("profile", profile.toJson())
        put("state", state.ordinal)
        put("progress", progress.toJson())
        put("assetIndex", assetIndex.toJson())
        put("error", error?.toJson())
    }

private fun JSONObject.toDownloadTask(): VesperDownloadTaskSnapshot =
    VesperDownloadTaskSnapshot(
        taskId = optLong("taskId", 0L),
        assetId = optString("assetId", ""),
        source = optJSONObject("source")?.toDownloadSource() ?: VesperDownloadSource(
            source = VesperPlayerSource.remote("", ""),
            contentFormat = VesperDownloadContentFormat.Unknown,
        ),
        profile = optJSONObject("profile")?.toDownloadProfile() ?: VesperDownloadProfile(),
        state = enumValue<VesperDownloadState>(optInt("state", VesperDownloadState.Paused.ordinal)),
        progress = optJSONObject("progress")?.toDownloadProgress() ?: VesperDownloadProgressSnapshot(),
        assetIndex = optJSONObject("assetIndex")?.toDownloadAssetIndex() ?: VesperDownloadAssetIndex(),
        error = optJSONObject("error")?.toDownloadError(),
    )

private fun VesperDownloadSnapshot.toJson(): JSONObject =
    JSONObject().apply {
        put(
            "tasks",
            JSONArray().apply {
                tasks.forEach { put(it.toJson()) }
            },
        )
    }

private fun JSONObject.toDownloadSnapshot(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(
        tasks =
            optJSONArray("tasks")
                ?.toObjectList { toDownloadTask() }
                ?: emptyList(),
    )

private fun VesperDownloadSource.toJson(): JSONObject =
    JSONObject().apply {
        put("source", source.toJson())
        put("contentFormat", contentFormat.ordinal)
        put("manifestUri", manifestUri)
    }

private fun JSONObject.toDownloadSource(): VesperDownloadSource =
    VesperDownloadSource(
        source = optJSONObject("source")?.toPlayerSource() ?: VesperPlayerSource.remote("", ""),
        contentFormat = enumValue(optInt("contentFormat", VesperDownloadContentFormat.Unknown.ordinal)),
        manifestUri = optStringOrNull("manifestUri"),
    )

private fun VesperPlayerSource.toJson(): JSONObject =
    JSONObject().apply {
        put("uri", uri)
        put("label", label)
        put("kind", kind.ordinal)
        put("protocol", protocol.ordinal)
        put(
            "headers",
            JSONObject().apply {
                headers.forEach { (key, value) -> put(key, value) }
            },
        )
    }

private fun JSONObject.toPlayerSource(): VesperPlayerSource =
    VesperPlayerSource(
        uri = optString("uri", ""),
        label = optString("label", ""),
        kind = enumValue(optInt("kind", VesperPlayerSourceKind.Remote.ordinal)),
        protocol = enumValue(optInt("protocol", VesperPlayerSourceProtocol.Unknown.ordinal)),
        headers =
            optJSONObject("headers")
                ?.keys()
                ?.asSequence()
                ?.associateWith { key -> optJSONObject("headers")?.optString(key, "") ?: "" }
                ?: emptyMap(),
    )

private fun VesperDownloadProfile.toJson(): JSONObject =
    JSONObject().apply {
        put("variantId", variantId)
        put("preferredAudioLanguage", preferredAudioLanguage)
        put("preferredSubtitleLanguage", preferredSubtitleLanguage)
        put("selectedTrackIds", JSONArray().apply { selectedTrackIds.forEach(::put) })
        put("targetOutputFormat", targetOutputFormat?.ordinal ?: -1)
        put("targetDirectory", targetDirectory)
        put("allowMeteredNetwork", allowMeteredNetwork)
    }

private fun JSONObject.toDownloadProfile(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = optStringOrNull("variantId"),
        preferredAudioLanguage = optStringOrNull("preferredAudioLanguage"),
        preferredSubtitleLanguage = optStringOrNull("preferredSubtitleLanguage"),
        selectedTrackIds = optJSONArray("selectedTrackIds")?.toStringList() ?: emptyList(),
        targetOutputFormat =
            optInt("targetOutputFormat", -1)
                .takeIf { it >= 0 }
                ?.let { enumValue<VesperDownloadOutputFormat>(it) },
        targetDirectory = optStringOrNull("targetDirectory"),
        allowMeteredNetwork = optBoolean("allowMeteredNetwork", false),
    )

private fun VesperDownloadProgressSnapshot.toJson(): JSONObject =
    JSONObject().apply {
        put("receivedBytes", receivedBytes)
        put("totalBytes", totalBytes)
        put("receivedSegments", receivedSegments)
        put("totalSegments", totalSegments)
    }

private fun JSONObject.toDownloadProgress(): VesperDownloadProgressSnapshot =
    VesperDownloadProgressSnapshot(
        receivedBytes = optLong("receivedBytes", 0L),
        totalBytes = optLongOrNull("totalBytes"),
        receivedSegments = optInt("receivedSegments", 0),
        totalSegments = optIntOrNull("totalSegments"),
    )

private fun VesperDownloadAssetIndex.toJson(): JSONObject =
    JSONObject().apply {
        put("contentFormat", contentFormat.ordinal)
        put("version", version)
        put("etag", etag)
        put("checksum", checksum)
        put("totalSizeBytes", totalSizeBytes)
        put("resources", JSONArray().apply { resources.forEach { put(it.toJson()) } })
        put("segments", JSONArray().apply { segments.forEach { put(it.toJson()) } })
        put("streams", JSONArray().apply { streams.forEach { put(it.toJson()) } })
        put("completedPath", completedPath)
    }

private fun JSONObject.toDownloadAssetIndex(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat = enumValue(optInt("contentFormat", VesperDownloadContentFormat.Unknown.ordinal)),
        version = optStringOrNull("version"),
        etag = optStringOrNull("etag"),
        checksum = optStringOrNull("checksum"),
        totalSizeBytes = optLongOrNull("totalSizeBytes"),
        resources = optJSONArray("resources")?.toObjectList { toDownloadResource() } ?: emptyList(),
        segments = optJSONArray("segments")?.toObjectList { toDownloadSegment() } ?: emptyList(),
        streams = optJSONArray("streams")?.toObjectList { toDownloadAssetStream() } ?: emptyList(),
        completedPath = optStringOrNull("completedPath"),
    )

private fun VesperDownloadResourceRecord.toJson(): JSONObject =
    JSONObject().apply {
        put("resourceId", resourceId)
        put("uri", uri)
        put("relativePath", relativePath)
        put("byteRange", byteRange?.toJson())
        put("generatedText", null)
        put("sizeBytes", sizeBytes)
        put("etag", etag)
        put("checksum", checksum)
    }

private fun JSONObject.toDownloadResource(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = optString("resourceId", ""),
        uri = optString("uri", ""),
        relativePath = optStringOrNull("relativePath"),
        byteRange = optJSONObject("byteRange")?.toDownloadByteRange(),
        generatedText = optStringOrNull("generatedText"),
        sizeBytes = optLongOrNull("sizeBytes"),
        etag = optStringOrNull("etag"),
        checksum = optStringOrNull("checksum"),
    )

private fun VesperDownloadSegmentRecord.toJson(): JSONObject =
    JSONObject().apply {
        put("segmentId", segmentId)
        put("uri", uri)
        put("relativePath", relativePath)
        put("sequence", sequence)
        put("byteRange", byteRange?.toJson())
        put("sizeBytes", sizeBytes)
        put("checksum", checksum)
    }

private fun JSONObject.toDownloadSegment(): VesperDownloadSegmentRecord =
    VesperDownloadSegmentRecord(
        segmentId = optString("segmentId", ""),
        uri = optString("uri", ""),
        relativePath = optStringOrNull("relativePath"),
        sequence = optLongOrNull("sequence"),
        byteRange = optJSONObject("byteRange")?.toDownloadByteRange(),
        sizeBytes = optLongOrNull("sizeBytes"),
        checksum = optStringOrNull("checksum"),
    )

private fun VesperDownloadAssetStream.toJson(): JSONObject =
    JSONObject().apply {
        put("streamId", streamId)
        put("kind", kind.ordinal)
        put("language", language)
        put("codec", codec)
        put("label", label)
        put("qualityRank", qualityRank)
        put("resourceIds", JSONArray().apply { resourceIds.forEach(::put) })
        put("segmentIds", JSONArray().apply { segmentIds.forEach(::put) })
        put("metadata", JSONObject().apply { metadata.forEach { (key, value) -> put(key, value) } })
    }

private fun JSONObject.toDownloadAssetStream(): VesperDownloadAssetStream =
    VesperDownloadAssetStream(
        streamId = optString("streamId", ""),
        kind = enumValue(optInt("kind", VesperDownloadStreamKind.Combined.ordinal)),
        language = optStringOrNull("language"),
        codec = optStringOrNull("codec"),
        label = optStringOrNull("label"),
        qualityRank = optIntOrNull("qualityRank"),
        resourceIds = optJSONArray("resourceIds")?.toStringList() ?: emptyList(),
        segmentIds = optJSONArray("segmentIds")?.toStringList() ?: emptyList(),
        metadata = optJSONObject("metadata")?.toStringMap() ?: emptyMap(),
    )

private fun VesperDownloadByteRange.toJson(): JSONObject =
    JSONObject().apply {
        put("offset", offset)
        put("length", length)
    }

private fun JSONObject.toDownloadByteRange(): VesperDownloadByteRange =
    VesperDownloadByteRange(offset = optLong("offset", 0L), length = optLong("length", 0L))

private fun VesperDownloadError.toJson(): JSONObject =
    JSONObject().apply {
        put("codeOrdinal", codeOrdinal)
        put("categoryOrdinal", categoryOrdinal)
        put("retriable", retriable)
        put("message", message)
    }

private fun JSONObject.toDownloadError(): VesperDownloadError =
    VesperDownloadError(
        codeOrdinal = optInt("codeOrdinal", 0),
        categoryOrdinal = optInt("categoryOrdinal", 0),
        retriable = optBoolean("retriable", false),
        message = optString("message", "download failed"),
    )

private inline fun <reified T : Enum<T>> enumValue(ordinal: Int): T =
    enumValues<T>().getOrElse(ordinal) { enumValues<T>().last() }

private fun JSONObject.optStringOrNull(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotEmpty)

private fun JSONObject.optLongOrNull(key: String): Long? =
    if (isNull(key) || !has(key)) null else optLong(key)

private fun JSONObject.optIntOrNull(key: String): Int? =
    if (isNull(key) || !has(key)) null else optInt(key)

private fun JSONArray.toStringList(): List<String> =
    buildList {
        for (index in 0 until length()) {
            optString(index).takeIf(String::isNotEmpty)?.let(::add)
        }
    }

private fun JSONObject.toStringMap(): Map<String, String> =
    buildMap {
        keys().forEach { key ->
            optString(key).takeIf(String::isNotEmpty)?.let { put(key, it) }
        }
    }

private fun <T> JSONArray.toObjectList(transform: JSONObject.() -> T): List<T> =
    buildList {
        for (index in 0 until length()) {
            optJSONObject(index)?.let { add(it.transform()) }
        }
    }

private fun sanitizeDownloadRequestHeaders(headers: Map<String, String>): Map<String, String> =
    headers
        .mapNotNull { (name, value) ->
            val sanitizedName = name.trim().takeIf { it.isNotEmpty() } ?: return@mapNotNull null
            val sanitizedValue = value.takeIf { it.isNotBlank() } ?: return@mapNotNull null
            sanitizedName to sanitizedValue
        }
        .toMap()

private fun sanitizedOutputFileName(value: String): String {
    val sanitized = value.replace(Regex("[^A-Za-z0-9._ -]+"), "_").trim('.', ' ')
    return sanitized.takeIf { it.isNotBlank() && it != ".." } ?: "vesper-download"
}

private fun guessMimeType(file: File): String {
    val extension = file.extension.takeIf { it.isNotBlank() } ?: return "application/octet-stream"
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension.lowercase())
        ?: when (extension.lowercase()) {
            "m3u8" -> "application/vnd.apple.mpegurl"
            "mpd" -> "application/dash+xml"
            "mp4" -> "video/mp4"
            "mkv" -> "video/x-matroska"
            "ts" -> "video/mp2t"
            else -> "application/octet-stream"
        }
}

private val VesperDownloadPublicCollection.relativePath: String
    get() =
        when (this) {
            VesperDownloadPublicCollection.Downloads -> Environment.DIRECTORY_DOWNLOADS
            VesperDownloadPublicCollection.Movies -> Environment.DIRECTORY_MOVIES
        }

private val VesperDownloadPublicCollection.contentUri: Uri
    get() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            error("MediaStore public collection output requires Android 10 or newer")
        }
        return when (this) {
            VesperDownloadPublicCollection.Downloads -> MediaStore.Downloads.EXTERNAL_CONTENT_URI
            VesperDownloadPublicCollection.Movies -> MediaStore.Video.Media.EXTERNAL_CONTENT_URI
        }
    }

private fun NativeDownloadSource.downloadSourceHeaders(): Map<String, String> =
    sanitizeDownloadRequestHeaders(
        headerNames
            .zip(headerValues)
            .toMap(),
    )

private fun HttpURLConnection.applyDownloadRequestHeaders(headers: Map<String, String>) {
    sanitizeDownloadRequestHeaders(headers).forEach { (name, value) ->
        setRequestProperty(name, value)
    }
}

private fun DataSpec.Builder.setDownloadRequestHeaders(headers: Map<String, String>): DataSpec.Builder =
    setHttpRequestHeaders(sanitizeDownloadRequestHeaders(headers))

private fun isHttpUri(uri: String): Boolean {
    val scheme = uriScheme(uri) ?: return false
    return scheme.equals("http", ignoreCase = true) || scheme.equals("https", ignoreCase = true)
}

private fun uriScheme(uri: String): String? =
    runCatching { URI(uri).scheme }.getOrNull()

private fun lastPathSegmentFromUri(uri: String): String? =
    runCatching {
        URI(uri).path
            ?.substringAfterLast('/')
            ?.takeIf { it.isNotBlank() }
    }.getOrNull()

private fun requestedHttpRangeHeader(
    byteRange: VesperDownloadByteRange?,
    resumeOffset: Long,
): String? {
    val offset = resumeOffset.coerceAtLeast(0L)
    if (byteRange != null) {
        val remaining = byteRange.length.coerceAtLeast(0L) - offset
        if (remaining <= 0L) {
            return null
        }
        val start = byteRange.offset.coerceAtLeast(0L) + offset
        val end = start + remaining - 1L
        return "bytes=$start-$end"
    }
    return if (offset > 0L) "bytes=$offset-" else null
}

private fun requestedHttpRangeStart(
    byteRange: VesperDownloadByteRange?,
    resumeOffset: Long,
): Long? {
    val offset = resumeOffset.coerceAtLeast(0L)
    return when {
        byteRange != null -> byteRange.offset.coerceAtLeast(0L) + offset
        offset > 0L -> offset
        else -> null
    }
}

private fun parseHttpContentRangeStart(contentRange: String?): Long? {
    val range = contentRange?.substringAfter(' ', "")?.takeIf { it.isNotBlank() } ?: return null
    if (range.startsWith("*")) {
        return null
    }
    return range.substringBefore('-').toLongOrNull()
}

private fun isExpiredHttpStatus(status: Int): Boolean =
    status == HttpURLConnection.HTTP_UNAUTHORIZED ||
        status == HttpURLConnection.HTTP_FORBIDDEN ||
        status == HttpURLConnection.HTTP_NOT_FOUND ||
        status == HttpURLConnection.HTTP_GONE

private class VesperStaleDownloadResourceException(
    message: String,
    val resourceId: String? = null,
    val segmentId: String? = null,
    val uri: String? = null,
    val phase: VesperDownloadStaleResourcePhase? = null,
    val statusCode: Int? = null,
    val receivedBytes: Long = 0L,
) : IllegalStateException(message) {
    fun toStaleResource(
        taskId: VesperDownloadTaskId,
        fallbackResourceId: String? = null,
        fallbackSegmentId: String? = null,
        fallbackUri: String? = null,
        fallbackPhase: VesperDownloadStaleResourcePhase,
        fallbackReceivedBytes: Long = 0L,
    ): VesperDownloadStaleResource =
        VesperDownloadStaleResource(
            taskId = taskId,
            resourceId = resourceId ?: fallbackResourceId,
            segmentId = segmentId ?: fallbackSegmentId,
            uri = uri ?: fallbackUri,
            phase = phase ?: fallbackPhase,
            statusCode = statusCode,
            receivedBytes = receivedBytes.takeIf { it > 0L } ?: fallbackReceivedBytes,
            message = message ?: "offline download resource is stale or expired",
        )
}

private fun staleDownloadResource(
    message: String,
    resourceId: String? = null,
    segmentId: String? = null,
    uri: String? = null,
    phase: VesperDownloadStaleResourcePhase? = null,
    statusCode: Int? = null,
    receivedBytes: Long = 0L,
): VesperStaleDownloadResourceException =
    VesperStaleDownloadResourceException(message, resourceId, segmentId, uri, phase, statusCode, receivedBytes)

private class DownloadProgressThrottle(
    minProgressBytes: Long,
    minProgressIntervalMs: Long,
) {
    private val minBytes = minProgressBytes.coerceAtLeast(1L)
    private val minIntervalNs = minProgressIntervalMs.coerceAtLeast(0L) * 1_000_000L
    private var lastReportedBytes = 0L
    private var lastReportedNs = 0L

    fun shouldReport(receivedBytes: Long, force: Boolean = false): Boolean {
        if (force || receivedBytes < lastReportedBytes) {
            markReported(receivedBytes)
            return true
        }
        if (receivedBytes - lastReportedBytes < minBytes) {
            return false
        }
        val now = System.nanoTime()
        if (lastReportedNs != 0L && now - lastReportedNs < minIntervalNs) {
            return false
        }
        markReported(receivedBytes, now)
        return true
    }

    fun markReported(receivedBytes: Long) {
        markReported(receivedBytes, System.nanoTime())
    }

    private fun markReported(
        receivedBytes: Long,
        now: Long,
    ) {
        lastReportedBytes = receivedBytes
        lastReportedNs = now
    }
}

private const val ANDROID_DOWNLOAD_BACKEND_FAILURE_ORDINAL = 3
private const val ANDROID_DOWNLOAD_NETWORK_CATEGORY_ORDINAL = 2
private const val ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_BYTES = 512L * 1024L
private const val ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_INTERVAL_MS = 250L
private const val ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS = 15_000
private const val ANDROID_HTTP_RANGE_NOT_SATISFIABLE = 416
