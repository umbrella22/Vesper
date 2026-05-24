package io.github.ikaros.vesper.player.android

import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.nio.file.Files
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperDownloadManagerTest {
    @Test
    fun sharedDownloadTaskContractKeepsStableFields() {
        val payload = contractText("download_task_snapshot.json")

        assertTrue(payload.contains("\"taskId\": 42"))
        assertTrue(payload.contains("\"assetId\": \"asset-contract\""))
        assertEquals(
            VesperPlayerSourceProtocol.Dash,
            VesperPlayerSourceProtocol.valueOf(
                contractString(payload, "protocol").replaceFirstChar { it.uppercase() },
            ),
        )
        assertTrue(payload.contains("\"contentFormat\": \"dashSegments\""))
        assertTrue(payload.contains("\"targetOutputFormat\": \"mp4\""))
        assertTrue(payload.contains("\"state\": \"downloading\""))
        assertTrue(payload.contains("\"receivedBytes\": 2048"))
        assertTrue(payload.contains("\"resourceId\": \"manifest\""))
        assertTrue(payload.contains("\"offset\": 128"))
        assertTrue(payload.contains("\"error\": null"))
    }

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
    fun sourceHeadersSurviveNativeDownloadCommandRoundTrip() {
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
                            uri = "https://example.com/video.m3u8",
                            label = "Video",
                            protocol = VesperPlayerSourceProtocol.Hls,
                            headers =
                                mapOf(
                                    "User-Agent" to "VesperTest/1.0",
                                    "Referer" to "https://example.com/player",
                                    "" to "ignored",
                                    "Origin" to "",
                                ),
                        ),
                ),
        )

        val expected =
            mapOf(
                "User-Agent" to "VesperTest/1.0",
                "Referer" to "https://example.com/player",
            )
        assertEquals(expected, executor.preparedSourceHeaders.single())
        assertEquals(expected, executor.startedSourceHeaders.single())
        assertEquals(expected, manager.task(1L)?.source?.source?.headers)
        manager.dispose()
    }

    @Test
    fun foregroundExecutorForwardsSourceHeadersToManifestProbesAndTransfers() {
        val requests = mutableListOf<RecordedHttpRequest>()
        val server = headerRecordingServer(requests)
        server.start()
        val outputDir = Files.createTempDirectory("vesper-android-download-test").toFile()
        try {
            val baseUrl = "http://127.0.0.1:${server.address.port}"
            val headers =
                mapOf(
                    "User-Agent" to "VesperFixture/1.0",
                    "Referer" to "https://example.com/player",
                )
            val task =
                VesperDownloadTaskSnapshot(
                    taskId = 7L,
                    assetId = "asset-http",
                    source =
                        VesperDownloadSource(
                            source =
                                VesperPlayerSource.hls(
                                    uri = "$baseUrl/index.m3u8",
                                    label = "Fixture",
                                    headers = headers,
                                ),
                        ),
                    profile = VesperDownloadProfile(targetDirectory = outputDir.absolutePath),
                    state = VesperDownloadState.Preparing,
                    progress = VesperDownloadProgressSnapshot(),
                    assetIndex = VesperDownloadAssetIndex(),
                )
            val executor =
                VesperForegroundDownloadExecutor(
                    context = null,
                    baseDirectory = outputDir,
                )
            val reporter = BlockingDownloadReporter()

            executor.prepare(task, reporter)
            val assetIndex = reporter.awaitPreparedAssetIndex()
            assertNotNull(assetIndex)

            executor.start(task.copy(assetIndex = checkNotNull(assetIndex)), reporter)
            assertTrue("download did not complete", reporter.awaitCompleted())

            assertRecordedHeader(requests, "GET", "/index.m3u8", "User-Agent", "VesperFixture/1.0")
            assertRecordedHeader(requests, "HEAD", "/init.mp4", "Referer", "https://example.com/player")
            assertRecordedHeader(requests, "HEAD", "/segment.ts", "Referer", "https://example.com/player")
            assertRecordedHeader(requests, "GET", "/init.mp4", "User-Agent", "VesperFixture/1.0")
            assertRecordedHeader(requests, "GET", "/segment.ts", "User-Agent", "VesperFixture/1.0")
            executor.dispose()
        } finally {
            server.stop(0)
            outputDir.deleteRecursively()
        }
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
        assertNull(manager.task(1L))
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
                                "/data/local/tmp/libvesper_remux_ffmpeg.so",
                                "/data/local/tmp/libvesper_metrics.so",
                            ),
                    ),
                executor = RecordingDownloadExecutor(),
                bindings = bindings,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        assertEquals(
            listOf(
                "/data/local/tmp/libvesper_remux_ffmpeg.so",
                "/data/local/tmp/libvesper_metrics.so",
            ),
            bindings.createdConfig?.pluginLibraryPaths?.toList(),
        )
        assertEquals(false, bindings.createdConfig?.runPostProcessorsOnCompletion)
        manager.dispose()
    }

    @Test
    fun defaultDownloadBaseDirectoryStaysUnderAppPrivateFilesDir() {
        val filesDir = Files.createTempDirectory("vesper-android-files").toFile()
        val configured = Files.createTempDirectory("vesper-android-custom").toFile()
        try {
            assertEquals(
                File(filesDir, "vesper-downloads"),
                vesperDefaultDownloadBaseDirectory(filesDir, null),
            )
            assertEquals(
                configured,
                vesperDefaultDownloadBaseDirectory(filesDir, configured),
            )
        } finally {
            filesDir.deleteRecursively()
            configured.deleteRecursively()
        }
    }

    @Test
    fun manifestDoesNotRequestPublicStorageOrDownloadForegroundServicePermissions() {
        val manifest = File("src/main/AndroidManifest.xml").readText()

        assertFalse(manifest.contains("MANAGE_EXTERNAL_STORAGE"))
        assertFalse(manifest.contains("WRITE_EXTERNAL_STORAGE"))
        assertFalse(manifest.contains("FOREGROUND_SERVICE_DATA_SYNC"))
        assertFalse(manifest.contains("foregroundServiceType=\"dataSync\""))
    }

    @Test
    fun consumerRulesKeepAndroidBridgeClassesForR8() {
        val rules = File("consumer-rules.pro").readText()

        assertTrue(rules.contains("-keep class io.github.ikaros.vesper.player.android.**"))
        assertTrue(rules.contains("*;"))
    }

    @Test
    fun persistedActiveTasksRestoreAndAutoResumeOnStartup() {
        val directory = Files.createTempDirectory("vesper-android-restore").toFile()
        val task =
            VesperDownloadTaskSnapshot(
                taskId = 42L,
                assetId = "asset-restore",
                source =
                    VesperDownloadSource(
                        source =
                            VesperPlayerSource.remote(
                                uri = "https://example.com/video.mp4",
                                label = "Video",
                            ),
                        contentFormat = VesperDownloadContentFormat.SingleFile,
                    ),
                profile = VesperDownloadProfile(),
                state = VesperDownloadState.Downloading,
                progress = VesperDownloadProgressSnapshot(receivedBytes = 512L, totalBytes = 1024L),
                assetIndex =
                    VesperDownloadAssetIndex(
                        contentFormat = VesperDownloadContentFormat.SingleFile,
                        totalSizeBytes = 1024L,
                        resources =
                            listOf(
                                VesperDownloadResourceRecord(
                                    resourceId = "video",
                                    uri = "https://example.com/video.mp4",
                                    relativePath = "video.mp4",
                                    sizeBytes = 1024L,
                                ),
                            ),
                    ),
            )
        val stateStore = InMemoryDownloadStateStore(VesperDownloadSnapshot(listOf(task)))

        val bindings = FakeDownloadBindings(autoStart = false)
        val executor = RecordingDownloadExecutor()
        val manager =
            VesperDownloadManager(
                configuration = VesperDownloadConfiguration(autoStart = true),
                executor = executor,
                bindings = bindings,
                stateStore = stateStore,
                defaultBaseDirectory = directory,
                runtimeDispatcher = Dispatchers.Unconfined,
            )

        try {
            assertEquals(VesperDownloadState.Downloading, manager.task(42L)?.state)
            assertEquals(listOf(42L), executor.resumedTaskIds)
        } finally {
            manager.dispose()
            directory.deleteRecursively()
        }
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

private class InMemoryDownloadStateStore(
    initialSnapshot: VesperDownloadSnapshot = VesperDownloadSnapshot(emptyList()),
) : VesperDownloadStatePersistence {
    private var snapshot = initialSnapshot

    override fun load(): VesperDownloadSnapshot = snapshot

    override fun save(snapshot: VesperDownloadSnapshot) {
        this.snapshot = snapshot
    }
}

private class FakeDownloadBindings(
    private val autoStart: Boolean,
) : DownloadBindings {
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
                statusOrdinal = if (autoStart) 1 else 0,
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
        events += task.toStateChangedEvent()
        if (autoStart) {
            commands += NativeDownloadCommand.Prepare(task)
        }
        return taskId
    }

    override fun restoreDownloadTasks(
        sessionHandle: Long,
        tasks: Array<NativeDownloadTask>,
        nowEpochMs: Long,
    ): Boolean {
        tasks.forEach { task ->
            this.tasks[task.taskId] = task
            nextTaskId = maxOf(nextTaskId, task.taskId + 1)
        }
        return true
    }

    override fun startDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 1).also { updated ->
                commands += NativeDownloadCommand.Prepare(updated)
                events += updated.toStateChangedEvent()
            }
        }

    override fun completeDownloadPreparation(
        sessionHandle: Long,
        taskId: Long,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(
                statusOrdinal = 2,
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
            ).also { updated ->
                events += NativeDownloadEvent.AssetIndexUpdated(updated)
                events += updated.toStateChangedEvent()
                commands += NativeDownloadCommand.Start(updated)
            }
        }

    override fun replaceDownloadTaskPlan(
        sessionHandle: Long,
        taskId: Long,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean =
        updateTask(taskId) { task ->
            NativeDownloadTask(
                taskId = task.taskId,
                assetId = task.assetId,
                source = source,
                profile = profile,
                statusOrdinal = 1,
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
            ).also { updated ->
                events += NativeDownloadEvent.AssetIndexUpdated(updated)
                events += updated.toStateChangedEvent()
            }
        }

    override fun pauseDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 3).also { updated ->
                commands += NativeDownloadCommand.Pause(taskId)
                events += updated.toStateChangedEvent()
            }
        }

    override fun resumeDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 2).also { updated ->
                commands += NativeDownloadCommand.Resume(updated)
                events += updated.toStateChangedEvent()
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
                events += updated.toProgressUpdatedEvent()
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
                events += updated.toStateChangedEvent()
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
                events += updated.toStateChangedEvent()
            }
        }

    override fun removeDownloadTask(sessionHandle: Long, taskId: Long, nowEpochMs: Long): Boolean =
        updateTask(taskId) { task ->
            task.withStatus(statusOrdinal = 6).also { updated ->
                commands += NativeDownloadCommand.Remove(taskId)
                events += updated.toStateChangedEvent()
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

private fun NativeDownloadTask.toStateChangedEvent(): NativeDownloadEvent.StateChanged =
    NativeDownloadEvent.StateChanged(
        taskId = taskId,
        statusOrdinal = statusOrdinal,
        progress = progress,
        hasError = hasError,
        errorCodeOrdinal = errorCodeOrdinal,
        errorCategoryOrdinal = errorCategoryOrdinal,
        errorRetriable = errorRetriable,
        errorMessage = errorMessage,
        completedPath = assetIndex.completedPath,
    )

private fun NativeDownloadTask.toProgressUpdatedEvent(): NativeDownloadEvent.ProgressUpdated =
    NativeDownloadEvent.ProgressUpdated(
        taskId = taskId,
        progress = progress,
    )

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
        streams = streams,
        completedPath = completedPath,
    )

private data class RecordedHttpRequest(
    val method: String,
    val path: String,
    val headers: Map<String, String>,
)

private class BlockingDownloadReporter : VesperDownloadExecutionReporter {
    private val preparedLatch = CountDownLatch(1)
    private val completedLatch = CountDownLatch(1)
    @Volatile private var preparedAssetIndex: VesperDownloadAssetIndex? = null
    @Volatile private var failure: VesperDownloadError? = null

    override fun completePreparation(
        taskId: VesperDownloadTaskId,
        assetIndex: VesperDownloadAssetIndex,
    ) {
        preparedAssetIndex = assetIndex
        preparedLatch.countDown()
    }

    override fun updateProgress(
        taskId: VesperDownloadTaskId,
        receivedBytes: Long,
        receivedSegments: Int,
    ) = Unit

    override fun complete(taskId: VesperDownloadTaskId, completedPath: String?) {
        completedLatch.countDown()
    }

    override fun fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
        failure = error
        preparedLatch.countDown()
        completedLatch.countDown()
    }

    fun awaitPreparedAssetIndex(): VesperDownloadAssetIndex? {
        assertTrue(
            "download preparation did not finish: ${failure?.message}",
            preparedLatch.await(5, TimeUnit.SECONDS),
        )
        assertEquals(null, failure)
        return preparedAssetIndex
    }

    fun awaitCompleted(): Boolean {
        val completed = completedLatch.await(5, TimeUnit.SECONDS)
        assertEquals(null, failure)
        return completed
    }
}

