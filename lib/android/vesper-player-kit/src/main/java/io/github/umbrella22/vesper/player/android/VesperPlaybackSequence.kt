package io.github.umbrella22.vesper.player.android

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject
import android.os.Handler
import android.os.Looper
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

class VesperPlaybackSequenceException(
    val code: String,
    message: String = code,
) : IllegalStateException(message)

enum class VesperPlaybackSequenceMode(internal val wireName: String) {
    Finite("finite"),
    Replenishable("replenishable"),
}

enum class VesperPlaybackSequenceMediaKind(internal val wireName: String) {
    Vod("vod"),
    Live("live"),
    LiveDvr("liveDvr"),
}

data class VesperPlaybackSequenceConfiguration(
    val sequenceId: String,
    val mode: VesperPlaybackSequenceMode = VesperPlaybackSequenceMode.Finite,
    val historyLimit: Int = 16,
    val forwardWindow: Int = 1,
    val refillThreshold: Int = 1,
    val maxItems: Int = 512,
    val maxPendingRequests: Int = 32,
    val maxEvents: Int = 512,
    val requestTimeoutMs: Long = 15_000,
    val sourceExpiryLeadMs: Long = 15_000,
    val maxSourceRegistryEntries: Int = 1_024,
) {
    init {
        require(sequenceId.isNotBlank()) { "sequenceId must not be blank" }
        require(maxItems in 1..512) { "maxItems must be between 1 and 512" }
        require(maxPendingRequests in 1..512) {
            "maxPendingRequests must be between 1 and 512"
        }
        require(maxEvents in 1..1_024) { "maxEvents must be between 1 and 1024" }
        require(maxSourceRegistryEntries in maxItems..4_096) {
            "maxSourceRegistryEntries must cover maxItems and remain bounded"
        }
    }
}

data class VesperPlaybackSequenceContentIdentity(
    val providerNamespace: String,
    val value: String,
)

internal data class VesperPlaybackSequenceCacheIdentity(
    val providerNamespace: String,
    val contentIdentity: String,
    val renditionIdentity: String,
    val resourceIdentity: String,
    val accessPartition: String,
    val sourceRevision: Long,
)

data class VesperPlaybackSequencePreloadProfile(
    val expectedMemoryBytes: Long = 0,
    val expectedDiskBytes: Long = 0,
    val ttlMs: Long? = null,
    val warmupWindowMs: Long? = null,
)

data class VesperPlaybackSequenceItem(
    val itemId: String,
    val contentIdentity: VesperPlaybackSequenceContentIdentity,
    val mediaKind: VesperPlaybackSequenceMediaKind = VesperPlaybackSequenceMediaKind.Vod,
    val source: VesperSourceHandle? = null,
    val providerMetadataRef: String? = null,
    val preloadProfile: VesperPlaybackSequencePreloadProfile =
        VesperPlaybackSequencePreloadProfile(),
) {
    init {
        require(itemId.isNotBlank()) { "itemId must not be blank" }
        require(contentIdentity.providerNamespace.isNotBlank()) {
            "provider namespace must not be blank"
        }
        require(contentIdentity.value.isNotBlank()) { "content identity must not be blank" }

    }
}

internal data class VesperPlaybackSequenceResolvedSource(
    val sessionGeneration: Long,
    val requestId: Long,
    val resolutionAttemptId: Long,
    val itemId: String,
    val expectedSourceRevision: Long,
    val source: VesperSourceHandle,
)

/** Opaque resolver fence decoded from a source-resolution event, not supplied by application policy. */
class VesperPlaybackSequenceSourceRequest private constructor(
    internal val sessionGeneration: Long,
    internal val requestId: Long,
    internal val resolutionAttemptId: Long,
    val itemId: String,
    internal val expectedSourceRevision: Long,
) {
    companion object {
        fun fromWireMap(value: Map<String, Any?>): VesperPlaybackSequenceSourceRequest = VesperPlaybackSequenceSourceRequest(
            value.requestInteger("sessionGeneration"),
            value.requestInteger("requestId"),
            value.requestInteger("resolutionAttemptId"),
            value["itemId"] as? String ?: error("missing_item_id"),
            value.requestInteger("expectedSourceRevision"),
        ).also { require(it.sessionGeneration > 0 && it.requestId > 0 && it.resolutionAttemptId > 0 && it.expectedSourceRevision >= 0) }
    }
}

