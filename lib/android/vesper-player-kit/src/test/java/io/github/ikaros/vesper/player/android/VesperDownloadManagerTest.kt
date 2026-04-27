package io.github.ikaros.vesper.player.android

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperDownloadManagerTest {
    @Test
    fun createTaskAutoStartRefreshesSnapshotAndStartsExecutor() {
        val bindings = FakeDownloadBindings(autoStart = true)
        val executor = RecordingDownloadExecutor()
        val manager =
            VesperDownloadManager(
                configuration = VesperDownloadConfiguration(autoStart = true),
                executor = executor,
                bindings = bindings,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        val taskId =
            manager.createTask(
                assetId = "asset-a",
                source =
                    VesperDownloadSource(
                        source =
                            VesperPlayerSource.remote(
                                uri = "https://example.com/video.mp4",
                                label = "Video",
                            ),
                    ),
                assetIndex = VesperDownloadAssetIndex(totalSizeBytes = 1024L),
            )

        assertEquals(1L, taskId)
        assertEquals(listOf(1L), executor.startedTaskIds)
        val task = manager.task(1L)
        assertNotNull(task)
        assertEquals(VesperDownloadState.Downloading, task?.state)
        assertTrue(manager.drainEvents().any { it is VesperDownloadEvent.Created })
        manager.dispose()
    }

    @Test
    fun pauseResumeAndRemoveDelegateToExecutorWithoutForkingStateMachine() {
        val bindings = FakeDownloadBindings(autoStart = true)
        val executor = RecordingDownloadExecutor()
        val manager =
            VesperDownloadManager(
                configuration = VesperDownloadConfiguration(autoStart = true),
                executor = executor,
                bindings = bindings,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        manager.createTask(
            assetId = "asset-a",
            source =
                VesperDownloadSource(
                    source =
                        VesperPlayerSource.remote(
                            uri = "https://example.com/video.mp4",
                            label = "Video",
                        ),
                ),
        )

        assertTrue(manager.pauseTask(1L))
        assertEquals(listOf(1L), executor.pausedTaskIds)
        assertEquals(VesperDownloadState.Paused, manager.task(1L)?.state)

        assertTrue(manager.resumeTask(1L))
        assertEquals(listOf(1L), executor.resumedTaskIds)
        assertEquals(VesperDownloadState.Downloading, manager.task(1L)?.state)

        assertTrue(manager.removeTask(1L))
        assertEquals(listOf(1L), executor.removedTaskIds)
        assertEquals(VesperDownloadState.Removed, manager.task(1L)?.state)
        manager.dispose()
    }

    @Test
    fun executorReporterUpdatesSharedSnapshotProgressAndCompletion() {
        val bindings = FakeDownloadBindings(autoStart = true)
        val executor = RecordingDownloadExecutor(autoComplete = true)
        val manager =
            VesperDownloadManager(
                configuration = VesperDownloadConfiguration(autoStart = true),
                executor = executor,
                bindings = bindings,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        manager.createTask(
            assetId = "asset-a",
            source =
                VesperDownloadSource(
                    source =
                        VesperPlayerSource.remote(
                            uri = "https://example.com/video.mp4",
                            label = "Video",
                        ),
                ),
            assetIndex = VesperDownloadAssetIndex(totalSizeBytes = 512L),
        )

        val task = manager.task(1L)
        assertNotNull(task)
        assertEquals(VesperDownloadState.Completed, task?.state)
        assertEquals(512L, task?.progress?.receivedBytes)
        assertEquals("/tmp/downloads/1.bin", task?.assetIndex?.completedPath)
        manager.dispose()
    }

    @Test
    fun pluginLibraryPathsAreForwardedToNativeSessionConfig() {
        val bindings = FakeDownloadBindings(autoStart = false)
        val manager =
            VesperDownloadManager(
                configuration =
                    VesperDownloadConfiguration(
                        autoStart = false,
                        runPostProcessorsOnCompletion = false,
                        pluginLibraryPaths =
                            listOf(
                                "/data/local/tmp/libplayer_remux_ffmpeg.so",
                                "/data/local/tmp/libvesper_metrics.so",
                            ),
                    ),
                executor = RecordingDownloadExecutor(),
                bindings = bindings,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        assertEquals(
            listOf(
                "/data/local/tmp/libplayer_remux_ffmpeg.so",
                "/data/local/tmp/libvesper_metrics.so",
            ),
            bindings.createdConfig?.pluginLibraryPaths?.toList(),
        )
        assertEquals(false, bindings.createdConfig?.runPostProcessorsOnCompletion)
        manager.dispose()
    }

    @Test
    fun exportTaskOutputForwardsProgressAndCancellationToBindings() = runBlocking {
        val bindings = FakeDownloadBindings(autoStart = false)
        val manager =
            VesperDownloadManager(
                configuration = VesperDownloadConfiguration(autoStart = false),
                executor = RecordingDownloadExecutor(),
                bindings = bindings,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        val taskId =
            manager.createTask(
                assetId = "asset-a",
                source =
                    VesperDownloadSource(
                        source =
                            VesperPlayerSource.remote(
                                uri = "https://example.com/video.m3u8",
                                label = "Video",
                                protocol = VesperPlayerSourceProtocol.Hls,
                            ),
                    ),
            )

        manager.exportTaskOutput(
            taskId = taskId ?: error("task must be created"),
            outputPath = "/tmp/exported.mp4",
            onProgress = bindings.forwardedProgress::add,
            isCancelled = { true },
        )

        assertEquals(listOf(0.25f, 1.0f), bindings.forwardedProgress)
        assertEquals(true, bindings.exportWasCancelled)
        manager.dispose()
    }
}

private class FakeDownloadBindings(
    private val autoStart: Boolean,
) : VesperDownloadManager.DownloadBindings {
    private val tasks = linkedMapOf<Long, NativeDownloadTask>()
    private val commands = mutableListOf<NativeDownloadCommand>()
    private val events = mutableListOf<NativeDownloadEvent>()
    private var nextTaskId = 1L
    var createdConfig: NativeDownloadConfig? = null
    val forwardedProgress = mutableListOf<Float>()
    var exportWasCancelled: Boolean = false

    override fun createDownloadSession(config: NativeDownloadConfig): Long {
        createdConfig = config
        return 17L
    }

    override fun disposeDownloadSession(sessionHandle: Long) = Unit

    override fun createDownloadTask(
        sessionHandle: Long,
        assetId: String,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Long {
        val taskId = nextTaskId++
        val task =
            NativeDownloadTask(
                taskId = taskId,
                assetId = assetId,
                source = source,
                profile = profile,
                statusOrdinal = if (autoStart) 2 else 0,
                progress =
                    NativeDownloadProgress(
                        receivedBytes = 0L,
                        hasTotalBytes = assetIndex.hasTotalSizeBytes,
                        totalBytes = assetIndex.totalSizeBytes,
                        receivedSegments = 0,
                        hasTotalSegments = assetIndex.segments.isNotEmpty(),
                        totalSegments = assetIndex.segments.size,
                    ),
                assetIndex = assetIndex,
                hasError = false,
                errorCodeOrdinal = 0,
                errorCategoryOrdinal = 0,
                errorRetriable = false,
                errorMessage = null,
            )
        tasks[taskId] = task
        events += NativeDownloadEvent.Created(task)
        events += NativeDownloadEvent.StateChanged(task)
        if (autoStart) {
            commands += NativeDownloadCommand.Start(task)
        }
        return taskId
    }

    override fun startDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 2).also { updated ->
                commands += NativeDownloadCommand.Start(updated)
                events += NativeDownloadEvent.StateChanged(updated)
            }
        }

    override fun pauseDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 3).also { updated ->
                commands += NativeDownloadCommand.Pause(taskId)
                events += NativeDownloadEvent.StateChanged(updated)
            }
        }

    override fun resumeDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 2).also { updated ->
                commands += NativeDownloadCommand.Resume(updated)
                events += NativeDownloadEvent.StateChanged(updated)
            }
        }

    override fun updateDownloadTaskProgress(
        sessionHandle: Long,
        taskId: Long,
        receivedBytes: Long,
        receivedSegments: Int,
        nowEpochMs: Long,
    ): Boolean =
        updateTask(taskId) { task ->
            task.withProgress(
                progress =
                    task.progress.withValues(
                        receivedBytes = receivedBytes,
                        receivedSegments = receivedSegments,
                    ),
            ).also { updated ->
                events += NativeDownloadEvent.ProgressUpdated(updated)
            }
        }

    override fun completeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        completedPath: String,
        nowEpochMs: Long,
    ): Boolean =
        updateTask(taskId) { task ->
            val totalBytes = if (task.progress.hasTotalBytes) task.progress.totalBytes else task.progress.receivedBytes
            val totalSegments =
                if (task.progress.hasTotalSegments) task.progress.totalSegments else task.progress.receivedSegments
            task.withStatus(
                statusOrdinal = 4,
                progress =
                    task.progress.withValues(
                        receivedBytes = totalBytes,
                        receivedSegments = totalSegments,
                    ),
                assetIndex = task.assetIndex.withCompletedPath(completedPath),
            ).also { updated ->
                events += NativeDownloadEvent.StateChanged(updated)
            }
        }

    override fun exportDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        outputPath: String,
        progressCallback: NativeDownloadExportProgressCallback?,
    ): Boolean {
        progressCallback?.onProgress(0.25f)
        progressCallback?.onProgress(1.0f)
        exportWasCancelled = progressCallback?.isCancelled() ?: false
        return true
    }

    override fun failDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
        nowEpochMs: Long,
    ): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(
                statusOrdinal = 5,
                hasError = true,
                errorCodeOrdinal = codeOrdinal,
                errorCategoryOrdinal = categoryOrdinal,
                errorRetriable = retriable,
                errorMessage = message,
            ).also { updated ->
                events += NativeDownloadEvent.StateChanged(updated)
            }
        }

    override fun removeDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 6).also { updated ->
                commands += NativeDownloadCommand.Remove(taskId)
                events += NativeDownloadEvent.StateChanged(updated)
            }
        }

    override fun pollDownloadSnapshot(sessionHandle: Long): NativeDownloadSnapshot =
        NativeDownloadSnapshot(tasks = tasks.values.toTypedArray())

    override fun drainDownloadCommands(sessionHandle: Long): Array<NativeDownloadCommand> =
        commands.toTypedArray().also { commands.clear() }

    override fun drainDownloadEvents(sessionHandle: Long): Array<NativeDownloadEvent> =
        events.toTypedArray().also { events.clear() }

    private fun updateTask(
        taskId: Long,
        transform: (NativeDownloadTask) -> NativeDownloadTask,
    ): Boolean {
        val task = tasks[taskId] ?: return false
        tasks[taskId] = transform(task)
        return true
    }
}