private fun headerRecordingServer(requests: MutableList<RecordedHttpRequest>): HeaderRecordingServer =
    HeaderRecordingServer(requests)

private class HeaderRecordingServer(
    private val requests: MutableList<RecordedHttpRequest>,
) {
    private val serverSocket = ServerSocket()
    private var acceptThread: Thread? = null

    @Volatile
    private var running = false

    val address: InetSocketAddress
        get() = serverSocket.localSocketAddress as InetSocketAddress

    fun start() {
        serverSocket.bind(InetSocketAddress("127.0.0.1", 0))
        running = true
        acceptThread =
            thread(
                name = "vesper-download-test-http",
                isDaemon = true,
            ) {
                while (running) {
                    try {
                        val socket = serverSocket.accept()
                        thread(
                            name = "vesper-download-test-http-client",
                            isDaemon = true,
                        ) {
                            socket.use(::handleSocket)
                        }
                    } catch (error: SocketException) {
                        if (running) {
                            throw error
                        }
                        return@thread
                    }
                }
            }
    }

    fun stop(delaySeconds: Int) {
        running = false
        runCatching { serverSocket.close() }
        val joinMs = delaySeconds.coerceAtLeast(0) * 1000L
        if (joinMs > 0) {
            acceptThread?.join(joinMs)
        }
    }

    private fun handleSocket(socket: Socket) {
        val reader =
            BufferedReader(
                InputStreamReader(socket.getInputStream(), Charsets.ISO_8859_1),
            )
        val requestLine = reader.readLine() ?: return
        val requestParts = requestLine.split(" ", limit = 3)
        if (requestParts.size < 2) {
            return
        }

        val method = requestParts[0]
        val path = requestParts[1].substringBefore('?')
        val headers = linkedMapOf<String, MutableList<String>>()
        while (true) {
            val line = reader.readLine() ?: break
            if (line.isEmpty()) {
                break
            }
            val separator = line.indexOf(':')
            if (separator <= 0) {
                continue
            }
            val name = line.substring(0, separator).trim().lowercase()
            val value = line.substring(separator + 1).trim()
            headers.getOrPut(name) { mutableListOf() } += value
        }

        val requestHeaders =
            headers.mapValues { (_, values) -> values.joinToString(",") }
        synchronized(requests) {
            requests +=
                RecordedHttpRequest(
                    method = method,
                    path = path,
                    headers = requestHeaders,
                )
        }
        socket.getOutputStream().respondForFixturePath(method, path, requestHeaders)
    }
}