private fun Map<String, Any?>.requestInteger(key: String): Long = when (val value = this[key]) {
    is Int -> value.toLong()
    is Long -> value
    else -> throw IllegalArgumentException("invalid_$key")
}

data class VesperPlaybackSequenceItemState(
    val itemId: String,
    val index: Int,
    val isActive: Boolean,
    val mediaKind: String,
    val sourceState: String,
    val sourceRevision: Long,
    internal val sourceReference: String?,
)

data class VesperPlaybackSequenceSnapshot(
    val sequenceId: String,
    val sessionGeneration: Long,
    val activationEpoch: Long,
    val items: List<VesperPlaybackSequenceItemState>,
    val activeItemId: String?,
    val pendingRequests: List<Map<String, Any?>>,
    val requestFailures: List<Map<String, Any?>>,
    val previousEndReached: Boolean,
    val nextEndReached: Boolean,
    val droppedEvents: Long,
    val warmupTasks: List<Map<String, Any?>> = emptyList(),
    val warmupStats: Map<String, Any?> = emptyMap(),
) {
    /** A bounded, URL-free payload for Flutter/channel consumers. */
    fun toWireMap(): Map<String, Any?> =
        mapOf(
            "sequenceId" to sequenceId,
            "sessionGeneration" to sessionGeneration,
            "activationEpoch" to activationEpoch,
            "items" to items.map { item ->
                mapOf(
                    "index" to item.index,
                    "isActive" to item.isActive,
                    "item" to mapOf(
                        "itemId" to item.itemId,
                        "mediaKind" to item.mediaKind,
                        "sourceState" to mapOf(
                            "state" to item.sourceState,
                            "sourceRevision" to item.sourceRevision,
                            "sourceReference" to item.sourceReference,
                        ),
                    ),
                )
            },
            "activeItemId" to activeItemId,
            "pendingRequests" to pendingRequests,
            "requestFailures" to requestFailures,
            "previousEndReached" to previousEndReached,
            "nextEndReached" to nextEndReached,
            "droppedEvents" to droppedEvents,
            "warmupTasks" to warmupTasks,
            "warmupStats" to warmupStats,
        )

    companion object {
        internal fun empty(sequenceId: String) =
            VesperPlaybackSequenceSnapshot(
                sequenceId = sequenceId,
                sessionGeneration = 1,
                activationEpoch = 0,
                items = emptyList(),
                activeItemId = null,
                pendingRequests = emptyList(),
                requestFailures = emptyList(),
                previousEndReached = false,
                nextEndReached = false,
                droppedEvents = 0,
            )
    }
}

data class VesperPlaybackSequenceEvent(
    val eventSequence: Long,
    val sessionGeneration: Long,
    val event: Map<String, Any?>,
) {
    val sourceRequest: VesperPlaybackSequenceSourceRequest?
        get() = if (event["type"] == "sourceResolutionRequired")
            VesperPlaybackSequenceSourceRequest.fromWireMap(event + ("sessionGeneration" to sessionGeneration)) else null
}

internal fun VesperPlaybackSequenceEvent.toWireMap(): Map<String, Any?> =
    mapOf(
        "type" to "event",
        "sequenceId" to (event["sequenceId"] ?: ""),
        "sessionGeneration" to sessionGeneration,
        "eventSequence" to eventSequence,
        "event" to event,
    )

