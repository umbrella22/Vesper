package io.github.ikaros.vesper.player.android

import android.util.Log

internal class VesperNativePreloadCoordinator(
    private val bindings: PreloadBindings,
    preloadBudgetPolicy: VesperPreloadBudgetPolicy,
) {
    private val resolvedBudget = resolvePreloadBudget(preloadBudgetPolicy)
    private var sessionHandle: Long = 0L

    fun ensureSession(): Long {
        if (sessionHandle != 0L) {
            return sessionHandle
        }
        sessionHandle = bindings.createPreloadSession(resolvedBudget)
        check(sessionHandle != 0L) { "native preload session handle must not be zero" }
        return sessionHandle
    }

    fun dispose() {
        if (sessionHandle == 0L) {
            return
        }
        bindings.disposePreloadSession(sessionHandle)
        sessionHandle = 0L
    }

    fun planCurrentSource(source: VesperPlayerSource): List<NativePreloadCommand> {
        val handle = ensureSession()
        val taskIds =
            bindings.planPreloadCandidates(
                sessionHandle = handle,
                candidates = arrayOf(source.toCurrentPreloadCandidate(resolvedBudget)),
                nowEpochMs = System.currentTimeMillis(),
            )
        if (taskIds.isNotEmpty()) {
            runCatching {
                Log.i(TAG, "planned preload tasks=${taskIds.toList()} source=${source.uri}")
            }
        }
        return bindings.drainPreloadCommands(handle).toList()
    }

    fun complete(taskId: Long): Boolean {
        val handle = sessionHandle
        if (handle == 0L) {
            return false
        }
        return bindings.completePreloadTask(handle, taskId)
    }

    fun fail(taskId: Long, error: NativeBridgeEvent.Error): Boolean {
        val handle = sessionHandle
        if (handle == 0L) {
            return false
        }
        return bindings.failPreloadTask(
            sessionHandle = handle,
            taskId = taskId,
            codeOrdinal = error.codeOrdinal,
            categoryOrdinal = error.categoryOrdinal,
            retriable = error.retriable,
            message = error.message,
        )
    }

    internal interface PreloadBindings {
        fun createPreloadSession(preloadBudget: NativeResolvedPreloadBudgetPolicy): Long

        fun resolvePreloadBudget(preloadBudget: NativePreloadBudget): NativeResolvedPreloadBudgetPolicy

        fun disposePreloadSession(sessionHandle: Long)

        fun planPreloadCandidates(
            sessionHandle: Long,
            candidates: Array<NativePreloadCandidate>,
            nowEpochMs: Long,
        ): Array<Long>

        fun drainPreloadCommands(sessionHandle: Long): Array<NativePreloadCommand>

        fun completePreloadTask(sessionHandle: Long, taskId: Long): Boolean

        fun failPreloadTask(
            sessionHandle: Long,
            taskId: Long,
            codeOrdinal: Int,
            categoryOrdinal: Int,
            retriable: Boolean,
            message: String,
        ): Boolean
    }

    internal object NativeJniPreloadBindings : PreloadBindings {
        override fun createPreloadSession(preloadBudget: NativeResolvedPreloadBudgetPolicy): Long =
            VesperNativeJni.createPreloadSession(preloadBudget)

        override fun resolvePreloadBudget(
            preloadBudget: NativePreloadBudget,
        ): NativeResolvedPreloadBudgetPolicy =
            VesperNativeJni.resolvePreloadBudget(preloadBudget)

        override fun disposePreloadSession(sessionHandle: Long) =
            VesperNativeJni.disposePreloadSession(sessionHandle)

        override fun planPreloadCandidates(
            sessionHandle: Long,
            candidates: Array<NativePreloadCandidate>,
            nowEpochMs: Long,
        ): Array<Long> = VesperNativeJni.planPreloadCandidates(sessionHandle, candidates, nowEpochMs)

        override fun drainPreloadCommands(sessionHandle: Long): Array<NativePreloadCommand> =
            VesperNativeJni.drainPreloadCommands(sessionHandle)

        override fun completePreloadTask(sessionHandle: Long, taskId: Long): Boolean =
            VesperNativeJni.completePreloadTask(sessionHandle, taskId)

        override fun failPreloadTask(
            sessionHandle: Long,
            taskId: Long,
            codeOrdinal: Int,
            categoryOrdinal: Int,
            retriable: Boolean,
            message: String,
        ): Boolean =
            VesperNativeJni.failPreloadTask(
                sessionHandle,
                taskId,
                codeOrdinal,
                categoryOrdinal,
                retriable,
                message,
            )
    }

    private fun resolvePreloadBudget(policy: VesperPreloadBudgetPolicy): NativeResolvedPreloadBudgetPolicy =
        bindings.resolvePreloadBudget(policy.toNativePayload())
}

private fun VesperPreloadBudgetPolicy.toNativePayload(): NativePreloadBudget =
    NativePreloadBudget(
        hasMaxConcurrentTasks = maxConcurrentTasks != null,
        maxConcurrentTasks = maxConcurrentTasks ?: 0,
        hasMaxMemoryBytes = maxMemoryBytes != null,
        maxMemoryBytes = maxMemoryBytes ?: 0L,
        hasMaxDiskBytes = maxDiskBytes != null,
        maxDiskBytes = maxDiskBytes ?: 0L,
        hasWarmupWindowMs = warmupWindowMs != null,
        warmupWindowMs = warmupWindowMs ?: 0L,
    )

private fun VesperPlayerSource.toCurrentPreloadCandidate(
    budget: NativeResolvedPreloadBudgetPolicy,
): NativePreloadCandidate =
    NativePreloadCandidate(
        sourceUri = uri,
        scopeKindOrdinal = 0,
        scopeId = null,
        kindOrdinal = 0,
        selectionHintOrdinal = 1,
        priorityOrdinal = 0,
        expectedMemoryBytes = budget.maxMemoryBytes,
        expectedDiskBytes = budget.maxDiskBytes,
        hasTtlMs = true,
        ttlMs = budget.warmupWindowMs,
        hasWarmupWindowMs = true,
        warmupWindowMs = budget.warmupWindowMs,
    )

private const val TAG = "VesperPreloadCoordinator"
