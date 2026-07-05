package io.github.ikaros.vesper.player.android

import android.util.Log

internal class VesperNativePreloadCoordinator(
    private val bindings: PreloadBindings,
    preloadBudgetPolicy: VesperPreloadBudgetPolicy,
) {
    private val resolvedBudget = resolvePreloadBudget(preloadBudgetPolicy)
    private val sessionLock = Any()
    @Volatile
    private var sessionHandle: Long = 0L

    fun ensureSession(): Long {
        synchronized(sessionLock) {
            if (sessionHandle != 0L) {
                return sessionHandle
            }
        }
        // Perform the JNI call outside the synchronized block to avoid holding a
        // monitor during a potentially long-running native call (AGENTS.md rule).
        val handle = bindings.createPreloadSession(resolvedBudget)
        check(handle != 0L) { "native preload session handle must not be zero" }
        synchronized(sessionLock) {
            // Another thread may have raced and created a session first.
            if (sessionHandle != 0L) {
                bindings.disposePreloadSession(handle)
                return sessionHandle
            }
            sessionHandle = handle
        }
        return handle
    }

    fun dispose() {
        val handle =
            synchronized(sessionLock) {
                val handle = sessionHandle
                if (handle == 0L) {
                    return
                }
                sessionHandle = 0L
                handle
            }
        bindings.disposePreloadSession(handle)
    }

    private fun currentSessionHandle(): Long =
        synchronized(sessionLock) {
            sessionHandle
        }

    fun planCurrentSource(source: VesperPlayerSource): List<NativePreloadCommand> {
        if (source.drmConfiguration != null) {
            return emptyList()
        }
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
        val handle = currentSessionHandle()
        if (handle == 0L) {
            return false
        }
        return bindings.completePreloadTask(handle, taskId)
    }

    fun fail(taskId: Long, error: NativeBridgeEvent.Error): Boolean {
        val handle = currentSessionHandle()
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
