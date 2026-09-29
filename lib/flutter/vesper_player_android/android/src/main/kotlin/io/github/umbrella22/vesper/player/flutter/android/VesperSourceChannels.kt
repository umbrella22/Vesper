package io.github.umbrella22.vesper.player.flutter.android

import android.content.Context
import io.github.umbrella22.vesper.player.android.*

/** Main-dispatcher-owned registries. Wire identities never encode credentials. */
internal class VesperSourceChannels(private val context: Context, private val player: (String) -> VesperPlayerController) {
    private data class Session(
        val native: VesperSourceSession,
        val handles: MutableMap<String, VesperSourceHandle> = linkedMapOf(),
        // Retain the latest task per registered source, including its terminal result.
        val tasks: MutableMap<String, Task> = linkedMapOf(),
    )
    private data class Task(val sessionId: String, val native: VesperPreloadTask)
    private val sessions = linkedMapOf<String, Session>()
    private var waiters = 0

    fun source(reference: Map<String, Any?>): VesperSourceHandle =
        sessions[reference["sessionId"]]?.handles?.get(reference["sourceId"])
            ?: throw IllegalStateException("unknown_source")

    suspend fun execute(method: String, args: Map<String, Any?>): Any? = when (method) {
        "createSourceSession" -> {
            check(sessions.size < 32) { "source_session_capacity" }
            val c = args.nested("configuration")
            val native = VesperSourceSession(context, VesperSourceSessionConfiguration(
                maxSources = c.int("maxSources", 128), maxConcurrentPreloads = c.int("maxConcurrentPreloads", 2),
                maxPendingPreloads = c.int("maxPendingPreloads", 4), maxMemoryBytes = c.long("maxMemoryBytes", 8 * 1024 * 1024)))
            sessions[native.id] = Session(native)
            mapOf("sessionId" to native.id)
        }
        "registerSource" -> {
            val session = sessions[args["sessionId"]] ?: error("unknown_source_session")
            val expiry = args["expiresAtEpochMs"]?.let { args.sourceLong("expiresAtEpochMs", 0) }
            val handle = session.native.register(args.nested("source").toVesperPlayerSource(), expiry)
            session.handles[handle.id] = handle
            mapOf("sessionId" to handle.sessionId, "sourceId" to handle.id, "expiresAtEpochMs" to expiry)
        }
        "releaseSource" -> {
            sessions[args["sessionId"]]?.let { session ->
                session.handles.remove(args["sourceId"])?.close()
                session.tasks.remove(args["sourceId"])
            }
            null
        }
        "invalidateSourceSession", "disposeSourceSession" -> {
            val session = sessions.remove(args["sessionId"])
            if (method == "invalidateSourceSession") session?.native?.invalidate() else session?.native?.close()
            null
        }
        "preloadSource" -> {
            val handle = source(args)
            val o = args.nested("options")
            val native = handle.preload(VesperPreloadOptions(o.long("maximumBytes", 8 * 1024 * 1024), o.long("timeoutMs", 5000)))
            val task = Task(handle.sessionId, native)
            sessions.getValue(handle.sessionId).tasks[handle.id] = task
            task.wire()
        }
        "sourcePreloadSnapshot" -> task(args).wire()
        "awaitSourcePreload" -> {
            val task = task(args)
            check(waiters < 128) { "preload_waiter_capacity" }
            waiters++
            try { task.native.await(); task.wire() } finally { waiters-- }
        }
        "cancelSourcePreload" -> { findTask(args)?.native?.cancel(); null }
        "activateSource" -> {
            val handle = source(args)
            check(waiters < 128) { "activation_waiter_capacity" }
            waiters++
            try { player(args["playerId"] as? String ?: error("missing_player")).activate(handle, args.nested("options").activationOptions()).toWireMap() }
            finally { waiters-- }
        }
        else -> error("unsupported_source_method")
    }
    suspend fun <T> withWaiter(operation: suspend () -> T): T {
        check(waiters < 128) { "activation_waiter_capacity" }
        waiters++
        try { return operation() } finally { waiters-- }
    }
    fun close() { sessions.values.forEach { it.native.close() }; sessions.clear() }
    private fun findTask(args: Map<String, Any?>): Task? =
        sessions[args["sessionId"]]?.tasks?.values?.firstOrNull { it.native.id == args["taskId"] }

    private fun task(args: Map<String, Any?>): Task = findTask(args) ?: error("unknown_preload_task")
    private fun Task.wire(): Map<String, Any?> {
        val value = native.snapshot
        return mapOf("sessionId" to sessionId, "sourceId" to value.handleId, "taskId" to value.taskId,
            "status" to value.state.name.replaceFirstChar { it.lowercase() },
            "goal" to value.goal.name.replaceFirstChar { it.lowercase() },
            "reuse" to value.reuse.name.replaceFirstChar { it.lowercase() },
            "actualBytes" to value.actualBytes, "cacheHit" to value.cacheHit, "reasonCode" to value.reasonCode)
    }
    companion object {
        val methods = setOf("createSourceSession", "registerSource", "releaseSource", "invalidateSourceSession", "disposeSourceSession",
            "preloadSource", "sourcePreloadSnapshot", "awaitSourcePreload", "cancelSourcePreload", "activateSource")
    }
}
internal fun Map<String, Any?>.nested(key: String): Map<String, Any?> {
    if (!containsKey(key)) return emptyMap()
    val value = this[key]
    require(value is Map<*, *> && value.keys.all { it is String }) { "invalid_$key" }
    return value.stringMap()
}

internal fun Map<String, Any?>.sourceLong(key: String, default: Long): Long {
    if (!containsKey(key)) return default
    return when (val value = this[key]) {
        is Int -> value.toLong()
        is Long -> value
        else -> throw IllegalArgumentException("invalid_$key")
    }
}

private fun Map<String, Any?>.long(key: String, default: Long) = sourceLong(key, default)

internal fun Map<String, Any?>.int(key: String, default: Int): Int {
    val value = sourceLong(key, default.toLong())
    require(value in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()) { "invalid_$key" }
    return value.toInt()
}

internal fun Map<String, Any?>.activationOptions(): VesperSourceActivationOptions {
    val play = if (containsKey("playWhenReady")) {
        this["playWhenReady"] as? Boolean ?: throw IllegalArgumentException("invalid_playWhenReady")
    } else true
    val rate = if (containsKey("playbackRate")) {
        (this["playbackRate"] as? Number)?.toFloat() ?: throw IllegalArgumentException("invalid_playbackRate")
    } else 1f
    return VesperSourceActivationOptions(
        playWhenReady = play,
        startPositionMs = sourceLong("startPositionMs", 0),
        playbackRate = rate,
        timeoutMs = sourceLong("timeoutMs", 30_000),
    )
}
