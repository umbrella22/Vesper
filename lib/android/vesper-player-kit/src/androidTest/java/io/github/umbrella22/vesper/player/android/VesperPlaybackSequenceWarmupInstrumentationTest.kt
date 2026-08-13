package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Device proof for the sequence warmup path backed by Media3 SimpleCache. */
@RunWith(AndroidJUnit4::class)
class VesperPlaybackSequenceWarmupInstrumentationTest {
    @Test
    fun physicalSimpleCacheReportsMissHitSharedLifetimeAndEviction() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val isolatedCacheRoot = File(context.cacheDir, "vesper-sequence-warmup-instrumentation-cache")
        isolatedCacheRoot.deleteRecursively()
        check(isolatedCacheRoot.mkdirs()) { "failed to create isolated sequence cache directory" }
        val warmupContext = SequenceWarmupTestContext(context, isolatedCacheRoot)
        val fixture = File(context.cacheDir, "vesper-sequence-warmup-device-fixture.bin")
        fixture.outputStream().use { output ->
            val block = ByteArray(64 * 1024) { index -> (index % 251).toByte() }
            repeat(4) { output.write(block) }
        }
        val source =
            VesperPlayerSource(
                uri = Uri.fromFile(fixture).toString(),
                label = "sequence warmup device fixture",
                kind = VesperPlayerSourceKind.Local,
                protocol = VesperPlayerSourceProtocol.Progressive,
            )