class VesperPlaybackSequence(
    val configuration: VesperPlaybackSequenceConfiguration,
) : VesperPlaybackSequenceAttachment {
    private data class SourceRegistryEntry(
        val itemId: String,
        val sourceRevision: Long,
        val handle: VesperSourceHandle,
        val lease: VesperSourceLease,
    ) { val source: VesperPlayerSource get() = lease.sourceForActivation() }

    private val isDisposed = AtomicBoolean(false)
    private val attachmentEpoch = AtomicLong(0)
    private val sourceReferenceCounter = AtomicLong(1)
    private val ownershipLock = Any()
    private val sourceRegistry = LinkedHashMap<String, SourceRegistryEntry>()
    private var controller: VesperPlayerController? = null
    @Volatile private var navigationJob: Job? = null
    @Volatile private var navigationItemId: String? = null
    private val preloadObservers = java.util.concurrent.ConcurrentHashMap<WarmupKey, Job>()
    private val preloadScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    @Volatile private var warmupStats = VesperPlaybackSequenceWarmupSnapshot()
    private val mainHandler = Handler(Looper.getMainLooper())

    private val sessionHandle: Long =
        VesperNativeJni.createSequenceSession(configuration.toConfigJson().toString())

    private val _snapshot =
        MutableStateFlow(VesperPlaybackSequenceSnapshot.empty(configuration.sequenceId))
    val snapshot: StateFlow<VesperPlaybackSequenceSnapshot> = _snapshot.asStateFlow()

    private val _events = MutableSharedFlow<VesperPlaybackSequenceEvent>(extraBufferCapacity = 512)
    val events: SharedFlow<VesperPlaybackSequenceEvent> = _events.asSharedFlow()

    /** Host-observed physical warmup and cache accounting for this sequence. */
    fun warmupSnapshot(): VesperPlaybackSequenceWarmupSnapshot =
        warmupStats.copy(activeJobs = preloadObservers.values.count { it.isActive })

    init {
        try {
            check(sessionHandle != 0L) { "native sequence session handle must not be zero" }
        } catch (error: Throwable) {
            if (sessionHandle != 0L) {
                runCatching { VesperNativeJni.disposeSequenceSession(sessionHandle) }
            }
            throw error
        }
    }

    fun attach(target: VesperPlayerController) {
        checkActive()
        attachmentEpoch.updateAndGet { current ->
            if (current == Long.MAX_VALUE) 1L else current + 1L
        }
        var attached = false
        synchronized(ownershipLock) {
            if (controller != null) {
                throw VesperPlaybackSequenceException("already_attached")
            }
            try {
                target.attachPlaybackSequence(this)
                controller = target
                attached = true
            } catch (error: Throwable) {
                throw error
            }
        }
        try {
            check(attached) { "sequence attachment did not complete" }
            pumpPreloadIntents()
        } catch (error: Throwable) {
            detach()
            throw error
        }
    }

    fun detach() {
        navigationJob?.cancel(CancellationException("sequence_detached"))
        navigationJob = null
        attachmentEpoch.updateAndGet { current ->
            if (current == Long.MAX_VALUE) 1L else current + 1L
        }
        val target = synchronized(ownershipLock) {
            controller.also {
                controller = null
            }
        }
        preloadObservers.values.toList().forEach { it.cancel() }
        preloadObservers.clear()
        target?.detachPlaybackSequence(this)
    }

    override fun onControllerDisposed(controller: VesperPlayerController) {
        navigationJob?.cancel(CancellationException("controller_disposed"))
        navigationJob = null
        attachmentEpoch.updateAndGet { current ->
            if (current == Long.MAX_VALUE) 1L else current + 1L
        }
        synchronized(ownershipLock) {
            if (this.controller === controller) {
                this.controller = null
                sourceRegistry.values.forEach { it.lease.close() }
                sourceRegistry.clear()
            }
        }
        preloadObservers.values.toList().forEach { it.cancel() }
        preloadObservers.clear()
        runCatching {
            execute(
                JSONObject()
                    .put("type", "replace")
                    .put("items", JSONArray())
                    .putNullable("activeItemId", null),
                refresh = false,
            )
        }
    }

    fun dispose() {
        if (!isDisposed.compareAndSet(false, true)) {
            return
        }
        detach()
        synchronized(ownershipLock) {
            sourceRegistry.values.forEach { it.lease.close() }
            sourceRegistry.clear()
        }
        preloadScope.cancel()
        VesperNativeJni.disposeSequenceSession(sessionHandle)
    }

    fun replace(
        items: List<VesperPlaybackSequenceItem>,
    ) {
        checkBatch(items)
        val stagedRegistry = LinkedHashMap<String, SourceRegistryEntry>()
        val itemPayloads = JSONArray()
        try { items.forEach { item -> itemPayloads.put(item.toJson(stagedRegistry)) } }
        catch (error: Throwable) { stagedRegistry.values.forEach { it.lease.close() }; throw error }
        val command = JSONObject().put("type", "replace").put("items", itemPayloads)
        try { execute(command, refresh = false) }
        catch (error: Throwable) { stagedRegistry.values.forEach { it.lease.close() }; throw error }
        navigationJob?.cancel(CancellationException("activation_superseded"))
        synchronized(ownershipLock) {
            sourceRegistry.values.forEach { it.lease.close() }
            sourceRegistry.clear()
            sourceRegistry.putAll(stagedRegistry)
        }
        refreshAndPump()
    }

    fun append(
        sessionGeneration: Long,
        requestId: Long,
        anchorItemId: String?,
        items: List<VesperPlaybackSequenceItem>,
        endReached: Boolean,
    ): Int =
        submitItemsResponse(
            type = "append",
            sessionGeneration = sessionGeneration,
            requestId = requestId,
            anchorItemId = anchorItemId,
            items = items,
            endReached = endReached,
        )

    fun prepend(
        sessionGeneration: Long,
        requestId: Long,
        anchorItemId: String?,
        items: List<VesperPlaybackSequenceItem>,
        endReached: Boolean,
    ): Int =
        submitItemsResponse(
            type = "prepend",
            sessionGeneration = sessionGeneration,
            requestId = requestId,
            anchorItemId = anchorItemId,
            items = items,
            endReached = endReached,
        )

    fun remove(itemId: String): Boolean {
        val removed = executeAndRefresh(JSONObject().put("type", "remove").put("itemId", itemId)).optBoolean("removed")
        if (removed && navigationItemId == itemId) {
            navigationJob?.cancel(CancellationException("activation_item_removed"))
        }
        pruneRegistry()
        return removed
    }

    suspend fun activate(itemId: String, options: VesperSourceActivationOptions = VesperSourceActivationOptions()): VesperSourceActivationResult =
        navigate(JSONObject().put("type", "setActive").put("itemId", itemId), options)
            ?: throw VesperPlaybackSequenceException("activation_item_unavailable")

    suspend fun next(options: VesperSourceActivationOptions = VesperSourceActivationOptions()): VesperSourceActivationResult? =
        navigate(JSONObject().put("type", "next"), options)

    suspend fun previous(options: VesperSourceActivationOptions = VesperSourceActivationOptions()): VesperSourceActivationResult? =
        navigate(JSONObject().put("type", "previous"), options)

    private suspend fun navigate(command: JSONObject, options: VesperSourceActivationOptions): VesperSourceActivationResult? =
        withContext(Dispatchers.Main.immediate) {
            checkActive()
            val target = synchronized(ownershipLock) { controller } ?: throw VesperPlaybackSequenceException("not_attached")
            val job = currentCoroutineContext().job
            navigationJob?.cancel(CancellationException("activation_superseded"))
            navigationJob = job
            try {
                withTimeout(options.timeoutMs) {
                    val beforeEpoch = snapshot.value.activationEpoch
                    executeAndRefresh(command)
                    if (command.optString("type") != "setActive" && snapshot.value.activationEpoch == beforeEpoch) return@withTimeout null
                    val itemId = snapshot.value.activeItemId ?: return@withTimeout null
                    navigationItemId = itemId
                    val generation = snapshot.value.sessionGeneration
                    val activationEpoch = snapshot.value.activationEpoch
                    val resolved = snapshot.first { value ->
                        checkActive()
                        check(value.sessionGeneration == generation && value.activationEpoch == activationEpoch &&
                            value.activeItemId == itemId) { "activation_superseded" }
                        val state = value.items.firstOrNull { it.itemId == itemId }
                        check(state != null) { "activation_item_removed" }
                        check(state.sourceState != "failed") { "source_resolution_failed" }
                        state.sourceReference != null
                    }
                    val active = resolved.items.first { it.itemId == itemId }
                    val entry = synchronized(ownershipLock) { sourceRegistry[active.sourceReference] }
                        ?: throw VesperPlaybackSequenceException("stale_source_registry_entry")
                    val lease = entry.lease.retain()
                    target.activateSequenceSource(this@VesperPlaybackSequence, entry.handle.sessionId, entry.handle.id, lease, options)
                }
            } finally {
                if (navigationJob === job) {
                    navigationJob = null
                    navigationItemId = null
                }
            }
        }

    fun submitResolvedSource(request: VesperPlaybackSequenceSourceRequest, source: VesperSourceHandle) =
        submitResolvedSource(VesperPlaybackSequenceResolvedSource(request.sessionGeneration, request.requestId,
            request.resolutionAttemptId, request.itemId, request.expectedSourceRevision, source))

    private fun submitResolvedSource(resolved: VesperPlaybackSequenceResolvedSource) {
        checkActive()
        val revision = Math.addExact(resolved.expectedSourceRevision, 1)
        val sourceReference = nextSourceReference()
        val entry = SourceRegistryEntry(resolved.itemId, revision, resolved.source, resolved.source.acquire())
        try { synchronized(ownershipLock) {
            ensureRegistryCapacity(1)
            sourceRegistry[sourceReference] = entry
        } } catch (error: Throwable) { entry.lease.close(); throw error }
        val source =
            JSONObject()
                .put("sessionGeneration", resolved.sessionGeneration)
                .put("requestId", resolved.requestId)
                .put("resolutionAttemptId", resolved.resolutionAttemptId)
                .put("itemId", resolved.itemId)
                .put("expectedSourceRevision", resolved.expectedSourceRevision)
                .put("sourceRevision", revision)
                .put("sourceReference", sourceReference)
                .put("cacheIdentity", resolved.source.cacheIdentity(revision).toJson())
                .put("warmupGoal", entry.source.sequenceWarmupGoal())
                .putNullable("expiresAtEpochMs", entry.source.dashStartupScope?.sourceExpiresAtMs)
        try {
            executeAndRefresh(
                JSONObject().put("type", "submitResolvedSource").put("source", source)
            )
        } catch (error: Throwable) {
            synchronized(ownershipLock) { sourceRegistry.remove(sourceReference) }?.lease?.close()
            throw error
        }
        pruneRegistry()
    }

    fun markSourceExpired(itemId: String, sourceRevision: Long) {
        executeAndRefresh(
            JSONObject()
                .put("type", "markSourceExpired")
                .put("itemId", itemId)
                .put("sourceRevision", sourceRevision)
        )
        pruneRegistry()
    }

    fun failRequest(sessionGeneration: Long, requestId: Long, reasonCode: String) {
        executeAndRefresh(
            JSONObject()
                .put("type", "failRequest")
                .put("sessionGeneration", sessionGeneration)
                .put("requestId", requestId)
                .put("reasonCode", reasonCode)
        )
    }

    fun tick() {
        executeAndRefresh(JSONObject().put("type", "tick"))
    }

    fun resyncPendingRequests() {
        execute(JSONObject().put("type", "resyncPendingRequests"), refresh = false)
        drainEvents()
        refreshSnapshot()
    }

    fun validateActivationCallback(
        itemId: String,
        activationEpoch: Long,
        sourceRevision: Long,
    ): Boolean =
        runCatching {
            execute(
                JSONObject()
                    .put("type", "validateActivationCallback")
                    .put("itemId", itemId)
                    .put("activationEpoch", activationEpoch)
                    .put("sourceRevision", sourceRevision),
                refresh = false,
            )
            true
        }.getOrDefault(false)

    private fun submitItemsResponse(
        type: String,
        sessionGeneration: Long,
        requestId: Long,
        anchorItemId: String?,
        items: List<VesperPlaybackSequenceItem>,
        endReached: Boolean,
    ): Int {
        checkBatch(items)
        val staged = LinkedHashMap<String, SourceRegistryEntry>()
        val payload = JSONArray()
        try { items.forEach { payload.put(it.toJson(staged)) } }
        catch (error: Throwable) { staged.values.forEach { it.lease.close() }; throw error }
        val command =
            JSONObject()
                .put("type", type)
                .put("sessionGeneration", sessionGeneration)
                .put("requestId", requestId)
                .putNullable("anchorItemId", anchorItemId)
                .put("items", payload)
                .put("endReached", endReached)
        val result = try {
            synchronized(ownershipLock) { ensureRegistryCapacity(staged.size) }
            execute(command, refresh = false)
        }
        catch (error: Throwable) { staged.values.forEach { it.lease.close() }; throw error }
        synchronized(ownershipLock) {
            sourceRegistry.putAll(staged)
        }
        refreshAndPump()
        pruneRegistry()
        return result.optInt("acceptedCount")
    }

    private fun VesperPlaybackSequenceItem.toJson(
        stagedRegistry: MutableMap<String, SourceRegistryEntry>,
    ): JSONObject {
        val payload =
            JSONObject()
                .put("itemId", itemId)
                .put("providerNamespace", contentIdentity.providerNamespace)
                .put("contentIdentity", contentIdentity.value)
                .put("mediaKind", mediaKind.wireName)
                .putNullable("providerMetadataRef", providerMetadataRef)
                .put("preloadProfile", preloadProfile.toJson())
        if (source != null) {
            val sourceReference = nextSourceReference()
            val retained = synchronized(ownershipLock) { sourceRegistry.values.firstOrNull { it.itemId == itemId && it.handle === source } }
            val revision = retained?.sourceRevision
                ?: Math.addExact(snapshot.value.items.firstOrNull { it.itemId == itemId }?.sourceRevision ?: 0, 1)
            val entry = SourceRegistryEntry(itemId, revision, source, retained?.lease?.retain() ?: source.acquire())
            stagedRegistry[sourceReference] = entry
            payload.put(
                "resolvedSource",
                JSONObject()
                    .put("sourceReference", sourceReference)
                    .put("cacheIdentity", source.cacheIdentity(revision).toJson())
                    .put("warmupGoal", entry.source.sequenceWarmupGoal())
                    .putNullable("expiresAtEpochMs", entry.source.dashStartupScope?.sourceExpiresAtMs),
            )
        }
        return payload
    }

    private fun executeAndRefresh(command: JSONObject): JSONObject {
        val result = execute(command, refresh = false)
        refreshAndPump()
        return result
    }

    private fun execute(command: JSONObject, refresh: Boolean): JSONObject {
        checkActive()
        val response =
            JSONObject(
                VesperNativeJni.executeSequenceCommand(
                    sessionHandle,
                    command.toString(),
                    System.currentTimeMillis(),
                )
            )
        val result = response.requireResult()
        if (refresh) {
            refreshAndPump()
        }
        return result
    }

    private fun refreshAndPump() {
        refreshSnapshot()
        drainEvents()
        pumpPreloadIntents()
    }

    private fun pumpPreloadIntents() {
        if (isDisposed.get() || controller == null) return
        val token = attachmentEpoch.get()
        val envelope = runCatching { JSONObject(VesperNativeJni.sequencePreloadIntents(sessionHandle, System.currentTimeMillis())) }.getOrNull() ?: return
        val raw = runCatching { envelope.requireResult().optJSONArray("intents") }.getOrNull() ?: return
        val intents = (0 until raw.length()).mapNotNull { raw.optJSONObject(it)?.let(VesperSequenceWarmupIntent::fromJson) }
        val retained = intents.map { it.key }.toSet()
        preloadObservers.keys.filter { it !in retained }.forEach { preloadObservers.remove(it)?.cancel() }
        intents.forEach { intent ->
            if (preloadObservers.containsKey(intent.key)) return@forEach
            val entry = synchronized(ownershipLock) { sourceRegistry[intent.sourceReference] }
                ?.takeIf { it.itemId == intent.itemId && it.sourceRevision == intent.sourceRevision } ?: return@forEach
            val job = preloadScope.launch(start = CoroutineStart.LAZY) {
                try {
                    val task = entry.handle.preload(VesperPreloadOptions(timeoutMs = intent.warmupWindowMs.takeIf { it > 0 }?.coerceAtMost(60_000) ?: 5000))
                    val firstObservation = task.snapshots.first { it.state != VesperPreloadState.Queued }
                    if (firstObservation.state == VesperPreloadState.Running) {
                        reportSessionPreload(intent, "started", firstObservation, token)
                    }
                    val result = task.await()
                    when (result.state) {
                        VesperPreloadState.Completed -> warmupStats = warmupStats.copy(completedJobs = warmupStats.completedJobs + 1,
                            actualBytes = warmupStats.actualBytes + result.actualBytes,
                            cacheHits = warmupStats.cacheHits + if (result.cacheHit == true) 1 else 0,
                            cacheMisses = warmupStats.cacheMisses + if (result.cacheHit == false) 1 else 0)
                        VesperPreloadState.Unsupported -> warmupStats = warmupStats.copy(unsupportedJobs = warmupStats.unsupportedJobs + 1)
                        VesperPreloadState.Cancelled -> warmupStats = warmupStats.copy(cancelledJobs = warmupStats.cancelledJobs + 1)
                        else -> warmupStats = warmupStats.copy(failedJobs = warmupStats.failedJobs + 1)
                    }
                    reportSessionPreload(intent, result.state.name.replaceFirstChar { it.lowercase() }, result, token)
                } catch (error: CancellationException) { throw error }
                catch (_: Exception) {
                    reportSessionPreload(intent, "failed", null, token)
                }
            }
            if (preloadObservers.putIfAbsent(intent.key, job) == null) job.start() else job.cancel()
        }
    }

    private fun reportSessionPreload(intent: VesperSequenceWarmupIntent, status: String, result: VesperPreloadResult?, token: Long) {
        if (isDisposed.get() || attachmentEpoch.get() != token || controller == null) return
        runCatching {
            execute(JSONObject().put("type", "reportWarmup")
                .put("sessionGeneration", intent.sessionGeneration).put("taskId", intent.warmupTaskId)
                .put("itemId", intent.itemId).put("sourceRevision", intent.sourceRevision).put("warmupGoal", intent.goal)
                .put("status", status).put("expectedBytes", result?.actualBytes ?: 0).put("actualBytes", result?.actualBytes ?: 0)
                .putNullable("cacheHit", result?.cacheHit).put("cacheEntries", 0).put("cacheBytes", 0).put("evictedEntries", 0)
                .putNullable("reasonCode", result?.reasonCode ?: if (status == "failed") "source_unavailable" else null), refresh = false)
            refreshSnapshot()
            drainEvents()
        }
    }

    private fun refreshSnapshot() {
        val envelope = JSONObject(VesperNativeJni.sequenceSnapshot(sessionHandle))
        _snapshot.value = envelope.requireResult().toSnapshot()
    }

    private fun drainEvents() {
        val envelope = JSONObject(VesperNativeJni.drainSequenceEvents(sessionHandle, 512))
        val events = envelope.requireResult().getJSONArray("events")
        for (index in 0 until events.length()) {
            val event = events.getJSONObject(index)
            _events.tryEmit(
                VesperPlaybackSequenceEvent(
                    eventSequence = event.getLong("eventSequence"),
                    sessionGeneration = event.getLong("sessionGeneration"),
                    event = event.getJSONObject("event").toMap(),
                )
            )
        }
    }

    private fun pruneRegistry() {
        val retained = _snapshot.value.items.mapNotNull { it.sourceReference }.toSet()
        synchronized(ownershipLock) {
            sourceRegistry.filterKeys { it !in retained }.values.forEach { it.lease.close() }
            sourceRegistry.keys.retainAll(retained)
        }
    }

    private fun checkBatch(items: List<VesperPlaybackSequenceItem>) {
        checkActive()
        if (items.size > configuration.maxItems) {
            throw VesperPlaybackSequenceException("capacity_exceeded")
        }
        val ids = items.map { it.itemId }
        if (ids.size != ids.toSet().size) {
            throw VesperPlaybackSequenceException("duplicate_item_id")
        }
    }

    private fun ensureRegistryCapacity(additional: Int) {
        if (sourceRegistry.size + additional > configuration.maxSourceRegistryEntries) {
            throw VesperPlaybackSequenceException("source_registry_capacity_exceeded")
        }
    }

    private fun nextSourceReference(): String {
        val value = sourceReferenceCounter.getAndUpdate { current ->
            if (current == Long.MAX_VALUE) 1 else current + 1
        }
        return "sequence-source-$value"
    }

    private fun checkActive() {
        if (isDisposed.get()) {
            throw VesperPlaybackSequenceException("sequence_disposed")
        }
    }
}