private fun OutputStream.respondForFixturePath(
    requestMethod: String,
    requestPath: String,
    requestHeaders: Map<String, String>,
) {
    val body =
        when (requestPath) {
            "/index.m3u8" ->
                """
                #EXTM3U
                #EXT-X-VERSION:7
                #EXT-X-TARGETDURATION:1
                #EXT-X-MAP:URI="init.mp4"
                #EXTINF:1.0,
                segment.ts
                #EXT-X-ENDLIST
                """.trimIndent().toByteArray()
            "/init.mp4" -> "init".toByteArray()
            "/segment.ts" -> "segment-data".toByteArray()
            else -> ByteArray(0)
        }
    if (body.isEmpty()) {
        writeHttpResponse(404, "Not Found", body = ByteArray(0), includeBody = false)
        return
    }
    if (requestMethod.equals("HEAD", ignoreCase = true)) {
        writeHttpResponse(
            200,
            "OK",
            headers = mapOf("Content-Length" to body.size.toString()),
            includeBody = false,
        )
    } else if (requestHeaders["range"]?.startsWith("bytes=") == true) {
        val range = checkNotNull(requestHeaders["range"]).removePrefix("bytes=")
        val start = range.substringBefore('-').toIntOrNull()
        val requestedEnd = range.substringAfter('-', missingDelimiterValue = "")
        val end = requestedEnd.toIntOrNull() ?: body.lastIndex
        if (start == null || start !in body.indices || end !in body.indices || end < start) {
            writeHttpResponse(
                416,
                "Range Not Satisfiable",
                body = ByteArray(0),
                includeBody = false,
            )
        } else {
            val slice = body.copyOfRange(start, end + 1)
            writeHttpResponse(
                206,
                "Partial Content",
                headers =
                    mapOf(
                        "Content-Length" to slice.size.toString(),
                        "Content-Range" to "bytes $start-$end/${body.size}",
                    ),
                body = slice,
            )
        }
    } else {
        writeHttpResponse(
            200,
            "OK",
            headers = mapOf("Content-Length" to body.size.toString()),
            body = body,
        )
    }
}