private fun NativeDownloadTask.withStatus(
    statusOrdinal: Int,
    progress: NativeDownloadProgress = this.progress,
    assetIndex: NativeDownloadAssetIndex = this.assetIndex,
    hasError: Boolean = this.hasError,
    errorCodeOrdinal: Int = this.errorCodeOrdinal,
    errorCategoryOrdinal: Int = this.errorCategoryOrdinal,
    errorRetriable: Boolean = this.errorRetriable,
    errorMessage: String? = this.errorMessage,
): NativeDownloadTask =
    NativeDownloadTask(
        taskId = taskId,
        assetId = assetId,
        source = source,
        profile = profile,
        statusOrdinal = statusOrdinal,
        progress = progress,
        assetIndex = assetIndex,
        hasError = hasError,
        errorCodeOrdinal = errorCodeOrdinal,
        errorCategoryOrdinal = errorCategoryOrdinal,
        errorRetriable = errorRetriable,
        errorMessage = errorMessage,
    )

private fun NativeDownloadTask.withProgress(
    progress: NativeDownloadProgress,
): NativeDownloadTask =
    withStatus(statusOrdinal = statusOrdinal, progress = progress)

private fun NativeDownloadProgress.withValues(
    receivedBytes: Long = this.receivedBytes,
    receivedSegments: Int = this.receivedSegments,
): NativeDownloadProgress =
    NativeDownloadProgress(
        receivedBytes = receivedBytes,
        hasTotalBytes = hasTotalBytes,
        totalBytes = totalBytes,
        receivedSegments = receivedSegments,
        hasTotalSegments = hasTotalSegments,
        totalSegments = totalSegments,
    )