        val firstReports = LinkedBlockingQueue<VesperSequenceWarmupReport>()
        val firstExecutor =
            VesperPlaybackSequenceWarmupExecutor(
                context = warmupContext,
                maxDiskBytes = CACHE_BUDGET_BYTES,
                onSourceExpired = { _, _ -> error("local fixture cannot expire") },
                onReport = firstReports::add,
            )
        try {
            val first = warm(firstExecutor, firstReports, intent("item-a-first", 101L, CACHE_KEY_A), source)

            assertEquals(false, first.cacheHit)
            assertEquals(WARMUP_BYTES, first.actualBytes)
            assertEquals(1, first.cacheEntries)
            assertEquals(WARMUP_BYTES, first.cacheBytes)
            assertEquals(0L, first.evictedEntries)

            val secondReports = LinkedBlockingQueue<VesperSequenceWarmupReport>()
            val secondExecutor =
                VesperPlaybackSequenceWarmupExecutor(
                    context = warmupContext,
                    maxDiskBytes = CACHE_BUDGET_BYTES,
                    onSourceExpired = { _, _ -> error("local fixture cannot expire") },
                    onReport = secondReports::add,
                )
            try {
                firstExecutor.close()

                val sharedHit =
                    warm(secondExecutor, secondReports, intent("item-a-hit", 102L, CACHE_KEY_A), source)
                assertEquals(true, sharedHit.cacheHit)
                assertEquals(WARMUP_BYTES, sharedHit.actualBytes)
                assertEquals(1, sharedHit.cacheEntries)
                assertEquals(WARMUP_BYTES, sharedHit.cacheBytes)

                val secondKey = warm(secondExecutor, secondReports, intent("item-b", 103L, CACHE_KEY_B), source)
                assertEquals(false, secondKey.cacheHit)
                assertEquals(WARMUP_BYTES, secondKey.actualBytes)
                assertEquals(1L, secondKey.evictedEntries)
                assertEquals(1, secondKey.cacheEntries)
                assertTrue(secondKey.cacheBytes in 1L..CACHE_BUDGET_BYTES)

                val evictedFirstKey =
                    warm(secondExecutor, secondReports, intent("item-a-after-eviction", 104L, CACHE_KEY_A), source)
                assertEquals(false, evictedFirstKey.cacheHit)
                assertEquals(WARMUP_BYTES, evictedFirstKey.actualBytes)
                assertEquals(1L, evictedFirstKey.evictedEntries)
                assertEquals(1, evictedFirstKey.cacheEntries)
                assertTrue(evictedFirstKey.cacheBytes in 1L..CACHE_BUDGET_BYTES)

                val snapshot = awaitIdle(secondExecutor)
                assertEquals(3L, snapshot.completedJobs)
                assertEquals(1L, snapshot.cacheHits)
                assertEquals(2L, snapshot.cacheMisses)
                assertEquals(2L, snapshot.evictedEntries)
                assertEquals(1, snapshot.cacheEntries)
                assertTrue(snapshot.cacheBytes in 1L..CACHE_BUDGET_BYTES)
                assertEquals(0, snapshot.activeJobs)
            } finally {
                secondExecutor.close()
            }
        } finally {
            firstExecutor.close()
            fixture.delete()
        }
    }

    private class SequenceWarmupTestContext(
        base: Context,
        private val isolatedCacheRoot: File,
    ) : ContextWrapper(base) {
        override fun getApplicationContext(): Context = this

        override fun getCacheDir(): File = isolatedCacheRoot

        override fun getPackageName(): String = "${baseContext.packageName}.sequence-warmup-instrumentation"
    }

    private fun warm(
        executor: VesperPlaybackSequenceWarmupExecutor,
        reports: LinkedBlockingQueue<VesperSequenceWarmupReport>,
        intent: VesperSequenceWarmupIntent,
        source: VesperPlayerSource,
    ): VesperSequenceWarmupReport {
        executor.reconcile(listOf(intent)) { sourceReference, itemId, sourceRevision ->
            assertEquals(intent.sourceReference, sourceReference)
            assertEquals(intent.itemId, itemId)
            assertEquals(intent.sourceRevision, sourceRevision)
            source
        }

        var started = false
        val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
        while (System.nanoTime() < deadlineNanos) {
            val remainingNanos = deadlineNanos - System.nanoTime()
            val report = reports.poll(remainingNanos, TimeUnit.NANOSECONDS) ?: break
            if (report.taskId != intent.warmupTaskId) continue
            if (report.status == "started") {
                started = true
                continue
            }
            assertTrue("warmup did not emit started before ${report.status}", started)
            assertEquals("completed", report.status)
            assertEquals(intent.itemId, report.itemId)
            assertEquals(intent.sourceRevision, report.sourceRevision)
            assertFalse("completed report carried a failure reason", report.reasonCode != null)
            return report
        }
        throw AssertionError("timed out waiting for warmup task ${intent.warmupTaskId}")
    }

    private fun awaitIdle(
        executor: VesperPlaybackSequenceWarmupExecutor,
    ): VesperPlaybackSequenceWarmupSnapshot {
        val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(2)
        while (System.nanoTime() < deadlineNanos) {
            val snapshot = executor.snapshot.value
            if (snapshot.activeJobs == 0) return snapshot
            Thread.sleep(10L)
        }
        return executor.snapshot.value
    }

    private fun intent(
        itemId: String,
        taskId: Long,
        cacheKey: String,
    ): VesperSequenceWarmupIntent =
        VesperSequenceWarmupIntent(
            sessionGeneration = 1L,
            itemId = itemId,
            sourceReference = "source-$itemId",
            sourceRevision = 1L,
            warmupTaskId = taskId,
            cacheKey = cacheKey,
            warmupGoal = "progressiveRange",
            priority = VesperSequenceWarmupPriority.Next,
            expectedBytes = WARMUP_BYTES,
            warmupWindowMs = 10_000L,
        )

    private companion object {
        const val WARMUP_BYTES = 64L * 1024L
        const val CACHE_BUDGET_BYTES = 96L * 1024L
        const val CACHE_KEY_A = "vesper-sequence-cache:v1:device:item-a:progressive:public:1"
        const val CACHE_KEY_B = "vesper-sequence-cache:v1:device:item-b:progressive:public:1"
    }
}
