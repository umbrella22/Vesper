package io.github.umbrella22.vesper.player.android

import com.sun.net.httpserver.HttpServer
import java.io.File
import java.net.InetSocketAddress
import java.nio.file.Files
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.Continuation
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperForegroundDownloadExecutionTest {
    @Test
    fun cancelledExecutionCannotUnregisterItsReplacement() = runBlocking {
        val directory = Files.createTempDirectory("vesper-execution").toFile()
        val executor = VesperForegroundDownloadExecutor(null, directory)
        val started = CountDownLatch(1)
        val releaseOld = CountDownLatch(1)
        val replacementStarted = CompletableDeferred<Unit>()
        val releaseReplacement = CompletableDeferred<Unit>()
        try {
            val old = executor.launchExecution(17) {
                started.countDown()
                // Model an I/O/callback frame which finishes unwinding after cancellation.
                check(releaseOld.await(5, TimeUnit.SECONDS))
            }
            assertTrue(started.await(5, TimeUnit.SECONDS))
            executor.pause(17)
            val replacement = executor.launchExecution(17) {
                replacementStarted.complete(Unit)
                releaseReplacement.await()
            }
            withTimeout(5_000) { replacementStarted.await() }
            releaseOld.countDown()
            withTimeout(5_000) { old.join() }
            assertFalse(replacement.isCancelled)
            executor.pause(17)
            assertTrue("pause must still find the replacement", replacement.isCancelled)
            withTimeout(5_000) { replacement.join() }
        } finally {
            releaseOld.countDown()
            releaseReplacement.complete(Unit)
            executor.dispose()
            directory.deleteRecursively()
        }
    }

    @Test
    fun preparationHandoffAndFastCompletionKeepOwnershipConsistent() = runBlocking {
        val directory = Files.createTempDirectory("vesper-handoff").toFile()
        val executor = VesperForegroundDownloadExecutor(null, directory)
        val transfer = CompletableDeferred<kotlinx.coroutines.Job>()
        val hold = CompletableDeferred<Unit>()
        try {
            val preparation = executor.launchExecution(19) {
                transfer.complete(executor.launchExecution(19) { hold.await() })
            }
            val current = withTimeout(5_000) { transfer.await() }
            withTimeout(5_000) { preparation.join() }
            executor.pause(19)
            assertTrue(current.isCancelled)
            withTimeout(5_000) { current.join() }
            repeat(100) {
                val fast = executor.launchExecution(19) {}
                withTimeout(5_000) { fast.join() }
            }
            val next = executor.launchExecution(19) { hold.await() }
            executor.dispose()
            assertTrue(next.isCancelled)
            withTimeout(5_000) { next.join() }
        } finally {
            hold.complete(Unit)
            executor.dispose()
            directory.deleteRecursively()
        }
    }

    @Test
    fun removedPreparationRejectsLateNonCooperativeRecovery() = runBlocking {
        val outcome = runDelayedRecovery(removeBeforeReturn = true)
        assertEquals(
            "removed preparation must not recreate files, publish reports, or recache a source",
            RecoveryOutcome(false, false, null, emptyList(), false),
            outcome,
        )
    }

    @Test
    fun activePreparationAcceptsDelayedRecoveryAndCompletes() = runBlocking {
        val outcome = runDelayedRecovery(removeBeforeReturn = false)
        assertTrue(outcome.directoryExists)
        assertTrue(outcome.generatedFileExists)
        assertEquals(recoveredManifest, outcome.generatedText)
        assertEquals(listOf("replaceTaskPlan", "completePreparation"), outcome.events)
        assertTrue(outcome.cachedSource)
    }

    private suspend fun runDelayedRecovery(removeBeforeReturn: Boolean): RecoveryOutcome {
        val baseDirectory = Files.createTempDirectory("vesper-delayed-recovery").toFile()
        val taskDirectory = File(baseDirectory, "recovery-asset")
        val generatedFile = File(taskDirectory, ".generated/index.m3u8")
        check(taskDirectory.mkdirs())
        File(taskDirectory, "existing-task-file").writeText("removed with the task")
        val enteredRecovery = CompletableDeferred<HeldRecovery>()
        val pending = AtomicReference<Continuation<VesperDownloadRecoveredTaskPlan?>?>()
        val closing = AtomicBoolean(false)
        val events = Collections.synchronizedList(mutableListOf<String>())
        val recoverer = object : VesperDownloadStaleResourcePlanRecoverer {
            override suspend fun recoverPlan(
                task: VesperDownloadTaskSnapshot,
                staleResource: VesperDownloadStaleResource,
            ): VesperDownloadRecoveredTaskPlan? {
                val job = checkNotNull(currentCoroutineContext()[Job])
                // A public callback-style recoverer need not use cancellable suspension.
                return suspendCoroutine { continuation ->
                    pending.set(continuation)
                    enteredRecovery.complete(HeldRecovery(job, staleResource))
                    if (closing.get()) pending.getAndSet(null)?.resume(null)
                }
            }
        }
        val executor = VesperForegroundDownloadExecutor(
            context = null,
            baseDirectory = baseDirectory,
            staleResourcePlanRecoverer = recoverer,
        )
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/") { exchange ->
            try {
                val expired = exchange.requestURI.path == "/expired.m3u8"
                val bytes = if (expired) "expired".toByteArray() else recoveredManifest.toByteArray()
                exchange.sendResponseHeaders(if (expired) 403 else 200, bytes.size.toLong())
                exchange.responseBody.write(bytes)
            } finally {
                exchange.close()
            }
        }
        server.start()
        var worker: Job? = null
        try {
            val baseUri = "http://127.0.0.1:${server.address.port}"
            val source = VesperDownloadSource(
                VesperPlayerSource(
                    uri = "$baseUri/expired.m3u8",
                    label = "Expired VOD fixture",
                    kind = VesperPlayerSourceKind.Remote,
                    protocol = VesperPlayerSourceProtocol.Hls,
                ),
            )
            val profile = VesperDownloadProfile(targetDirectory = taskDirectory.absolutePath)
            val task = VesperDownloadTaskSnapshot(
                taskId = 23,
                assetId = "recovery-asset",
                source = source,
                profile = profile,
                state = VesperDownloadState.Preparing,
                progress = VesperDownloadProgressSnapshot(),
                assetIndex = VesperDownloadAssetIndex(),
            )
            val recoveredSource = source.copy(source = source.source.copy(uri = "$baseUri/fresh.m3u8"))
            val recoveredPlan = VesperDownloadRecoveredTaskPlan(
                source = recoveredSource,
                profile = profile,
                assetIndex = VesperDownloadAssetIndex(
                    contentFormat = VesperDownloadContentFormat.HlsSegments,
                    resources = listOf(VesperDownloadResourceRecord(
                        resourceId = "manifest",
                        uri = "$baseUri/fresh.m3u8",
                        relativePath = "index.m3u8",
                        generatedText = recoveredManifest,
                    )),
                    segments = listOf(VesperDownloadSegmentRecord(
                        segmentId = "segment",
                        uri = "$baseUri/segment.ts",
                        relativePath = "segment.ts",
                        sequence = 0,
                        sizeBytes = 1,
                    )),
                ),
            )
            executor.prepare(task, object : VesperDownloadExecutionReporter {
                override fun replaceTaskPlan(
                    taskId: VesperDownloadTaskId,
                    source: VesperDownloadSource,
                    profile: VesperDownloadProfile,
                    assetIndex: VesperDownloadAssetIndex,
                ) { events += "replaceTaskPlan" }
                override fun completePreparation(taskId: VesperDownloadTaskId, assetIndex: VesperDownloadAssetIndex) {
                    events += "completePreparation"
                }
                override fun updateProgress(taskId: VesperDownloadTaskId, receivedBytes: Long, receivedSegments: Int) {
                    events += "updateProgress"
                }
                override fun complete(taskId: VesperDownloadTaskId, completedPath: String?) { events += "complete" }
                override fun fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
                    events += "fail:${error.message}"
                }
            })
            val held = withTimeout(5_000) { enteredRecovery.await() }
            worker = held.job
            assertEquals(VesperDownloadStaleResourcePhase.Prepare, held.staleResource.phase)
            assertEquals(task.taskId, held.staleResource.taskId)
            assertTrue(held.staleResource.message.contains("HTTP 403"))
            assertFalse(generatedFile.exists())
            if (removeBeforeReturn) {
                executor.remove(task)
                assertTrue(held.job.isCancelled)
                assertFalse("remove must actually delete the task directory before the callback", taskDirectory.exists())
            }
            checkNotNull(pending.getAndSet(null)).resume(recoveredPlan)
            withTimeout(5_000) { held.job.join() }
            return RecoveryOutcome(
                directoryExists = taskDirectory.exists(),
                generatedFileExists = generatedFile.isFile,
                generatedText = generatedFile.takeIf(File::isFile)?.readText(),
                events = synchronized(events) { events.toList() },
                cachedSource = synchronized(executor.recoveredSourcesLock) {
                    executor.recoveredSources.containsKey(task.taskId)
                },
            )
        } finally {
            executor.dispose()
            closing.set(true)
            pending.getAndSet(null)?.resume(null)
            try {
                worker?.let { withTimeout(5_000) { it.join() } }
            } finally {
                server.stop(0)
                baseDirectory.deleteRecursively()
            }
        }
    }

    private data class HeldRecovery(val job: Job, val staleResource: VesperDownloadStaleResource)

    private data class RecoveryOutcome(
        val directoryExists: Boolean,
        val generatedFileExists: Boolean,
        val generatedText: String?,
        val events: List<String>,
        val cachedSource: Boolean,
    )

    private val recoveredManifest =
        "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n"
}
