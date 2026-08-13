package io.github.umbrella22.vesper.player.android

import android.content.Context
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.HttpDataSource
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.SimpleCache
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.yield
import org.json.JSONObject

private const val MAX_SEQUENCE_WARMUP_BYTES = 512 * 1024L
private const val DEFAULT_SEQUENCE_WARMUP_BYTES = 64 * 1024L
private const val DEFAULT_SEQUENCE_CACHE_BYTES = 256L * 1024L * 1024L
private const val MAX_SEQUENCE_CACHE_KEYS = 4_096
private const val MAX_SEQUENCE_WARMUP_CONCURRENCY = 2
private const val MAX_SEQUENCE_ACTIVE_WARMUP_RECORDS = MAX_SEQUENCE_WARMUP_CONCURRENCY * 2
private const val MAX_SEQUENCE_TERMINAL_WARMUP_RECORDS = 512
private const val DEFAULT_SEQUENCE_WARMUP_TIMEOUT_MS = 5_000L
private const val MAX_SEQUENCE_WARMUP_TIMEOUT_MS = 60_000L

internal enum class VesperSequenceWarmupPriority {
    Current,
    Next,
    Previous,
}

internal data class VesperSequenceWarmupIntent(
    val sessionGeneration: Long,
    val itemId: String,
    val sourceReference: String,
    val sourceRevision: Long,
    val warmupTaskId: Long,
    val cacheKey: String,
    val warmupGoal: String,
    val priority: VesperSequenceWarmupPriority,
    val expectedBytes: Long,
    val warmupWindowMs: Long,
) {
    val goal: String
        get() = warmupGoal

    val targetBytes: Long
        get() = DEFAULT_SEQUENCE_WARMUP_BYTES.coerceIn(1L, MAX_SEQUENCE_WARMUP_BYTES)

    val key: WarmupKey
        get() = WarmupKey(itemId, sourceRevision, warmupTaskId, cacheKey)

    companion object {
        fun fromJson(value: JSONObject): VesperSequenceWarmupIntent? {
            val sessionGeneration = value.optLong("sessionGeneration", 0L)
            val itemId = value.optString("itemId").trim()
            val sourceReference = value.optString("sourceReference").trim()
            val sourceRevision = value.optLong("sourceRevision", 0L)
            val warmupTaskId = value.optLong("warmupTaskId", 0L)
            val warmupGoal = value.optString("warmupGoal").trim()
            val cacheIdentity = value.optJSONObject("cacheIdentity") ?: return null
            val cacheKey = cacheIdentity.optString("canonicalKey").trim()
            if (sessionGeneration <= 0L || itemId.isEmpty() || sourceReference.isEmpty() ||
                sourceRevision <= 0L || warmupTaskId <= 0L
            ) {
                return null
            }
            if (cacheKey.isEmpty() || cacheKey.length > 2_048 || cacheKey.contains("://") ||
                warmupGoal != "progressiveRange"
            ) {
                return null
            }
            val priority =
                when (value.optString("priority")) {
                    "current" -> VesperSequenceWarmupPriority.Current
                    "next" -> VesperSequenceWarmupPriority.Next
                    "previous" -> VesperSequenceWarmupPriority.Previous
                    else -> return null
                }
            val profile = value.optJSONObject("profile")
            return VesperSequenceWarmupIntent(
                sessionGeneration = sessionGeneration,
                itemId = itemId,
                sourceReference = sourceReference,
                sourceRevision = sourceRevision,
                warmupTaskId = warmupTaskId,
                cacheKey = cacheKey,
                warmupGoal = warmupGoal,
                priority = priority,
                expectedBytes = profile?.optLong("expectedMemoryBytes", 0L)?.coerceAtLeast(0L) ?: 0L,
                warmupWindowMs = profile?.optLong("warmupWindowMs", 0L)?.coerceAtLeast(0L) ?: 0L,
            )
        }
    }
}

