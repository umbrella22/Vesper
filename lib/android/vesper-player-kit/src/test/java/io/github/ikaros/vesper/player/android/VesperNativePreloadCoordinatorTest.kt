package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativePreloadCoordinatorTest {
    @Test
    fun sparseBudgetPreservesRuntimeDefaultsWhenFieldIsMissing() {
        val bindings = FakePreloadBindings()
        val coordinator =
            VesperNativePreloadCoordinator(
                bindings = bindings,
                preloadBudgetPolicy = VesperPreloadBudgetPolicy(maxDiskBytes = 512L),
            )

        coordinator.ensureSession()

        assertEquals(2, bindings.lastBudget?.maxConcurrentTasks)
        assertEquals(64L * 1024L * 1024L, bindings.lastBudget?.maxMemoryBytes)
        assertEquals(512L, bindings.lastBudget?.maxDiskBytes)
        assertEquals(30_000L, bindings.lastBudget?.warmupWindowMs)
    }

    @Test
    fun planCurrentSourceDrainsRustPreloadCommands() {
        val bindings = FakePreloadBindings(
            drainCommands =
                arrayOf(
                    NativePreloadCommand.Start(
                        NativePreloadTask(
                            taskId = 7L,
                            sourceUri = "https://example.com/current.m3u8",
                            sourceIdentity = "https://example.com/current.m3u8",
                            cacheKey = "https://example.com/current.m3u8",
                            scopeKindOrdinal = 0,
                            scopeId = null,
                            kindOrdinal = 0,
                            selectionHintOrdinal = 1,
                            priorityOrdinal = 0,
                            expectedMemoryBytes = 8L,
                            expectedDiskBytes = 16L,
                            warmupWindowMs = 30_000L,
                            hasExpiresInMs = true,
                            expiresInMs = 30_000L,
                            statusOrdinal = 1,
                            errorCodeOrdinal = 0,
                            errorMessage = null,
                        ),
                    ),
                ),
        )
        val coordinator =
            VesperNativePreloadCoordinator(
                bindings = bindings,
                preloadBudgetPolicy = VesperPreloadBudgetPolicy(),
            )

        val commands =
            coordinator.planCurrentSource(
                VesperPlayerSource.remote(
                    uri = "https://example.com/current.m3u8",
                    label = "Current",
                ),
            )

        assertEquals(1, bindings.plannedCandidates.size)
        assertEquals("https://example.com/current.m3u8", bindings.plannedCandidates.single().sourceUri)
        assertEquals(1, commands.size)
        assertTrue(commands.single() is NativePreloadCommand.Start)
    }

    @Test
    fun completionAndFailureDelegateToBindings() {
        val bindings = FakePreloadBindings()
        val coordinator =
            VesperNativePreloadCoordinator(
                bindings = bindings,
                preloadBudgetPolicy = VesperPreloadBudgetPolicy(),
            )

        coordinator.ensureSession()

        assertTrue(coordinator.complete(11L))
        assertEquals(listOf(11L), bindings.completedTaskIds)

        assertTrue(
            coordinator.fail(
                12L,
                NativeBridgeEvent.Error(
                    message = "warmup failed",
                    codeOrdinal = 3,
                    categoryOrdinal = 7,
                    retriable = false,
                ),
            ),
        )
        assertEquals(listOf(12L), bindings.failedTaskIds)
        assertEquals("warmup failed", bindings.lastFailureMessage)
    }

    @Test
    fun completionReturnsFalseWithoutSession() {
        val coordinator =
            VesperNativePreloadCoordinator(
                bindings = FakePreloadBindings(),
                preloadBudgetPolicy = VesperPreloadBudgetPolicy(),
            )

        assertFalse(coordinator.complete(1L))
    }
}

private class FakePreloadBindings(
    private val resolvedBudget: NativeResolvedPreloadBudgetPolicy =
        NativeResolvedPreloadBudgetPolicy(
            maxConcurrentTasks = 2,
            maxMemoryBytes = 64L * 1024L * 1024L,
            maxDiskBytes = 256L * 1024L * 1024L,
            warmupWindowMs = 30_000L,
        ),
    private val drainCommands: Array<NativePreloadCommand> = emptyArray(),
) : VesperNativePreloadCoordinator.PreloadBindings {
    var lastBudget: NativeResolvedPreloadBudgetPolicy? = null
    var plannedCandidates: List<NativePreloadCandidate> = emptyList()
    val completedTaskIds = mutableListOf<Long>()
    val failedTaskIds = mutableListOf<Long>()
    var lastFailureMessage: String? = null

    override fun createPreloadSession(preloadBudget: NativeResolvedPreloadBudgetPolicy): Long = 17L

    override fun resolvePreloadBudget(
        preloadBudget: NativePreloadBudget,
    ): NativeResolvedPreloadBudgetPolicy {
        lastBudget = NativeResolvedPreloadBudgetPolicy(
            maxConcurrentTasks = if (preloadBudget.hasMaxConcurrentTasks) preloadBudget.maxConcurrentTasks else resolvedBudget.maxConcurrentTasks,
            maxMemoryBytes = if (preloadBudget.hasMaxMemoryBytes) preloadBudget.maxMemoryBytes else resolvedBudget.maxMemoryBytes,
            maxDiskBytes = if (preloadBudget.hasMaxDiskBytes) preloadBudget.maxDiskBytes else resolvedBudget.maxDiskBytes,
            warmupWindowMs = if (preloadBudget.hasWarmupWindowMs) preloadBudget.warmupWindowMs else resolvedBudget.warmupWindowMs,
        )
        return lastBudget!!
    }

    override fun disposePreloadSession(sessionHandle: Long) = Unit

    override fun planPreloadCandidates(
        sessionHandle: Long,
        candidates: Array<NativePreloadCandidate>,
        nowEpochMs: Long,
    ): Array<Long> {
        plannedCandidates = candidates.toList()
        return arrayOf(7L)
    }

    override fun drainPreloadCommands(sessionHandle: Long): Array<NativePreloadCommand> = drainCommands

    override fun completePreloadTask(sessionHandle: Long, taskId: Long): Boolean {
        completedTaskIds += taskId
        return true
    }

    override fun failPreloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
    ): Boolean {
        failedTaskIds += taskId
        lastFailureMessage = message
        return true
    }
}
