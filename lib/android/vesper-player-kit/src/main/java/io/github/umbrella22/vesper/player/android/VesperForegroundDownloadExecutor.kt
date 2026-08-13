package io.github.umbrella22.vesper.player.android

import android.content.Context
import androidx.media3.datasource.DefaultDataSource
import java.io.File
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runInterruptible

internal class VesperForegroundDownloadExecutor(
    context: Context?,
    internal val baseDirectory: File?,
    internal val resumePartialDownloads: Boolean = true,
    rangeChunkBytes: Long? = null,
    internal val minProgressBytes: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_BYTES,
    internal val minProgressIntervalMs: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_INTERVAL_MS,
    internal val staleResourcePlanRecoverer: VesperDownloadStaleResourcePlanRecoverer? = null,
) : VesperDownloadExecutor {
    internal val appContext = context?.applicationContext
    internal val rangeChunkBytes = rangeChunkBytes?.takeIf { it > 0L }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobsLock = Any()
    private val jobs = mutableMapOf<VesperDownloadTaskId, Job>()
    internal val recoveredSourcesLock = Any()
    internal val recoveredSources = mutableMapOf<VesperDownloadTaskId, VesperDownloadSource>()
    internal val dataSourceFactory by lazy {
        DefaultDataSource.Factory(checkNotNull(appContext) { "Android Context is required for non-HTTP downloads" })
    }

    override fun prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        scope.launch {
            try {
                reporter.completePreparation(task.taskId, prepareAssetIndexWithRecovery(task, reporter))
            } catch (_: CancellationException) {
                return@launch
            } catch (error: Exception) {
                reporter.fail(
                    task.taskId,
                    VesperDownloadError(
                        code = VesperPlayerErrorCode.BackendFailure,
                        category = VesperPlayerErrorCategory.Network,
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
        launchDownload(withRecoveredSource(task), reporter)
    }

    override fun resume(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(withRecoveredSource(task), reporter)
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
                    } catch (_: CancellationException) {
                        return@launch
                    } catch (recoveryError: Exception) {
                        reporter.fail(
                            task.taskId,
                            VesperDownloadError(
                                code = VesperPlayerErrorCode.BackendFailure,
                                category = VesperPlayerErrorCategory.Network,
                                retriable = false,
                                message = recoveryError.message ?: "android download recovery failed",
                            ),
                        )
                        return@launch
                    }
                    reporter.fail(
                        task.taskId,
                        VesperDownloadError(
                            code = VesperPlayerErrorCode.BackendFailure,
                            category = VesperPlayerErrorCategory.Network,
                            retriable = false,
                            message = error.message ?: "android foreground download failed",
                        ),
                    )
                } catch (error: Exception) {
                    reporter.fail(
                        task.taskId,
                        VesperDownloadError(
                            code = VesperPlayerErrorCode.BackendFailure,
                            category = VesperPlayerErrorCategory.Network,
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


}