private fun VesperPlaybackSequenceConfiguration.toConfigJson(): JSONObject =
    JSONObject()
        .put("sequenceId", sequenceId)
        .put("mode", mode.wireName)
        .put("historyLimit", historyLimit)
        .put("forwardWindow", forwardWindow)
        .put("refillThreshold", refillThreshold)
        .put("maxItems", maxItems)
        .put("maxPendingRequests", maxPendingRequests)
        .put("maxEvents", maxEvents)
        .put("requestTimeoutMs", requestTimeoutMs)
        .put("sourceExpiryLeadMs", sourceExpiryLeadMs)

private fun VesperPlaybackSequenceCacheIdentity.toJson(): JSONObject =
    JSONObject()
        .put("providerNamespace", providerNamespace)
        .put("contentIdentity", contentIdentity)
        .put("renditionIdentity", renditionIdentity)
        .put("resourceIdentity", resourceIdentity)
        .put("accessPartition", accessPartition)
        .put("sourceRevision", sourceRevision)

private fun VesperPlaybackSequencePreloadProfile.toJson(): JSONObject =
    JSONObject()
        .put("expectedMemoryBytes", expectedMemoryBytes)
        .put("expectedDiskBytes", expectedDiskBytes)
        .putNullable("ttlMs", ttlMs)
        .putNullable("warmupWindowMs", warmupWindowMs)

