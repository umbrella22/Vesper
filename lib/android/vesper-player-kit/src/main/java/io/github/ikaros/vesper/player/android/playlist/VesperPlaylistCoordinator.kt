package io.github.ikaros.vesper.player.android

import android.content.Context
import android.net.Uri
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DefaultDataSource
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

enum class VesperPlaylistViewportHintKind {
    Visible,
    NearVisible,
    PrefetchOnly,
    Hidden,
}

enum class VesperPlaylistRepeatMode {
    Off,
    One,
    All,
}

enum class VesperPlaylistFailureStrategy {
    Pause,
    SkipToNext,
}

data class VesperPlaylistNeighborWindow(
    val previous: Int = 1,
    val next: Int = 1,
)

data class VesperPlaylistPreloadWindow(
    val nearVisible: Int = 2,
    val prefetchOnly: Int = 2,
)

data class VesperPlaylistSwitchPolicy(
    val autoAdvance: Boolean = true,
    val repeatMode: VesperPlaylistRepeatMode = VesperPlaylistRepeatMode.Off,
    val failureStrategy: VesperPlaylistFailureStrategy = VesperPlaylistFailureStrategy.SkipToNext,
)

data class VesperPlaylistConfiguration(
    val playlistId: String = "android-host-playlist",
    val neighborWindow: VesperPlaylistNeighborWindow = VesperPlaylistNeighborWindow(),
    val preloadWindow: VesperPlaylistPreloadWindow = VesperPlaylistPreloadWindow(),
    val switchPolicy: VesperPlaylistSwitchPolicy = VesperPlaylistSwitchPolicy(),
)

data class VesperPlaylistItemPreloadProfile(
    val expectedMemoryBytes: Long = 0L,
    val expectedDiskBytes: Long = 0L,
    val ttlMs: Long? = null,
    val warmupWindowMs: Long? = null,
)

data class VesperPlaylistQueueItem(
    val itemId: String,
    val source: VesperPlayerSource,
    val preloadProfile: VesperPlaylistItemPreloadProfile = VesperPlaylistItemPreloadProfile(),
)

data class VesperPlaylistViewportHint(
    val itemId: String,
    val kind: VesperPlaylistViewportHintKind,
    val order: Int = 0,
)

data class VesperPlaylistActiveItem(
    val itemId: String,
    val index: Int,
)

data class VesperPlaylistQueueItemState(
    val item: VesperPlaylistQueueItem,
    val index: Int,
    val viewportHint: VesperPlaylistViewportHintKind,
    val isActive: Boolean,
)

data class VesperPlaylistSnapshot(
    val playlistId: String,
    val queue: List<VesperPlaylistQueueItemState>,
    val activeItem: VesperPlaylistActiveItem?,
    val neighborWindow: VesperPlaylistNeighborWindow,
    val preloadWindow: VesperPlaylistPreloadWindow,
    val switchPolicy: VesperPlaylistSwitchPolicy,
)