data class VesperPlaybackSequenceWarmupSnapshot(
    val activeJobs: Int = 0,
    val completedJobs: Long = 0,
    val failedJobs: Long = 0,
    val cancelledJobs: Long = 0,
    val unsupportedJobs: Long = 0,
    val cacheHits: Long = 0,
    val cacheMisses: Long = 0,
    val expectedBytes: Long = 0,
    val actualBytes: Long = 0,
    val evictedEntries: Long = 0,
    val cacheEntries: Int = 0,
    val cacheBytes: Long = 0,
)

internal data class VesperSequenceWarmupReport(
    val sessionGeneration: Long,
    val taskId: Long,
    val itemId: String,
    val sourceRevision: Long,
    val status: String,
    val expectedBytes: Long = 0,
    val actualBytes: Long = 0,
    val cacheHit: Boolean? = null,
    val cacheEntries: Int = 0,
    val cacheBytes: Long = 0,
    val evictedEntries: Long = 0,
    val reasonCode: String? = null,
)

internal data class VesperSequenceCacheInventoryObservation(
    val keys: Set<String>,
    val bytes: Long,
)

internal fun interface VesperSequenceCacheInventoryObserver {
    fun observe(): VesperSequenceCacheInventoryObservation
}

private data class VesperSequenceWarmupInventoryDelta(
    val entries: Int,
    val bytes: Long,
    val evictedEntries: Long,
)

internal data class VesperSequenceWarmupReadRequest(
    val uri: String,
    val headers: Map<String, String>,
    val cacheKey: String,
    val position: Long,
    val length: Long,
    val timeoutMillis: Long,
)

internal interface VesperSequenceWarmupReadStream : AutoCloseable {
    val cacheHit: Boolean

    suspend fun read(
        buffer: ByteArray,
        offset: Int,
        length: Int,
    ): Int
}

internal fun interface VesperSequenceWarmupTransport {
    suspend fun open(request: VesperSequenceWarmupReadRequest): VesperSequenceWarmupReadStream
}

internal class VesperSequenceWarmupHttpStatusException(
    val statusCode: Int,
) : Exception("sequence warmup HTTP status $statusCode")

internal fun vesperSequenceSaturatingAdd(
    left: Long,
    right: Long,
): Long {
    val safeLeft = left.coerceAtLeast(0L)
    val safeRight = right.coerceAtLeast(0L)
    return if (safeLeft > Long.MAX_VALUE - safeRight) Long.MAX_VALUE else safeLeft + safeRight
}

private class VesperMedia3SequenceWarmupTransport(
    private val appContext: Context,
    private val cache: SimpleCache?,
) : VesperSequenceWarmupTransport {
    override suspend fun open(request: VesperSequenceWarmupReadRequest): VesperSequenceWarmupReadStream {
        val cacheHit =
            cache?.let {
                runCatching { it.isCached(request.cacheKey, request.position, request.length) }
                    .getOrDefault(false)
            } ?: false
        val dataSource = buildDataSource(request)
        val dataSpec =
            DataSpec.Builder()
                .setUri(request.uri)
                .setKey(request.cacheKey)
                .setPosition(request.position)
                .setLength(request.length)
                .build()
        try {
            dataSource.open(dataSpec)
        } catch (error: Throwable) {
            runCatching { dataSource.close() }
            throw error
        }
        return object : VesperSequenceWarmupReadStream {
            override val cacheHit: Boolean = cacheHit

            override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int =
                dataSource.read(buffer, offset, length)

            override fun close() {
                dataSource.close()
            }
        }
    }

    private fun buildDataSource(request: VesperSequenceWarmupReadRequest) =
        if (cache == null) {
            DefaultDataSource.Factory(
                appContext,
                DefaultHttpDataSource.Factory()
                    .setConnectTimeoutMs(request.timeoutMillis.toInt())
                    .setReadTimeoutMs(request.timeoutMillis.toInt())
                    .setDefaultRequestProperties(request.headers),
            ).createDataSource()
        } else {
            val upstream =
                DefaultDataSource.Factory(
                    appContext,
                    DefaultHttpDataSource.Factory()
                        .setConnectTimeoutMs(request.timeoutMillis.toInt())
                        .setReadTimeoutMs(request.timeoutMillis.toInt())
                        .setDefaultRequestProperties(request.headers),
                )
            CacheDataSource.Factory()
                .setCache(cache)
                .setUpstreamDataSourceFactory(upstream)
                .setFlags(CacheDataSource.FLAG_IGNORE_CACHE_ON_ERROR)
                .createDataSource()
        }
}