private fun NativeDownloadAssetIndex.withCompletedPath(
    completedPath: String?,
): NativeDownloadAssetIndex =
    NativeDownloadAssetIndex(
        contentFormatOrdinal = contentFormatOrdinal,
        version = version,
        etag = etag,
        checksum = checksum,
        hasTotalSizeBytes = hasTotalSizeBytes,
        totalSizeBytes = totalSizeBytes,
        resources = resources,
        segments = segments,
        completedPath = completedPath,
    )

private class RecordingDownloadExecutor(
    private val autoComplete: Boolean = false,
) : VesperDownloadExecutor {
    val startedTaskIds = mutableListOf<Long>()
    val resumedTaskIds = mutableListOf<Long>()
    val pausedTaskIds = mutableListOf<Long>()
    val removedTaskIds = mutableListOf<Long>()

    override fun start(task: VesperDownloadTaskSnapshot, reporter: VesperDownloadExecutionReporter) {
        startedTaskIds += task.taskId
        if (autoComplete) {
            reporter.updateProgress(task.taskId, 512L, 0)
            reporter.complete(task.taskId, "/tmp/downloads/${task.taskId}.bin")
        }
    }

    override fun resume(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        resumedTaskIds += task.taskId
    }

    override fun pause(taskId: VesperDownloadTaskId) {
        pausedTaskIds += taskId
    }

    override fun remove(task: VesperDownloadTaskSnapshot?) {
        task?.let { removedTaskIds += it.taskId }
    }
}
