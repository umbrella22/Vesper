package io.github.umbrella22.vesper.player.android

import android.content.Context
import java.io.ByteArrayOutputStream
import java.util.Collections
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Bounds are per session; startup bytes also obey the process-wide cache limit. */
data class VesperSourceSessionConfiguration(
    val maxSources: Int = 128,
    val maxConcurrentPreloads: Int = 2,
    val maxPendingPreloads: Int = 4,
    val maxMemoryBytes: Long = 8 * 1024 * 1024,
) {
    init {
        require(maxSources in 1..512)
        require(maxConcurrentPreloads in 1..4)
        require(maxPendingPreloads in 0..32)
        require(maxMemoryBytes in 0..VesperDashStartupCache.MAX_WARMUP_BYTES)
    }
}

enum class VesperPreloadState { Queued, Running, Completed, Failed, Unsupported, Cancelled }
enum class VesperPreloadGoal { ProgressiveRange, DashSegmentBaseStartup, Unsupported }
enum class VesperPreloadReuse { DownloadOnly, PlaybackReusable, None }
data class VesperPreloadOptions(val maximumBytes: Long = 8 * 1024 * 1024, val timeoutMs: Long = 5_000) {
    init { require(maximumBytes in 1..VesperDashStartupCache.MAX_WARMUP_BYTES); require(timeoutMs in 1..60_000) }
}

/** cacheHit describes this preload's reads, never first-frame readiness or playback reuse. */
data class VesperPreloadResult(
    val taskId: String,
    val handleId: String,
    val state: VesperPreloadState,
    val goal: VesperPreloadGoal,
    val reuse: VesperPreloadReuse = when (goal) {
        VesperPreloadGoal.DashSegmentBaseStartup -> VesperPreloadReuse.PlaybackReusable
        VesperPreloadGoal.ProgressiveRange -> VesperPreloadReuse.DownloadOnly
        VesperPreloadGoal.Unsupported -> VesperPreloadReuse.None
    },
    val actualBytes: Long = 0,
    val cacheHit: Boolean? = null,
    val reasonCode: String? = null,
)

class VesperPreloadTask internal constructor(
    val id: String,
    val handleId: String,
    goal: VesperPreloadGoal,
    private val cancelAction: (VesperPreloadTask) -> Unit,
) {
    private val state = MutableStateFlow(VesperPreloadResult(id, handleId, VesperPreloadState.Queued, goal))
    val snapshot: VesperPreloadResult get() = state.value
    val snapshots: StateFlow<VesperPreloadResult> = state.asStateFlow()
    private val completion = CompletableDeferred<VesperPreloadResult>()
    suspend fun await(): VesperPreloadResult = completion.await()
    fun cancel() = cancelAction(this)
    internal fun publish(result: VesperPreloadResult) {
        while (true) {
            val previous = state.value
            if (previous.state != VesperPreloadState.Queued && previous.state != VesperPreloadState.Running) return
            // StateFlow resumes observers outside its own lock; no session or task
            // monitor may enclose that notification.
            if (state.compareAndSet(previous, result)) break
        }
        if (result.state != VesperPreloadState.Queued && result.state != VesperPreloadState.Running) {
            completion.complete(result)
        }
    }
}

/** Identity only. Descriptors and credentials remain owned by the session. */
class VesperSourceHandle internal constructor(val id: String, private val session: VesperSourceSession) : AutoCloseable {
    internal val accessRevoked = AtomicBoolean(false)
    val sessionId: String get() = session.id
    fun preload(options: VesperPreloadOptions = VesperPreloadOptions()): VesperPreloadTask = session.preload(this, options)
    override fun close() = session.release(this)
    fun dispose() = close()
    fun invalidate() = session.invalidate(this)
    internal fun acquire(): VesperSourceLease = session.acquire(this)
}

/** A retained source survives ordinary handle/session close, but not access invalidation. */
internal class VesperSourceLease(
    val source: VesperPlayerSource,
    private val isValid: () -> Boolean,
) : AutoCloseable {
    private val closed = AtomicBoolean(false)
    fun checkValid() { check(!closed.get() && isValid()) { "source_access_invalidated" } }
    fun sourceForActivation(): VesperPlayerSource { checkValid(); return source }
    fun retain(): VesperSourceLease { checkValid(); return VesperSourceLease(source, isValid) }
    override fun close() { closed.set(true) }
}