private fun OutputStream.writeHttpResponse(
    statusCode: Int,
    reason: String,
    headers: Map<String, String> = emptyMap(),
    body: ByteArray = ByteArray(0),
    includeBody: Boolean = true,
) {
    val responseHeaders =
        buildString {
            append("HTTP/1.1 ")
            append(statusCode)
            append(' ')
            append(reason)
            append("\r\n")
            append("Connection: close\r\n")
            headers.forEach { (name, value) ->
                append(name)
                append(": ")
                append(value)
                append("\r\n")
            }
            append("\r\n")
        }
    write(responseHeaders.toByteArray(Charsets.ISO_8859_1))
    if (includeBody && body.isNotEmpty()) {
        write(body)
    }
    flush()
}

private fun assertRecordedHeader(
    requests: List<RecordedHttpRequest>,
    method: String,
    path: String,
    headerName: String,
    expectedValue: String,
) {
    val request =
        synchronized(requests) {
            requests.firstOrNull { it.method.equals(method, ignoreCase = true) && it.path == path }
        }
    assertNotNull("missing $method $path request; saw $requests", request)
    assertEquals(expectedValue, request?.headers?.get(headerName.lowercase()))
}

private class RecordingDownloadExecutor(
    private val autoComplete: Boolean = false,
) : VesperDownloadExecutor {
    val preparedSourceHeaders = mutableListOf<Map<String, String>>()
    val startedSourceHeaders = mutableListOf<Map<String, String>>()
    val resumedSourceHeaders = mutableListOf<Map<String, String>>()
    val startedTaskIds = mutableListOf<Long>()
    val resumedTaskIds = mutableListOf<Long>()
    val pausedTaskIds = mutableListOf<Long>()
    val removedTaskIds = mutableListOf<Long>()

    override fun prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        preparedSourceHeaders += task.source.source.headers
        reporter.completePreparation(task.taskId, task.assetIndex)
    }

    override fun start(task: VesperDownloadTaskSnapshot, reporter: VesperDownloadExecutionReporter) {
        startedTaskIds += task.taskId
        startedSourceHeaders += task.source.source.headers
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
        resumedSourceHeaders += task.source.source.headers
    }

    override fun pause(taskId: VesperDownloadTaskId) {
        pausedTaskIds += taskId
    }

    override fun remove(task: VesperDownloadTaskSnapshot?) {
        task?.let { removedTaskIds += it.taskId }
    }
}