private fun JSONObject.requireResult(): JSONObject {
    if (!optBoolean("ok")) {
        val error = optJSONObject("error")
        throw VesperPlaybackSequenceException(
            code = error?.optString("code")?.takeIf(String::isNotBlank) ?: "unknown_error",
            message = error?.optString("message")?.takeIf(String::isNotBlank) ?: "sequence failed",
        )
    }
    return getJSONObject("result")
}

private fun JSONObject.toSnapshot(): VesperPlaybackSequenceSnapshot {
    val itemsJson = getJSONArray("items")
    val items = ArrayList<VesperPlaybackSequenceItemState>(itemsJson.length())
    for (index in 0 until itemsJson.length()) {
        val state = itemsJson.getJSONObject(index)
        val item = state.getJSONObject("item")
        val sourceState = item.getJSONObject("sourceState")
        items +=
            VesperPlaybackSequenceItemState(
                itemId = item.getString("itemId"),
                index = state.getInt("index"),
                isActive = state.getBoolean("isActive"),
                mediaKind = item.getString("mediaKind"),
                sourceState = sourceState.getString("state"),
                sourceRevision =
                    sourceState.optLong(
                        "sourceRevision",
                        sourceState.optLong("expectedSourceRevision", 0),
                    ),
                sourceReference = sourceState.nullableString("sourceReference"),
            )
    }
    return VesperPlaybackSequenceSnapshot(
        sequenceId = getString("sequenceId"),
        sessionGeneration = getLong("sessionGeneration"),
        activationEpoch = getLong("activationEpoch"),
        items = items,
        activeItemId = nullableString("activeItemId"),
        pendingRequests = getJSONArray("pendingRequests").toMapList(),
        requestFailures = getJSONArray("requestFailures").toMapList(),
        previousEndReached = getBoolean("previousEndReached"),
        nextEndReached = getBoolean("nextEndReached"),
        droppedEvents = getLong("droppedEvents"),
        warmupTasks = optJSONArray("warmupTasks")?.toMapList() ?: emptyList(),
        warmupStats = optJSONObject("warmupStats")?.toMap() ?: emptyMap(),
    )
}

private fun JSONObject.putNullable(key: String, value: Any?): JSONObject =
    put(key, value ?: JSONObject.NULL)

private fun JSONObject.nullableString(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)

private fun JSONArray.toMapList(): List<Map<String, Any?>> =
    (0 until length()).map { index -> getJSONObject(index).toMap() }

private fun JSONObject.toMap(): Map<String, Any?> =
    keys().asSequence().associateWith { key -> get(key).toKotlinValue() }

private fun Any?.toKotlinValue(): Any? =
    when (this) {
        JSONObject.NULL -> null
        is JSONObject -> toMap()
        is JSONArray -> (0 until length()).map { index -> get(index).toKotlinValue() }
        else -> this
    }

internal fun VesperPlayerSource.sequenceWarmupGoal(): String =
    if (protocol == VesperPlayerSourceProtocol.Dash) "dashSegmentBaseStartup" else "progressiveRange"

private fun VesperSourceHandle.cacheIdentity(revision: Long) = VesperPlaybackSequenceCacheIdentity(
    "vesper", sessionId, id, id, sessionId, revision,
)