/** Independent of players. A session never creates a decoder or changes playback. */
class VesperSourceSession internal constructor(
    val configuration: VesperSourceSessionConfiguration,
    private val progressiveTransport: VesperSequenceWarmupTransport,
    private val dashTransport: DashStartupTransport,
    private val cache: VesperDashStartupCache,
    dispatcher: CoroutineDispatcher,
    private val wallNowMs: () -> Long,
    private val monotonicMs: () -> Long,
) : AutoCloseable {
    constructor(context: Context, configuration: VesperSourceSessionConfiguration = VesperSourceSessionConfiguration()) : this(
        configuration, VesperMedia3SequenceWarmupTransport(context.applicationContext), DashStartupHttpTransport,
        VesperDashStartupCache.shared, Dispatchers.IO, System::currentTimeMillis, { System.nanoTime() / 1_000_000 })

    val id: String = UUID.randomUUID().toString()
    private class Entry(val handle: VesperSourceHandle, val source: VesperPlayerSource, val deadline: Long?) {
        var expired = false
    }
    private class Work(val entry: Entry, val task: VesperPreloadTask, val options: VesperPreloadOptions) {
        var job: Job? = null
        var timeout: Job? = null
        var reservedBytes = 0L
        var cancelled = false
        @Volatile var bytes = 0L
    }
    private val lock = Any()
    private val entries = LinkedHashMap<String, Entry>()
    private val work = LinkedHashMap<String, Work>()
    private val queue = ArrayDeque<Work>()
    private val workers = CoroutineScope(SupervisorJob() + dispatcher)
    private val deadlines = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var running = 0
    private var reservedBytes = 0L
    private var closed = false
    private var invalidated = false
    private val budget get() = minOf(configuration.maxMemoryBytes, VesperDashStartupCache.MAX_WARMUP_BYTES)

    fun register(source: VesperPlayerSource, expiresAtEpochMs: Long? = null): VesperSourceHandle {
        val remaining = expiresAtEpochMs?.let { expiry ->
            val now = wallNowMs()
            require(expiry > now) { "source_expired" }
            // Saturating subtraction avoids wrapping a far-future expiry into an expired value.
            if (now < 0 && expiry > Long.MAX_VALUE + now) Long.MAX_VALUE else expiry - now
        }
        val now = monotonicMs()
        val deadline = remaining?.let { if (now > Long.MAX_VALUE - it) Long.MAX_VALUE else now + it }
        val accepted = source.acceptedCopy().also { it.dashStartupScope = DashStartupScope(owner = id, sourceExpiresAtMs = expiresAtEpochMs) }
        return synchronized(lock) {
            check(!closed && !invalidated) { "source_session_closed" }
            check(entries.size < configuration.maxSources) { "source_capacity_exceeded" }
            val handle = VesperSourceHandle(UUID.randomUUID().toString(), this)
            entries[handle.id] = Entry(handle, accepted, deadline)
            handle
        }
    }

    internal fun acquire(handle: VesperSourceHandle): VesperSourceLease = synchronized(lock) {
        val entry = requireEntry(handle)
        VesperSourceLease(entry.source) { synchronized(lock) { !invalidated && !entry.handle.accessRevoked.get() && !expired(entry) } }
    }

    /** Duplicate requests share the live task; cancellation by any caller cancels that task. */
    fun preload(handle: VesperSourceHandle, options: VesperPreloadOptions = VesperPreloadOptions()): VesperPreloadTask {
        val record = synchronized(lock) {
            val entry = requireEntry(handle)
            work[handle.id]?.let {
                check(!it.cancelled) { "preload_capacity_exceeded" }
                return it.task
            }
            val goal = if (entry.source.drmConfiguration != null) VesperPreloadGoal.Unsupported else when (entry.source.protocol) {
                VesperPlayerSourceProtocol.Dash -> VesperPreloadGoal.DashSegmentBaseStartup
                VesperPlayerSourceProtocol.Progressive, VesperPlayerSourceProtocol.File, VesperPlayerSourceProtocol.Content -> VesperPreloadGoal.ProgressiveRange
                else -> VesperPreloadGoal.Unsupported
            }
            val task = VesperPreloadTask(UUID.randomUUID().toString(), handle.id, goal, ::cancel)
            val record = Work(entry, task, options)
            if (goal != VesperPreloadGoal.Unsupported && budget > 0) {
                check((queue.isEmpty() && canStart(record)) || queue.size < configuration.maxPendingPreloads) { "preload_capacity_exceeded" }
                work[handle.id] = record
                queue.addLast(record)
                record.timeout = deadlines.launch(start = CoroutineStart.LAZY) {
                    delay(options.timeoutMs)
                    terminate(task, VesperPreloadState.Failed, "timeout")
                }
            }
            record
        }
        if (record.task.snapshot.goal == VesperPreloadGoal.Unsupported || budget == 0L) {
            record.task.publish(record.task.snapshot.copy(state = VesperPreloadState.Unsupported, reuse = VesperPreloadReuse.None,
                reasonCode = if (budget == 0L) "cache_disabled" else "unsupported_source"))
        }
        record.timeout?.start()
        pump()
        return record.task
    }

    private fun reservation(record: Work): Long = minOf(budget, record.options.maximumBytes,
        if (record.task.snapshot.goal == VesperPreloadGoal.ProgressiveRange) 64 * 1024L else budget)

    private fun canStart(record: Work): Boolean = running < configuration.maxConcurrentPreloads &&
        reservedBytes + reservation(record) <= budget

    private fun pump() {
        val starts = synchronized(lock) {
            val result = mutableListOf<Job>()
            while (!closed && queue.isNotEmpty() && canStart(queue.first())) {
                val record = queue.removeFirst()
                running++
                record.reservedBytes = reservation(record)
                reservedBytes += record.reservedBytes
                val job = workers.launch(start = CoroutineStart.LAZY) { execute(record) }
                record.job = job
                // Completion tracks physical worker exit, including a cancelled lazy worker.
                job.invokeOnCompletion {
                    synchronized(lock) {
                        running--
                        reservedBytes -= record.reservedBytes
                        if (work[record.entry.handle.id] === record) work.remove(record.entry.handle.id)
                    }
                    record.timeout?.cancel()
                    pump()
                }
                result += job
            }
            result
        }
        starts.forEach { it.start() }
    }

    private fun cancel(task: VesperPreloadTask) = terminate(task, VesperPreloadState.Cancelled, "cancelled")

    private fun terminate(task: VesperPreloadTask, state: VesperPreloadState, reason: String) {
        val record = synchronized(lock) {
            val value = work[task.handleId]?.takeIf { it.task === task } ?: return
            value.cancelled = true
            if (value.job == null) { queue.remove(value); work.remove(task.handleId) }
            value
        }
        record.task.publish(record.task.snapshot.copy(state = state, actualBytes = record.bytes, reasonCode = reason))
        record.job?.cancel()
        record.timeout?.cancel()
        pump()
    }

    internal fun release(handle: VesperSourceHandle) {
        val task = synchronized(lock) {
            if (entries[handle.id]?.handle !== handle) return
            entries.remove(handle.id)
            work[handle.id]?.task
        }
        task?.cancel()
    }

    internal fun invalidate(handle: VesperSourceHandle) {
        check(handle.sessionId == id) { "foreign_source_handle" }
        handle.accessRevoked.set(true)
        release(handle)
    }

    /** Revokes retained leases as well as all new work. Ordinary close preserves retained leases. */
    fun invalidate() { synchronized(lock) { invalidated = true }; close() }
    override fun close() {
        val tasks = synchronized(lock) {
            if (closed) return
            closed = true
            entries.clear()
            work.values.map { it.task }
        }
        tasks.forEach { it.cancel() }
        workers.cancel()
        deadlines.cancel()
    }
    fun dispose() = close()

    private fun expired(entry: Entry): Boolean {
        entry.expired = entry.expired || entry.deadline?.let { monotonicMs() >= it } == true ||
            entry.source.dashStartupScope?.sourceExpiresAtMs?.let { wallNowMs() >= it } == true
        return entry.expired
    }
    private fun requireEntry(handle: VesperSourceHandle): Entry {
        check(!closed && !invalidated) { "source_session_closed" }
        val entry = checkNotNull(entries[handle.id]) { "source_handle_released" }
        check(entry.handle === handle) { "source_handle_released" }
        check(!entry.handle.accessRevoked.get()) { "source_access_invalidated" }
        check(!expired(entry)) { "source_expired" }
        return entry
    }
    private fun commit(record: Work, action: () -> Boolean): Boolean = synchronized(lock) {
        if (closed || invalidated || record.entry.handle.accessRevoked.get() || record.cancelled || entries[record.entry.handle.id] !== record.entry || expired(record.entry)) false
        else action() // Bounded memory commit only; never transport I/O or public callbacks.
    }

    private suspend fun execute(record: Work) {
        val task = record.task
        task.publish(task.snapshot.copy(state = VesperPreloadState.Running))
        try {
            val source = record.entry.source
            val supported = source.protocol in setOf(VesperPlayerSourceProtocol.Dash, VesperPlayerSourceProtocol.Progressive, VesperPlayerSourceProtocol.File, VesperPlayerSourceProtocol.Content)
            if (!supported || source.drmConfiguration != null || budget == 0L) {
                task.publish(task.snapshot.copy(state = VesperPreloadState.Unsupported, reuse = VesperPreloadReuse.None, reasonCode = if (budget == 0L) "cache_disabled" else "unsupported_source"))
                return
            }
            check(!expired(record.entry)) { "source_expired" }
            val maximum = minOf(budget, record.options.maximumBytes)
            val scope = requireNotNull(source.dashStartupScope)
            val result = withTimeout(record.options.timeoutMs) {
                if (source.protocol == VesperPlayerSourceProtocol.Dash) {
                    warmDashStartup(source, scope, record.options.timeoutMs, dashTransport, cache,
                        commitFence = { action -> commit(record, action) }, maximumBytes = maximum,
                        residentMaximumBytes = budget,
                        onBytesLoaded = { record.bytes = it })
                } else {
                    warmProgressive(record, scope, maximum)
                }
            }
            currentCoroutineContext().ensureActive()
            record.bytes = result.first
            task.publish(task.snapshot.copy(state = VesperPreloadState.Completed, actualBytes = result.first, cacheHit = result.second))
        } catch (_: TimeoutCancellationException) {
            task.publish(task.snapshot.copy(state = VesperPreloadState.Failed, actualBytes = record.bytes, reasonCode = "timeout"))
        } catch (_: CancellationException) {
            task.publish(task.snapshot.copy(state = VesperPreloadState.Cancelled, actualBytes = record.bytes, reasonCode = "cancelled"))
        } catch (_: Exception) {
            task.publish(task.snapshot.copy(state = VesperPreloadState.Failed, actualBytes = record.bytes,
                reasonCode = if (expired(record.entry)) "source_expired" else "preload_failed"))
        }
    }

    private suspend fun warmProgressive(record: Work, scope: DashStartupScope, maximum: Long): Pair<Long, Boolean> {
        val source = record.entry.source
        val length = minOf(maximum, 64 * 1024)
        val resource = DashStartupResource(source.uri, 0, length)
        cache.read(scope, resource, source.headers)?.let { return it.bytes.size.toLong() to true }
        val generation = cache.currentGeneration()
        val output = ByteArrayOutputStream()
        progressiveTransport.open(VesperSequenceWarmupReadRequest(source.uri, source.headers, scope.namespace, 0, length, record.options.timeoutMs)).use { stream ->
            val buffer = ByteArray(16 * 1024)
            while (output.size() < length) {
                currentCoroutineContext().ensureActive()
                val count = stream.read(buffer, 0, minOf(buffer.size.toLong(), length - output.size()).toInt())
                if (count < 0) break
                if (count == 0) continue
                output.write(buffer, 0, count)
                record.bytes = output.size().toLong()
            }
        }
        currentCoroutineContext().ensureActive()
        val bytes = output.toByteArray()
        check(bytes.isNotEmpty())
        check(commit(record) { cache.store(scope, listOf(DashStartupBytes(resource.copy(length = bytes.size.toLong()), bytes)), source.headers, generation, budget) })
        return bytes.size.toLong() to false
    }
}

private fun VesperPlayerSource.acceptedCopy(): VesperPlayerSource = copy(
    headers = Collections.unmodifiableMap(LinkedHashMap(headers)),
    drmConfiguration = drmConfiguration?.copy(licenseHeaders = Collections.unmodifiableMap(LinkedHashMap(drmConfiguration.licenseHeaders))),
    externalSubtitles = Collections.unmodifiableList(externalSubtitles.map { it.copy(headers = Collections.unmodifiableMap(LinkedHashMap(it.headers))) }),
)
