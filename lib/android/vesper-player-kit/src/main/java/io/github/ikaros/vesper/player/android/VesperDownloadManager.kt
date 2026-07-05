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
import kotlinx.coroutines.withTimeout

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

    companion object {
        private const val MAX_EVENT_BUFFER_SIZE = 500
        private const val RUNTIME_OP_TIMEOUT_MS = 30_000L
    }
    private val taskStore = DownloadTaskStore()
    private val lastProgressPersistence = mutableMapOf<VesperDownloadTaskId, ProgressPersistenceCheckpoint>()
    private val _snapshot = MutableStateFlow(VesperDownloadSnapshot(emptyList()))
    @Volatile
    private var sessionHandle: Long = bindings.createDownloadSession(configuration.toNativePayload())
    private val isDisposed = java.util.concurrent.atomic.AtomicBoolean(false)

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
        if (!isDisposed.compareAndSet(false, true)) {
            return
        }
        val handle = sessionHandle
        sessionHandle = 0L
        snapshot.value.tasks
            .filter {
                it.state == VesperDownloadState.Preparing ||
                    it.state == VesperDownloadState.Downloading
            }
            .forEach { pauseTask(it.taskId) }
        persistSnapshot(snapshot.value)
        executor.dispose()
        runCatching {
            if (handle != 0L) {
                // Bypass onRuntimeThread because its isDisposed guard would
                // skip the dispose call; we must dispose regardless.
                runBlocking {
                    withTimeout(RUNTIME_OP_TIMEOUT_MS) {
                        withContext(runtimeDispatcher) {
                            bindings.disposeDownloadSession(handle)
                        }
                    }
                }
            }
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
        source.source.drmConfiguration?.let {
            throw VesperPlayerUnsupportedOperation(
                drmUnsupportedRouteMessage("download"),
                drmUnsupportedRouteDetails(source.source, route = "download"),
            )
        }
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
        val handle = sessionHandle
        if (handle == 0L) {
            return null
        }
        val taskId =
            onRuntimeThread {
                bindings.createDownloadTask(
                    sessionHandle = handle,
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
        tasks.firstOrNull { it.source.source.drmConfiguration != null }?.let { task ->
            throw VesperPlayerUnsupportedOperation(
                drmUnsupportedRouteMessage("download"),
                drmUnsupportedRouteDetails(task.source.source, route = "download"),
            )
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
        val handle = sessionHandle
        if (handle == 0L) {
            return false
        }
        val started =
            onRuntimeThread {
                bindings.startDownloadTask(handle, taskId, System.currentTimeMillis())
            }
        if (started) {
            syncRuntimeState(processCommands = true)
        }
        return started
    }

    fun pauseTask(taskId: VesperDownloadTaskId): Boolean {
        val handle = sessionHandle
        if (handle == 0L) {
            return false
        }
        val paused =
            onRuntimeThread {
                bindings.pauseDownloadTask(handle, taskId, System.currentTimeMillis())
            }
        if (paused) {
            syncRuntimeState(processCommands = true)
        }
        return paused
    }

    fun resumeTask(taskId: VesperDownloadTaskId): Boolean {
        val handle = sessionHandle
        if (handle == 0L) {
            return false
        }
        val resumed =
            onRuntimeThread {
                bindings.resumeDownloadTask(handle, taskId, System.currentTimeMillis())
            }
        if (resumed) {
            syncRuntimeState(processCommands = true)
        }
        return resumed
    }

    fun removeTask(taskId: VesperDownloadTaskId): Boolean {
        val handle = sessionHandle
        if (handle == 0L) {
            return false
        }
        val removed =
            onRuntimeThread {
                bindings.removeDownloadTask(handle, taskId, System.currentTimeMillis())
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
        val handle = sessionHandle
        check(handle != 0L) { "native download session handle must not be zero" }
        withContext(runtimeDispatcher) {
            val exported =
                bindings.exportDownloadTask(
                    sessionHandle = handle,
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
        val handle = sessionHandle
        if (handle == 0L) {
            taskStore.replaceAll(VesperDownloadSnapshot(emptyList()))
            _snapshot.value = VesperDownloadSnapshot(emptyList())
            lastProgressPersistence.clear()
            synchronized(eventBufferLock) {
                eventBuffer.clear()
            }
            return
        }

        val events = onRuntimeThread { bindings.drainDownloadEvents(handle).toList() }
            .map(NativeDownloadEvent::toPublic)
        if (events.isNotEmpty()) {
            synchronized(eventBufferLock) {
                eventBuffer += events
                // Drop oldest events when buffer exceeds capacity to prevent unbounded growth.
                if (eventBuffer.size > MAX_EVENT_BUFFER_SIZE) {
                    val excess = eventBuffer.size - MAX_EVENT_BUFFER_SIZE
                    eventBuffer.subList(0, excess).clear()
                }
            }
            val immediateEvents = events.filterNot { it.isRemovedStatePatch }
            if (immediateEvents.isNotEmpty()) {
                val updatedSnapshot = taskStore.apply(immediateEvents)
                _snapshot.value = updatedSnapshot
            }
        }

        if (processCommands) {
            val commands = onRuntimeThread { bindings.drainDownloadCommands(handle).toList() }
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
        val handle = sessionHandle
        if (handle == 0L) {
            taskStore.replaceAll(VesperDownloadSnapshot(emptyList()))
            _snapshot.value = VesperDownloadSnapshot(emptyList())
            lastProgressPersistence.clear()
            synchronized(eventBufferLock) {
                eventBuffer.clear()
            }
            return
        }

        val fullSnapshot =
            onRuntimeThread { bindings.pollDownloadSnapshot(handle) }?.toPublic()
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

    /**
     * Executes [block] on the single-threaded runtime dispatcher and blocks the
     * calling thread until completion or [RUNTIME_OP_TIMEOUT_MS].
     *
     * Callers on the main thread should prefer [exportTaskOutput] (already
     * suspend) or dispatch to a background coroutine; synchronous methods exist
     * for convenience but may block the caller for the duration of the JNI call.
     */
    private fun <T> onRuntimeThread(block: () -> T): T =
        runBlocking {
            withTimeout(RUNTIME_OP_TIMEOUT_MS) {
                withContext(runtimeDispatcher) { block() }
            }
        }

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
                val handle = sessionHandle
                if (handle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.completeDownloadPreparation(
                        sessionHandle = handle,
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
                val handle = sessionHandle
                if (handle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.replaceDownloadTaskPlan(
                        sessionHandle = handle,
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
                val handle = sessionHandle
                if (handle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.updateDownloadTaskProgress(
                        sessionHandle = handle,
                        taskId = taskId,
                        receivedBytes = receivedBytes,
                        receivedSegments = receivedSegments,
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }

            override fun complete(taskId: VesperDownloadTaskId, completedPath: String?) {
                val handle = sessionHandle
                if (handle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.completeDownloadTask(
                        sessionHandle = handle,
                        taskId = taskId,
                        completedPath = completedPath.orEmpty(),
                        nowEpochMs = System.currentTimeMillis(),
                    )
                }
                syncRuntimeState(processCommands = false)
            }

            override fun fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
                val handle = sessionHandle
                if (handle == 0L) {
                    return
                }
                onRuntimeThread {
                    bindings.failDownloadTask(
                        sessionHandle = handle,
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
        try {
            check(sessionHandle != 0L) { "native download session handle must not be zero" }
            restorePersistedTasks()
            forceFullSync()
        } catch (error: Throwable) {
            // Clean up the native session if constructor initialization fails,
            // preventing a permanent resource leak (AGENTS.md rule).
            val handle = sessionHandle
            if (handle != 0L) {
                runCatching { bindings.disposeDownloadSession(handle) }
                sessionHandle = 0L
            }
            throw error
        }
    }

}
