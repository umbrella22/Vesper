package io.github.umbrella22.vesper.player.android

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

data class VesperPlaybackSequenceCacheIdentity(
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
    val source: VesperPlayerSource? = null,
    val cacheIdentity: VesperPlaybackSequenceCacheIdentity? = null,
    val sourceRevision: Long = cacheIdentity?.sourceRevision ?: 0,
    val expiresAtEpochMs: Long? = null,
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
        require((source == null) == (cacheIdentity == null)) {
            "source and cacheIdentity must either both be present or both be absent"
        }
        if (source != null) {
            require(sourceRevision > 0 && cacheIdentity?.sourceRevision == sourceRevision) {
                "resolved source revision must be positive and match cacheIdentity"
            }
        }
    }
}

data class VesperPlaybackSequenceResolvedSource(
    val sessionGeneration: Long,
    val requestId: Long,
    val resolutionAttemptId: Long,
    val itemId: String,
    val expectedSourceRevision: Long,
    val sourceRevision: Long,
    val source: VesperPlayerSource,
    val cacheIdentity: VesperPlaybackSequenceCacheIdentity,
    val expiresAtEpochMs: Long? = null,
)

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
)

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
        val source: VesperPlayerSource,
    )

    private data class AppliedActivation(
        val itemId: String,
        val sourceRevision: Long,
        val activationEpoch: Long,
    )

    private val isDisposed = AtomicBoolean(false)
    private val attachmentEpoch = AtomicLong(0)
    private val sourceReferenceCounter = AtomicLong(1)
    private val ownershipLock = Any()
    private val sourceRegistry = LinkedHashMap<String, SourceRegistryEntry>()
    private var controller: VesperPlayerController? = null
    private var appliedActivation: AppliedActivation? = null
    private var warmupExecutor: VesperPlaybackSequenceWarmupExecutor? = null
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
        warmupExecutor?.snapshot?.value ?: VesperPlaybackSequenceWarmupSnapshot()

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
        val attachmentToken = attachmentEpoch.updateAndGet { current ->
            if (current == Long.MAX_VALUE) 1L else current + 1L
        }
        val executor = target.sequencePreloadContext()?.let { context ->
            val maxDiskBytes =
                target.sequenceResiliencePolicy().cache.maxDiskBytes
                    ?: 256L * 1024L * 1024L
            VesperPlaybackSequenceWarmupExecutor(
                context = context,
                maxDiskBytes = maxDiskBytes,
                onSourceExpired = { itemId, sourceRevision ->
                    mainHandler.post {
                        synchronized(ownershipLock) {
                            if (isDisposed.get() || controller !== target ||
                                attachmentEpoch.get() != attachmentToken
                            ) return@synchronized
                        }
                        runCatching { markSourceExpired(itemId, sourceRevision) }
                    }
                },
                onReport = { report ->
                    mainHandler.post {
                        synchronized(ownershipLock) {
                            if (isDisposed.get() || controller !== target ||
                                attachmentEpoch.get() != attachmentToken
                            ) return@synchronized
                        }
                        runCatching {
                            execute(
                                JSONObject()
                                    .put("type", "reportWarmup")
                                    .put("sessionGeneration", report.sessionGeneration)
                                    .put("taskId", report.taskId)
                                    .put("itemId", report.itemId)
                                    .put("sourceRevision", report.sourceRevision)
                                    .put("warmupGoal", "progressiveRange")
                                    .put("status", report.status)
                                    .put("expectedBytes", report.expectedBytes)
                                    .put("actualBytes", report.actualBytes)
                                    .putNullable("cacheHit", report.cacheHit)
                                    .put("cacheEntries", report.cacheEntries)
                                    .put("cacheBytes", report.cacheBytes)
                                    .put("evictedEntries", report.evictedEntries)
                                    .putNullable("reasonCode", report.reasonCode),
                                refresh = true,
                            )
                        }
                    }
                },
            )
        }
        var attached = false
        synchronized(ownershipLock) {
            if (controller != null) {
                executor?.close()
                throw VesperPlaybackSequenceException("already_attached")
            }
            try {
                target.attachPlaybackSequence(this)
                controller = target
                warmupExecutor = executor
                attached = true
            } catch (error: Throwable) {
                executor?.close()
                throw error
            }
        }
        try {
            check(attached) { "sequence attachment did not complete" }
            applyActiveSource(_snapshot.value)
            pumpPreloadIntents()
        } catch (error: Throwable) {
            detach()
            throw error
        }
    }

    fun detach() {
        attachmentEpoch.updateAndGet { current ->
            if (current == Long.MAX_VALUE) 1L else current + 1L
        }
        val target = synchronized(ownershipLock) {
            controller.also {
                controller = null
                appliedActivation = null
            }
        }
        warmupExecutor?.close()
        warmupExecutor = null
        target?.detachPlaybackSequence(this)
    }

    override fun onControllerDisposed(controller: VesperPlayerController) {
        attachmentEpoch.updateAndGet { current ->
            if (current == Long.MAX_VALUE) 1L else current + 1L
        }
        synchronized(ownershipLock) {
            if (this.controller === controller) {
                this.controller = null
                appliedActivation = null
                sourceRegistry.clear()
            }
        }
        warmupExecutor?.close()
        warmupExecutor = null
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
            sourceRegistry.clear()
            appliedActivation = null
        }
        VesperNativeJni.disposeSequenceSession(sessionHandle)
    }

    fun replace(
        items: List<VesperPlaybackSequenceItem>,
        activeItemId: String? = items.firstOrNull()?.itemId,
    ) {
        checkBatch(items)
        val stagedRegistry = LinkedHashMap<String, SourceRegistryEntry>()
        val itemPayloads = JSONArray()
        items.forEach { item ->
            itemPayloads.put(item.toJson(stagedRegistry))
        }
        val command = JSONObject().put("type", "replace").put("items", itemPayloads)
        command.putNullable("activeItemId", activeItemId)
        execute(command, refresh = false)
        synchronized(ownershipLock) {
            sourceRegistry.clear()
            sourceRegistry.putAll(stagedRegistry)
            appliedActivation = null
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

    fun remove(itemId: String): Boolean =
        executeAndRefresh(JSONObject().put("type", "remove").put("itemId", itemId))
            .optBoolean("removed")

    fun setActive(itemId: String) {
        executeAndRefresh(JSONObject().put("type", "setActive").put("itemId", itemId))
    }

    fun next() {
        executeAndRefresh(JSONObject().put("type", "next"))
    }

    fun previous() {
        executeAndRefresh(JSONObject().put("type", "previous"))
    }

    fun submitResolvedSource(resolved: VesperPlaybackSequenceResolvedSource) {
        checkActive()
        require(resolved.sourceRevision > resolved.expectedSourceRevision) {
            "sourceRevision must advance"
        }
        require(resolved.cacheIdentity.sourceRevision == resolved.sourceRevision) {
            "cache identity revision must match source revision"
        }
        val sourceReference = nextSourceReference()
        val entry = SourceRegistryEntry(resolved.itemId, resolved.sourceRevision, resolved.source)
        synchronized(ownershipLock) {
            ensureRegistryCapacity(1)
            sourceRegistry[sourceReference] = entry
        }
        val source =
            JSONObject()
                .put("sessionGeneration", resolved.sessionGeneration)
                .put("requestId", resolved.requestId)
                .put("resolutionAttemptId", resolved.resolutionAttemptId)
                .put("itemId", resolved.itemId)
                .put("expectedSourceRevision", resolved.expectedSourceRevision)
                .put("sourceRevision", resolved.sourceRevision)
                .put("sourceReference", sourceReference)
                .put("cacheIdentity", resolved.cacheIdentity.toJson())
                .putNullable("expiresAtEpochMs", resolved.expiresAtEpochMs)
        try {
            executeAndRefresh(
                JSONObject().put("type", "submitResolvedSource").put("source", source)
            )
        } catch (error: Throwable) {
            synchronized(ownershipLock) { sourceRegistry.remove(sourceReference) }
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
        items.forEach { payload.put(it.toJson(staged)) }
        val command =
            JSONObject()
                .put("type", type)
                .put("sessionGeneration", sessionGeneration)
                .put("requestId", requestId)
                .putNullable("anchorItemId", anchorItemId)
                .put("items", payload)
                .put("endReached", endReached)
        synchronized(ownershipLock) {
            ensureRegistryCapacity(staged.size)
        }
        val result = execute(command, refresh = false)
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
        if (source != null && cacheIdentity != null) {
            val sourceReference = nextSourceReference()
            stagedRegistry[sourceReference] = SourceRegistryEntry(itemId, sourceRevision, source)
            payload.put(
                "resolvedSource",
                JSONObject()
                    .put("sourceReference", sourceReference)
                    .put("cacheIdentity", cacheIdentity.toJson())
                    .putNullable("expiresAtEpochMs", expiresAtEpochMs),
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
        applyActiveSource(_snapshot.value)
        pumpPreloadIntents()
    }

    private fun pumpPreloadIntents() {
        val executor = warmupExecutor ?: return
        if (isDisposed.get()) return
        val envelope =
            runCatching {
                JSONObject(
                    VesperNativeJni.sequencePreloadIntents(
                        sessionHandle,
                        System.currentTimeMillis(),
                    ),
                )
            }.getOrNull() ?: return
        val result = runCatching { envelope.requireResult() }.getOrNull() ?: return
        val intentsJson = result.optJSONArray("intents") ?: JSONArray()
        val intents = buildList {
            for (index in 0 until intentsJson.length()) {
                val rawIntent = intentsJson.optJSONObject(index)
                val intent = rawIntent?.let { VesperSequenceWarmupIntent.fromJson(it) }
                if (intent == null) {
                    executor.recordUnsupportedWireIntent()
                } else {
                    add(intent)
                }
            }
        }
        executor.reconcile(intents) { sourceReference, itemId, sourceRevision ->
            synchronized(ownershipLock) {
                sourceRegistry[sourceReference]
                    ?.takeIf { entry ->
                        entry.itemId == itemId && entry.sourceRevision == sourceRevision
                    }
                    ?.source
            }
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

    private fun applyActiveSource(snapshot: VesperPlaybackSequenceSnapshot) {
        val active = snapshot.items.firstOrNull { it.isActive } ?: return
        val sourceReference = active.sourceReference ?: return
        val target = synchronized(ownershipLock) { controller } ?: return
        val entry = synchronized(ownershipLock) { sourceRegistry[sourceReference] } ?: return
        if (entry.itemId != active.itemId || entry.sourceRevision != active.sourceRevision) {
            throw VesperPlaybackSequenceException("stale_source_registry_entry")
        }
        val activation = AppliedActivation(active.itemId, active.sourceRevision, snapshot.activationEpoch)
        if (activation == appliedActivation) {
            return
        }
        target.activateSequenceSource(this, entry.source)
        appliedActivation = activation
    }

    private fun pruneRegistry() {
        val retained = _snapshot.value.items.mapNotNull { it.sourceReference }.toSet()
        synchronized(ownershipLock) {
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
