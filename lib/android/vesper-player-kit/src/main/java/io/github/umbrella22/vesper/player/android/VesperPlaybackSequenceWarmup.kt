package io.github.umbrella22.vesper.player.android

import android.content.Context
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import org.json.JSONObject

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
                warmupGoal !in setOf("progressiveRange", "dashSegmentBaseStartup")
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

internal class VesperMedia3SequenceWarmupTransport(
    private val appContext: Context,
) : VesperSequenceWarmupTransport {
    override suspend fun open(request: VesperSequenceWarmupReadRequest): VesperSequenceWarmupReadStream {
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
            override val cacheHit: Boolean = false

            override suspend fun read(buffer: ByteArray, offset: Int, length: Int): Int =
                dataSource.read(buffer, offset, length)

            override fun close() {
                dataSource.close()
            }
        }
    }

    private fun buildDataSource(request: VesperSequenceWarmupReadRequest) =
        DefaultDataSource.Factory(
            appContext,
            DefaultHttpDataSource.Factory()
                .setConnectTimeoutMs(request.timeoutMillis.toInt())
                .setReadTimeoutMs(request.timeoutMillis.toInt())
                .setDefaultRequestProperties(request.headers),
        ).createDataSource()
}

internal data class WarmupKey(
    val itemId: String,
    val sourceRevision: Long,
    val warmupTaskId: Long,
    val cacheKey: String,
)