class VesperPlaylistCoordinator(
    context: Context,
    private val configuration: VesperPlaylistConfiguration = VesperPlaylistConfiguration(),
    preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
) {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val warmupJobsLock = Any()
    private val warmupJobs = mutableMapOf<Long, Job>()

    private var currentQueue: List<VesperPlaylistQueueItem> = emptyList()
    private var currentViewportHints: List<VesperPlaylistViewportHint> = emptyList()
    private var resiliencePolicy: VesperPlaybackResiliencePolicy = resiliencePolicy

    private val sessionHandle =
        VesperNativeJni.createPlaylistSession(
            configuration.toNativePayload(),
            VesperNativeJni.resolvePreloadBudget(preloadBudgetPolicy.toNativePayload()),
        )

    private val _snapshot = MutableStateFlow(
        VesperPlaylistSnapshot(
            playlistId = configuration.playlistId,
            queue = emptyList(),
            activeItem = null,
            neighborWindow = configuration.neighborWindow,
            preloadWindow = configuration.preloadWindow,
            switchPolicy = configuration.switchPolicy,
        ),
    )

    val snapshot: StateFlow<VesperPlaylistSnapshot> = _snapshot.asStateFlow()

    init {
        check(sessionHandle != 0L) { "native playlist session handle must not be zero" }
    }

    fun dispose() {
        cancelAllWarmups()
        scope.cancel()
        VesperNativeJni.disposePlaylistSession(sessionHandle)
    }

    fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) {
        resiliencePolicy = policy
    }

    fun replaceQueue(queue: List<VesperPlaylistQueueItem>) {
        currentQueue = queue
        currentViewportHints = currentViewportHints.filter { hint ->
            queue.any { item -> item.itemId == hint.itemId }
        }
        VesperNativeJni.replacePlaylistQueue(
            sessionHandle,
            queue.map(VesperPlaylistQueueItem::toNativePayload).toTypedArray(),
            System.currentTimeMillis(),
        )
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    fun updateViewportHints(hints: List<VesperPlaylistViewportHint>) {
        currentViewportHints = hints
            .filter { hint -> hint.kind != VesperPlaylistViewportHintKind.Hidden }
            .filter { hint -> currentQueue.any { item -> item.itemId == hint.itemId } }
        VesperNativeJni.updatePlaylistViewportHints(
            sessionHandle,
            currentViewportHints.map(VesperPlaylistViewportHint::toNativePayload).toTypedArray(),
            System.currentTimeMillis(),
        )
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    fun clearViewportHints() {
        currentViewportHints = emptyList()
        VesperNativeJni.clearPlaylistViewportHints(sessionHandle, System.currentTimeMillis())
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    fun advanceToNext() {
        VesperNativeJni.advancePlaylistToNext(sessionHandle, System.currentTimeMillis())
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    fun advanceToPrevious() {
        VesperNativeJni.advancePlaylistToPrevious(sessionHandle, System.currentTimeMillis())
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    fun handlePlaybackCompleted() {
        VesperNativeJni.handlePlaylistPlaybackCompleted(sessionHandle, System.currentTimeMillis())
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    fun handlePlaybackFailed() {
        VesperNativeJni.handlePlaylistPlaybackFailed(sessionHandle, System.currentTimeMillis())
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    private fun refreshSnapshot() {
        val activeItem = VesperNativeJni.currentPlaylistActiveItem(sessionHandle)?.toPublic()
        val activeItemId = activeItem?.itemId
        val hintByItemId = currentViewportHints.associateBy({ it.itemId }, { it.kind })
        _snapshot.value =
            VesperPlaylistSnapshot(
                playlistId = configuration.playlistId,
                queue =
                    currentQueue.mapIndexed { index, item ->
                        VesperPlaylistQueueItemState(
                            item = item,
                            index = index,
                            viewportHint = hintByItemId[item.itemId] ?: VesperPlaylistViewportHintKind.Hidden,
                            isActive = activeItemId == item.itemId,
                        )
                    },
                activeItem = activeItem,
                neighborWindow = configuration.neighborWindow,
                preloadWindow = configuration.preloadWindow,
                switchPolicy = configuration.switchPolicy,
            )
    }

    private fun drainAndApplyPreloadCommands() {
        VesperNativeJni.drainPlaylistPreloadCommands(sessionHandle).forEach { command ->
            when (command) {
                is NativePreloadCommand.Start -> startWarmup(command.task)
                is NativePreloadCommand.Cancel -> cancelWarmup(command.taskId)
            }
        }
    }

    private fun startWarmup(task: NativePreloadTask) {
        cancelWarmup(task.taskId)
        val source = currentSourceForTask(task.sourceUri)
        val resolvedResiliencePolicy = resolvePlaylistResiliencePolicy(source, resiliencePolicy)
        val dataSourceFactory =
            buildPlaylistDataSourceFactory(appContext, resolvedResiliencePolicy.cache)
        val job =
            scope.launch {
                val dataSource = dataSourceFactory.createDataSource()
                val readLength =
                    task.expectedMemoryBytes
                        .coerceAtLeast(1L)
                        .coerceAtMost(DEFAULT_PLAYLIST_WARMUP_READ_BYTES.toLong())
                val dataSpec =
                    DataSpec.Builder()
                        .setUri(task.sourceUri)
                        .setLength(readLength)
                        .build()

                runCatching {
                    dataSource.open(dataSpec)
                    val buffer = ByteArray(DEFAULT_PLAYLIST_WARMUP_READ_BYTES)
                    dataSource.read(buffer, 0, buffer.size)
                }.onSuccess {
                    VesperNativeJni.completePlaylistPreloadTask(sessionHandle, task.taskId)
                }.onFailure { error ->
                    VesperNativeJni.failPlaylistPreloadTask(
                        sessionHandle,
                        task.taskId,
                        PLAYLIST_BACKEND_FAILURE_ORDINAL,
                        PLAYLIST_PLATFORM_CATEGORY_ORDINAL,
                        false,
                        error.message ?: "android playlist preload warmup failed",
                    )
                }

                runCatching { dataSource.close() }
                synchronized(warmupJobsLock) {
                    warmupJobs.remove(task.taskId)
                }
            }

        synchronized(warmupJobsLock) {
            warmupJobs[task.taskId] = job
        }
    }

    private fun cancelWarmup(taskId: Long) {
        synchronized(warmupJobsLock) {
            warmupJobs.remove(taskId)
        }?.cancel()
    }

    private fun cancelAllWarmups() {
        val jobs =
            synchronized(warmupJobsLock) {
                warmupJobs.values.toList().also { warmupJobs.clear() }
            }
        jobs.forEach(Job::cancel)
    }

    private fun currentSourceForTask(uri: String): VesperPlayerSource =
        currentQueue.firstOrNull { it.source.uri == uri }?.source
            ?: if (
                uri.startsWith("content://", ignoreCase = true) ||
                    uri.startsWith("file://", ignoreCase = true)
            ) {
                VesperPlayerSource.local(uri = uri, label = Uri.parse(uri).lastPathSegment ?: uri)
            } else {
                VesperPlayerSource.remote(uri = uri, label = uri)
            }
}

private fun VesperPlaylistConfiguration.toNativePayload(): NativePlaylistConfig =
    NativePlaylistConfig(
        playlistId = playlistId,
        neighborPrevious = neighborWindow.previous,
        neighborNext = neighborWindow.next,
        preloadNearVisible = preloadWindow.nearVisible,
        preloadPrefetchOnly = preloadWindow.prefetchOnly,
        autoAdvance = switchPolicy.autoAdvance,
        repeatModeOrdinal = switchPolicy.repeatMode.ordinal,
        failureStrategyOrdinal = switchPolicy.failureStrategy.ordinal,
    )

private fun VesperPlaylistQueueItem.toNativePayload(): NativePlaylistQueueItem =
    NativePlaylistQueueItem(
        itemId = itemId,
        sourceUri = source.uri,
        expectedMemoryBytes = preloadProfile.expectedMemoryBytes,
        expectedDiskBytes = preloadProfile.expectedDiskBytes,
        hasTtlMs = preloadProfile.ttlMs != null,
        ttlMs = preloadProfile.ttlMs ?: 0L,
        hasWarmupWindowMs = preloadProfile.warmupWindowMs != null,
        warmupWindowMs = preloadProfile.warmupWindowMs ?: 0L,
    )

private fun VesperPlaylistViewportHint.toNativePayload(): NativePlaylistViewportHint =
    NativePlaylistViewportHint(
        itemId = itemId,
        kindOrdinal = kind.ordinal,
        order = order,
    )

private fun NativePlaylistActiveItem.toPublic(): VesperPlaylistActiveItem =
    VesperPlaylistActiveItem(
        itemId = itemId,
        index = index,
    )

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

private fun resolvePlaylistResiliencePolicy(
    source: VesperPlayerSource,
    resiliencePolicy: VesperPlaybackResiliencePolicy,
): NativeResolvedResiliencePolicy =
    VesperNativeJni.resolveResiliencePolicy(
        sourceKindOrdinal = source.kind.ordinal,
        sourceProtocolOrdinal = source.protocol.ordinal,
        bufferingPolicy = resiliencePolicy.buffering.toNativePayload(),
        retryPolicy = resiliencePolicy.retry.toNativePayload(),
        cachePolicy = resiliencePolicy.cache.toNativePayload(),
    )

private fun buildPlaylistDataSourceFactory(
    appContext: Context,
    cachePolicy: NativeCachePolicy,
): androidx.media3.datasource.DataSource.Factory {
    val upstreamFactory = DefaultDataSource.Factory(appContext)
    val resolvedCachePolicy = resolvePlaylistCachePolicy(cachePolicy)
    if (!resolvedCachePolicy.enabled) {
        return upstreamFactory
    }

    val cache =
        VesperPlaylistMediaCacheStore.cache(
            appContext = appContext,
            maxDiskBytes = resolvedCachePolicy.maxDiskBytes,
        )

    return CacheDataSource.Factory()
        .setCache(cache)
        .setUpstreamDataSourceFactory(upstreamFactory)
        .setFlags(CacheDataSource.FLAG_IGNORE_CACHE_ON_ERROR)
}

private fun resolvePlaylistCachePolicy(
    cachePolicy: NativeCachePolicy,
): PlaylistResolvedCachePolicy {
    val maxDiskBytes = cachePolicy.maxDiskBytes.takeIf { cachePolicy.hasMaxDiskBytes } ?: 0L
    return PlaylistResolvedCachePolicy(enabled = maxDiskBytes > 0L, maxDiskBytes = maxDiskBytes)
}

private data class PlaylistResolvedCachePolicy(
    val enabled: Boolean,
    val maxDiskBytes: Long,
)

private object VesperPlaylistMediaCacheStore {
    private val caches = mutableMapOf<Long, SimpleCache>()
    private val databaseProviders = mutableMapOf<Long, StandaloneDatabaseProvider>()

    @Synchronized
    fun cache(
        appContext: Context,
        maxDiskBytes: Long,
    ): SimpleCache {
        return caches.getOrPut(maxDiskBytes) {
            val cacheDir =
                File(appContext.cacheDir, "vesper-playlist-cache/$maxDiskBytes").apply { mkdirs() }
            val databaseProvider =
                databaseProviders.getOrPut(maxDiskBytes) { StandaloneDatabaseProvider(appContext) }
            SimpleCache(
                cacheDir,
                LeastRecentlyUsedCacheEvictor(maxDiskBytes),
                databaseProvider,
            )
        }
    }
}

private const val DEFAULT_PLAYLIST_WARMUP_READ_BYTES = 32 * 1024
private const val PLAYLIST_BACKEND_FAILURE_ORDINAL = 3
private const val PLAYLIST_PLATFORM_CATEGORY_ORDINAL = 7
