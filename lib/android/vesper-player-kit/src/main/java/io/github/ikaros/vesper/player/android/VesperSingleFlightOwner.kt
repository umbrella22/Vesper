package io.github.ikaros.vesper.player.android

import android.content.Context
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import java.io.File
import java.util.IdentityHashMap
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

internal class VesperSingleFlightOwner<K : Any, R : Any>(
    private val waitTimeoutMillis: Long = DEFAULT_SINGLE_FLIGHT_WAIT_MILLIS,
    private val maxEntries: Int = DEFAULT_SINGLE_FLIGHT_MAX_ENTRIES,
    private val closeResource: (R) -> Unit,
) : AutoCloseable {
    init {
        require(maxEntries > 0) { "Single-flight resource owner maxEntries must be positive" }
    }

    private val lock = Any()
    private val entries = mutableMapOf<K, Entry<R>>()
    private val closedResources = IdentityHashMap<R, Unit>()
    private val constructionExecutor =
        ThreadPoolExecutor(
            1,
            1,
            0L,
            TimeUnit.MILLISECONDS,
            ArrayBlockingQueue(maxEntries),
            { runnable ->
                Thread(runnable, "vesper-single-flight-owner").apply { isDaemon = true }
            },
            ThreadPoolExecutor.AbortPolicy(),
        )
    private var closed = false

    fun get(
        key: K,
        create: () -> R,
    ): R {
        var ownsConstruction = false
        val entry = synchronized(lock) {
            check(!closed) { "Single-flight resource owner is closed" }
            val acquired = entries[key] ?: Entry<R>().also { created ->
                if (entries.size >= maxEntries) {
                    throw RejectedExecutionException(
                        "Single-flight resource owner reached its entry limit of $maxEntries",
                    )
                }
                entries[key] = created
                ownsConstruction = true
            }
            acquired.waiters += 1
            acquired
        }

        if (ownsConstruction) {
            submitConstruction(key, entry, create)
        }

        try {
            val resource = try {
                entry.future.get(waitTimeoutMillis.coerceAtLeast(1L), TimeUnit.MILLISECONDS)
            } catch (error: ExecutionException) {
                throw error.cause ?: error
            }
            val stillOwned = synchronized(lock) { !closed && entries[key] === entry }
            if (!stillOwned) {
                closeOnce(resource)
                throw IllegalStateException("Single-flight resource owner closed during construction")
            }
            return resource
        } finally {
            releaseWaiter(key, entry)
        }
    }

    override fun close() {
        val activeEntries = synchronized(lock) {
            if (closed) {
                return
            }
            closed = true
            entries.values.toList().also { entries.clear() }
        }
        constructionExecutor.shutdownNow()
        activeEntries.forEach { entry ->
            if (!entry.started) {
                entry.future.completeExceptionally(
                    IllegalStateException("Single-flight resource owner closed during construction"),
                )
            }
            entry.future.whenComplete { resource, error ->
                if (error == null && resource != null) {
                    closeOnce(resource)
                }
            }
        }
    }

    private fun submitConstruction(
        key: K,
        entry: Entry<R>,
        create: () -> R,
    ) {
        val task =
            Runnable {
                val shouldCreate = synchronized(lock) {
                    if (!closed && entries[key] === entry && !entry.future.isDone) {
                        entry.started = true
                        true
                    } else {
                        false
                    }
                }
                if (shouldCreate) {
                    try {
                        val resource = create()
                        if (!entry.future.complete(resource)) {
                            closeOnce(resource)
                        }
                    } catch (error: Throwable) {
                        entry.future.completeExceptionally(error)
                        removeFailedEntry(key, entry)
                    }
                }
            }
        val shouldSubmit = synchronized(lock) {
            if (!closed && entries[key] === entry) {
                entry.task = task
                true
            } else {
                false
            }
        }
        if (!shouldSubmit) {
            entry.future.completeExceptionally(
                IllegalStateException("Single-flight resource owner closed during construction"),
            )
            return
        }
        try {
            constructionExecutor.execute(task)
        } catch (error: RejectedExecutionException) {
            entry.future.completeExceptionally(error)
            removeFailedEntry(key, entry)
        }
    }

    private fun removeFailedEntry(
        key: K,
        entry: Entry<R>,
    ) {
        synchronized(lock) {
            if (entries[key] === entry) {
                entries.remove(key)
            }
        }
    }

    private fun releaseWaiter(
        key: K,
        entry: Entry<R>,
    ) {
        var abandonedTask: Runnable? = null
        var cancelFuture = false
        synchronized(lock) {
            entry.waiters -= 1
            if (
                entry.waiters == 0 &&
                !entry.started &&
                !entry.future.isDone &&
                entries[key] === entry
            ) {
                entries.remove(key)
                abandonedTask = entry.task
                cancelFuture = true
            }
        }
        abandonedTask?.let(constructionExecutor::remove)
        if (cancelFuture) {
            entry.future.cancel(false)
        }
    }

    private fun closeOnce(resource: R) {
        val shouldClose = synchronized(lock) {
            closedResources.put(resource, Unit) == null
        }
        if (shouldClose) {
            runCatching { closeResource(resource) }
        }
    }

    private class Entry<R : Any> {
        val future = CompletableFuture<R>()
        var waiters = 0
        var started = false
        var task: Runnable? = null
    }
}

internal class VesperSimpleCacheResource private constructor(
    val cache: SimpleCache,
    private val databaseProvider: StandaloneDatabaseProvider,
) : AutoCloseable {
    override fun close() {
        runCatching { cache.release() }
        runCatching { databaseProvider.close() }
    }

    companion object {
        fun create(
            appContext: Context,
            cacheDir: File,
            maxDiskBytes: Long,
        ): VesperSimpleCacheResource {
            cacheDir.mkdirs()
            val databaseProvider = StandaloneDatabaseProvider(appContext)
            return try {
                VesperSimpleCacheResource(
                    cache = SimpleCache(
                        cacheDir,
                        LeastRecentlyUsedCacheEvictor(maxDiskBytes),
                        databaseProvider,
                    ),
                    databaseProvider = databaseProvider,
                )
            } catch (error: Throwable) {
                runCatching { databaseProvider.close() }
                throw error
            }
        }
    }
}

private const val DEFAULT_SINGLE_FLIGHT_WAIT_MILLIS = 10_000L

// Cache budgets should be stable within a process. This ceiling tolerates
// limited configuration churn without retaining unbounded cache/database pairs.
private const val DEFAULT_SINGLE_FLIGHT_MAX_ENTRIES = 8