internal data class WarmupKey(
    val itemId: String,
    val sourceRevision: Long,
    val warmupTaskId: Long,
    val cacheKey: String,
)

private class WarmupJobRecord(
    val key: WarmupKey,
) {
    lateinit var job: Job
    val terminal = AtomicBoolean(false)
    val started = AtomicBoolean(false)
    var cancellationRequested = false
}

/**
 * Executes only the v1 progressive range warmup goal. Rust supplies intent and
 * identity; this class owns the URL, request headers, physical cache, and byte
 * accounting on Android.
 */
internal class VesperPlaybackSequenceWarmupExecutor(
    context: Context?,
    maxDiskBytes: Long = DEFAULT_SEQUENCE_CACHE_BYTES,
    private val onSourceExpired: (itemId: String, sourceRevision: Long) -> Unit,
    private val onReport: (VesperSequenceWarmupReport) -> Unit = {},
    dispatcher: CoroutineDispatcher = Dispatchers.IO,
    transport: VesperSequenceWarmupTransport? = null,
    inventoryObserver: VesperSequenceCacheInventoryObserver? = null,
    private val cancellationMarkedCallback: (List<Job>) -> Unit = {},
    private val completionTransitionCallback: (WarmupKey) -> Unit = {},
) : AutoCloseable {
    private val appContext = context?.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + dispatcher)
    private val permits = Semaphore(MAX_SEQUENCE_WARMUP_CONCURRENCY)
    private val jobsLock = Any()
    private val jobs = LinkedHashMap<WarmupKey, WarmupJobRecord>()
    private val terminalKeys = LinkedHashSet<WarmupKey>()
    private val statsLock = Any()
    private var knownCacheKeys = LinkedHashSet<String>()
    private var stats = VesperPlaybackSequenceWarmupSnapshot()
    private val _snapshot = MutableStateFlow(stats)
    private val closed = AtomicBoolean(false)
    private val cache: SimpleCache?
    private val transport: VesperSequenceWarmupTransport
    private val inventoryObserver: VesperSequenceCacheInventoryObserver?
    val snapshot: StateFlow<VesperPlaybackSequenceWarmupSnapshot> = _snapshot.asStateFlow()

    init {
        val diskBytes = maxDiskBytes.coerceIn(0L, DEFAULT_SEQUENCE_CACHE_BYTES)
        if (diskBytes > 0L) {
            val activeContext = requireNotNull(appContext) { "context is required when sequence cache is enabled" }
            val cacheDir = File(activeContext.cacheDir, "vesper-sequence-cache/${stableDirectoryName(activeContext)}")
            cache = VesperSequenceCacheOwner.acquire(activeContext, cacheDir, diskBytes).cache
        } else {
            cache = null
        }
        this.inventoryObserver =
            inventoryObserver ?: cache?.let { physicalCache ->
                VesperSequenceCacheInventoryObserver {
                    VesperSequenceCacheInventoryObservation(
                        keys = physicalCache.keys.asSequence().take(MAX_SEQUENCE_CACHE_KEYS).toSet(),
                        bytes = physicalCache.cacheSpace.coerceAtLeast(0L),
                    )
                }
            }
        this.transport = transport ?: VesperMedia3SequenceWarmupTransport(
            requireNotNull(appContext) { "context is required for the Media3 warmup transport" },
            cache,
        )
        refreshInitialInventory()
    }

    /**
     * Reconciles desired intents. A revision change cancels the old job before
     * the new one starts, so a late completion cannot represent the current source.
     */
    fun reconcile(
        intents: List<VesperSequenceWarmupIntent>,
        sourceLookup: (sourceReference: String, itemId: String, sourceRevision: Long) -> VesperPlayerSource?,
    ) {
        if (closed.get()) return
        val bounded = intents.distinctBy(VesperSequenceWarmupIntent::key).take(MAX_SEQUENCE_WARMUP_CONCURRENCY * 2)
        val desired = bounded.map(VesperSequenceWarmupIntent::key).toSet()
        val toCancel = synchronized(jobsLock) {
            jobs.filterKeys { it !in desired }.values
                .onEach { it.cancellationRequested = true }
                .map { it.job }
        }
        if (toCancel.isNotEmpty()) cancellationMarkedCallback(toCancel)
        // Keep canceled records until their coroutine completion callback. This
        // preserves the concurrency bound and fences late completions.
        toCancel.forEach(Job::cancel)

        bounded.forEach { intent ->
            val shouldResolve = synchronized(jobsLock) {
                !closed.get() &&
                    intent.key !in terminalKeys &&
                    !jobs.containsKey(intent.key) &&
                    jobs.size < MAX_SEQUENCE_ACTIVE_WARMUP_RECORDS
            }
            if (!shouldResolve) return@forEach
            val source = sourceLookup(intent.sourceReference, intent.itemId, intent.sourceRevision)
            if (source == null) {
                if (recordRejectedIntent(intent)) {
                    emitReport(
                        VesperSequenceWarmupReport(
                            sessionGeneration = intent.sessionGeneration,
                            taskId = intent.warmupTaskId,
                            itemId = intent.itemId,
                            sourceRevision = intent.sourceRevision,
                            status = "failed",
                            reasonCode = "source_reference_missing",
                        ),
                    )
                }
                return@forEach
            }
            synchronized(jobsLock) {
                if (closed.get() ||
                    intent.key in terminalKeys ||
                    jobs.containsKey(intent.key) ||
                    jobs.size >= MAX_SEQUENCE_ACTIVE_WARMUP_RECORDS
                ) {
                    return@synchronized
                }
                val record = WarmupJobRecord(intent.key)
                val job = scope.launch(start = CoroutineStart.LAZY) {
                    permits.withPermit {
                        runWarmup(intent, source, record)
                    }
                }
                record.job = job
                jobs[intent.key] = record
                job.invokeOnCompletion {
                    val cancellationAccepted = synchronized(jobsLock) {
                        val shouldCancel = job.isCancelled || record.cancellationRequested
                        val accepted = shouldCancel && acceptTerminalLocked(record, allowCancellationRequested = true)
                        if (jobs[intent.key]?.job === job) jobs.remove(intent.key)
                        accepted
                    }
                    completionTransitionCallback(record.key)
                    if (cancellationAccepted) {
                        updateStats { copy(cancelledJobs = vesperSequenceSaturatingAdd(cancelledJobs, 1L)) }
                        emitReport(
                            VesperSequenceWarmupReport(
                                sessionGeneration = intent.sessionGeneration,
                                taskId = intent.warmupTaskId,
                                itemId = intent.itemId,
                                sourceRevision = intent.sourceRevision,
                                status = "cancelled",
                                expectedBytes = intent.targetBytes,
                            ),
                        )
                    }
                    updateActiveJobs()
                }
                job.start()
            }
        }
        updateActiveJobs()
    }

    fun recordUnsupportedWireIntent() {
        if (closed.get()) return
        updateStats { copy(unsupportedJobs = vesperSequenceSaturatingAdd(unsupportedJobs, 1L)) }
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        val toCancel = synchronized(jobsLock) {
            jobs.values
                .onEach { it.cancellationRequested = true }
                .map { it.job }
        }
        toCancel.forEach(Job::cancel)
        scope.cancel()
        // The process-wide cache owner keeps the physical SimpleCache alive
        // for other sequence executors. This executor only cancels its jobs.
        synchronized(jobsLock) { jobs.clear() }
        synchronized(statsLock) {
            stats = stats.copy(activeJobs = 0)
            _snapshot.value = stats
        }
    }

    private suspend fun runWarmup(
        intent: VesperSequenceWarmupIntent,
        source: VesperPlayerSource,
        record: WarmupJobRecord,
    ) {
        if (source.drmConfiguration != null || source.protocol != VesperPlayerSourceProtocol.Progressive) {
            if (recordTerminal(record) {
                    copy(unsupportedJobs = vesperSequenceSaturatingAdd(unsupportedJobs, 1L))
                }
            ) {
                emitReport(
                    VesperSequenceWarmupReport(
                        sessionGeneration = intent.sessionGeneration,
                        taskId = intent.warmupTaskId,
                        itemId = intent.itemId,
                        sourceRevision = intent.sourceRevision,
                        status = "unsupported",
                        reasonCode = "protocol_or_drm_unsupported",
                    ),
                )
            }
            return
        }
        val targetBytes = intent.targetBytes
        recordExpected(targetBytes)
        if (record.started.compareAndSet(false, true)) {
            emitReport(
                VesperSequenceWarmupReport(
                    sessionGeneration = intent.sessionGeneration,
                    taskId = intent.warmupTaskId,
                    itemId = intent.itemId,
                    sourceRevision = intent.sourceRevision,
                    status = "started",
                    expectedBytes = targetBytes,
                ),
            )
        }
        val timeoutMillis = timeoutMillis(intent)
        val request =
            VesperSequenceWarmupReadRequest(
                uri = source.uri,
                headers = source.headers.filterKeys { !it.equals("Range", ignoreCase = true) },
                cacheKey = intent.cacheKey,
                position = 0L,
                length = targetBytes,
                timeoutMillis = timeoutMillis,
            )
        var bytesRead = 0L
        var cacheHit = false
        try {
            withTimeout(timeoutMillis) {
                val stream = transport.open(request)
                cacheHit = stream.cacheHit
                try {
                    val buffer = ByteArray(16 * 1024)
                    while (bytesRead < targetBytes) {
                        ensureWarmupActive()
                        val requested = minOf(buffer.size.toLong(), targetBytes - bytesRead).toInt()
                        val read = stream.read(buffer, 0, requested)
                        if (read == -1) break
                        if (read == 0) {
                            // A stream may temporarily make no progress. Yield so
                            // timeout and cancellation can run.
                            yield()
                            continue
                        }
                        bytesRead += read
                    }
                } finally {
                    runCatching { stream.close() }
                }
            }
            ensureWarmupActive()
            val inventory = observeInventory()
            ensureWarmupActive()
            if (inventory == null) {
                if (recordTerminal(record) {
                        copy(failedJobs = vesperSequenceSaturatingAdd(failedJobs, 1L))
                    }
                ) {
                    emitReport(
                        VesperSequenceWarmupReport(
                            sessionGeneration = intent.sessionGeneration,
                            taskId = intent.warmupTaskId,
                            itemId = intent.itemId,
                            sourceRevision = intent.sourceRevision,
                            status = "failed",
                            expectedBytes = targetBytes,
                            actualBytes = bytesRead,
                            reasonCode = "cache_inventory_failed",
                        ),
                    )
                }
                return
            }
            var reportInventory = VesperSequenceWarmupInventoryDelta(0, 0L, 0L)
            if (recordTerminal(record) {
                val currentKeys = inventory.keys
                val evicted = knownCacheKeys.count { it !in currentKeys }.toLong()
                knownCacheKeys = LinkedHashSet(currentKeys)
                reportInventory =
                    VesperSequenceWarmupInventoryDelta(
                        entries = currentKeys.size,
                        bytes = inventory.bytes,
                        evictedEntries = evicted,
                    )
                copy(
                    cacheHits = vesperSequenceSaturatingAdd(cacheHits, if (cacheHit) 1L else 0L),
                    cacheMisses = vesperSequenceSaturatingAdd(cacheMisses, if (cacheHit) 0L else 1L),
                    actualBytes = vesperSequenceSaturatingAdd(actualBytes, bytesRead),
                    completedJobs = vesperSequenceSaturatingAdd(completedJobs, 1L),
                    evictedEntries = vesperSequenceSaturatingAdd(evictedEntries, evicted),
                    cacheEntries = currentKeys.size,
                    cacheBytes = inventory.bytes,
                )
            }) {
                emitReport(
                    VesperSequenceWarmupReport(
                        sessionGeneration = intent.sessionGeneration,
                        taskId = intent.warmupTaskId,
                        itemId = intent.itemId,
                        sourceRevision = intent.sourceRevision,
                        status = "completed",
                        expectedBytes = targetBytes,
                        actualBytes = bytesRead,
                        cacheHit = cacheHit,
                        cacheEntries = reportInventory.entries,
                        cacheBytes = reportInventory.bytes,
                        evictedEntries = reportInventory.evictedEntries,
                    ),
                )
            }
        } catch (error: TimeoutCancellationException) {
            if (recordTerminal(record) {
                    copy(failedJobs = vesperSequenceSaturatingAdd(failedJobs, 1L))
                }
            ) {
                emitReport(
                    VesperSequenceWarmupReport(
                        sessionGeneration = intent.sessionGeneration,
                        taskId = intent.warmupTaskId,
                        itemId = intent.itemId,
                        sourceRevision = intent.sourceRevision,
                        status = "failed",
                        expectedBytes = targetBytes,
                        actualBytes = bytesRead,
                        reasonCode = "timeout",
                    ),
                )
            }
        } catch (error: CancellationException) {
            if (recordTerminal(record, allowCancellationRequested = true) {
                    copy(cancelledJobs = vesperSequenceSaturatingAdd(cancelledJobs, 1L))
                }
            ) {
                emitReport(
                    VesperSequenceWarmupReport(
                        sessionGeneration = intent.sessionGeneration,
                        taskId = intent.warmupTaskId,
                        itemId = intent.itemId,
                        sourceRevision = intent.sourceRevision,
                        status = "cancelled",
                        expectedBytes = targetBytes,
                        actualBytes = bytesRead,
                    ),
                )
            }
            throw error
        } catch (error: Throwable) {
            val sourceExpired = isExpired(error)
            if (recordTerminal(record) {
                    copy(failedJobs = vesperSequenceSaturatingAdd(failedJobs, 1L))
                }
            ) {
                if (sourceExpired) {
                    if (!closed.get()) {
                        onSourceExpired(intent.itemId, intent.sourceRevision)
                    }
                }
                emitReport(
                    VesperSequenceWarmupReport(
                        sessionGeneration = intent.sessionGeneration,
                        taskId = intent.warmupTaskId,
                        itemId = intent.itemId,
                        sourceRevision = intent.sourceRevision,
                        status = "failed",
                        expectedBytes = targetBytes,
                        actualBytes = bytesRead,
                        reasonCode = if (sourceExpired) "source_expired" else "warmup_failed",
                    ),
                )
            }
        }
    }

    private fun isExpired(error: Throwable): Boolean {
        var current: Throwable? = error
        repeat(8) {
            if (current is HttpDataSource.InvalidResponseCodeException &&
                current.responseCode in setOf(401, 403, 410)
            ) return true
            if (current is VesperSequenceWarmupHttpStatusException &&
                current.statusCode in setOf(401, 403, 410)
            ) return true
            current = current?.cause
        }
        return false
    }

    private suspend fun ensureWarmupActive() {
        if (closed.get()) throw CancellationException("sequence warmup executor is closed")
        currentCoroutineContext().ensureActive()
    }

    private fun timeoutMillis(intent: VesperSequenceWarmupIntent): Long =
        (intent.warmupWindowMs.takeIf { it > 0 } ?: DEFAULT_SEQUENCE_WARMUP_TIMEOUT_MS)
            .coerceIn(1L, MAX_SEQUENCE_WARMUP_TIMEOUT_MS)

    private fun updateActiveJobs() {
        synchronized(jobsLock) {
            val count = jobs.size
            synchronized(statsLock) {
                stats = stats.copy(activeJobs = count)
                _snapshot.value = stats
            }
        }
    }

    private fun observeInventory(): VesperSequenceCacheInventoryObservation? {
        val observer = inventoryObserver
            ?: return VesperSequenceCacheInventoryObservation(emptySet(), 0L)
        return runCatching { observer.observe() }
            .getOrNull()
            ?.let { observation ->
                VesperSequenceCacheInventoryObservation(
                    keys = observation.keys.asSequence().take(MAX_SEQUENCE_CACHE_KEYS).toSet(),
                    bytes = observation.bytes.coerceAtLeast(0L),
                )
            }
    }

    private fun refreshInitialInventory() {
        val inventory = observeInventory() ?: return
        synchronized(statsLock) {
            knownCacheKeys = LinkedHashSet(inventory.keys)
            stats = stats.copy(cacheEntries = inventory.keys.size, cacheBytes = inventory.bytes)
            _snapshot.value = stats
        }
    }

    private fun recordExpected(value: Long) =
        updateStats { copy(expectedBytes = vesperSequenceSaturatingAdd(expectedBytes, value)) }

    private fun recordTerminal(
        record: WarmupJobRecord,
        allowCancellationRequested: Boolean = false,
        transform: VesperPlaybackSequenceWarmupSnapshot.() -> VesperPlaybackSequenceWarmupSnapshot,
    ): Boolean {
        val accepted = synchronized(jobsLock) { acceptTerminalLocked(record, allowCancellationRequested) }
        if (!accepted) return false
        updateStats(transform)
        return true
    }

    private fun acceptTerminalLocked(
        record: WarmupJobRecord,
        allowCancellationRequested: Boolean,
    ): Boolean {
        if (closed.get() ||
            (!allowCancellationRequested && record.cancellationRequested) ||
            !record.terminal.compareAndSet(false, true)
        ) {
            return false
        }
        terminalKeys += record.key
        trimTerminalKeysLocked()
        return true
    }

    private fun recordRejectedIntent(intent: VesperSequenceWarmupIntent): Boolean {
        val accepted = synchronized(jobsLock) {
            if (closed.get() ||
                jobs.containsKey(intent.key) ||
                !terminalKeys.add(intent.key)
            ) {
                return@synchronized false
            }
            trimTerminalKeysLocked()
            true
        }
        if (accepted) {
            updateStats { copy(failedJobs = vesperSequenceSaturatingAdd(failedJobs, 1L)) }
        }
        return accepted
    }

    private fun trimTerminalKeysLocked() {
        while (terminalKeys.size > MAX_SEQUENCE_TERMINAL_WARMUP_RECORDS) {
            terminalKeys.firstOrNull()?.let(terminalKeys::remove) ?: break
        }
    }

    private fun emitReport(report: VesperSequenceWarmupReport) {
        if (closed.get()) return
        runCatching { onReport(report) }
    }

    private fun updateStats(transform: VesperPlaybackSequenceWarmupSnapshot.() -> VesperPlaybackSequenceWarmupSnapshot) {
        synchronized(statsLock) {
            if (closed.get()) return
            stats = stats.transform()
            _snapshot.value = stats
        }
    }

    private fun stableDirectoryName(context: Context): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(context.packageName.toByteArray())
        return digest.joinToString("") { byte -> "%02x".format(byte) }.take(32)
    }
}
