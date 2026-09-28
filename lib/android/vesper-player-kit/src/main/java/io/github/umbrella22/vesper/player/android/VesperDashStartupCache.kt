package io.github.umbrella22.vesper.player.android

import java.security.MessageDigest
import java.util.Locale
import java.util.UUID

internal data class DashStartupScope(
    val namespace: String = UUID.randomUUID().toString(),
    val owner: String = namespace,
    val sourceExpiresAtMs: Long? = null,
    val ttlMs: Long = 30_000,
)

internal data class DashStartupResource(
    val uri: String, val position: Long = 0, val length: Long? = null,
) {
    init {
        require(position >= 0 && (length == null || length > 0))
        if (length != null) Math.addExact(position, length)
    }
}

internal data class DashStartupBytes(
    val resource: DashStartupResource, val bytes: ByteArray, val finalUri: String = resource.uri,
)

/** Immutable startup bytes only; no sockets, player ownership, or disk I/O under the lock. */
internal class VesperDashStartupCache(private val nowMs: () -> Long = { System.nanoTime() / 1_000_000 },
    private val wallNowMs: () -> Long = System::currentTimeMillis,) {
    private data class Entry(val key: String, val owner: String, val value: DashStartupBytes, val expiresAt: Long)
    private val entries = LinkedHashMap<String, Entry>(64, 0.75f, true)
    private var sizeBytes = 0L
    private val lock = Any()
    private var generation = 0L
    fun currentGeneration(): Long = synchronized(lock) { generation }

    fun read(scope: DashStartupScope, resource: DashStartupResource, headers: Map<String, String>): DashStartupBytes? {
        val key = resourceKey(scope, resource.uri, headers)
        return synchronized(lock) {
            expire()
            if (scope.sourceExpiresAtMs?.let { it <= wallNowMs() } == true) return@synchronized null
            val matching = entries.values.filter { it.key == key }.sortedBy { it.value.resource.position }
            val whole = matching.firstOrNull { it.value.resource.length == null }
            val wantedLength = resource.length ?: whole?.let { it.value.bytes.size.toLong() - resource.position }
                ?: return@synchronized null
            if (wantedLength <= 0 || wantedLength > MAX_RESOURCE_BYTES) return@synchronized null
            val end = Math.addExact(resource.position, wantedLength)
            var cursor = resource.position
            val output = ByteArray(wantedLength.toInt())
            var finalUri = resource.uri
            for (entry in matching) {
                val value = entry.value
                val start = value.resource.position
                val entryEnd = start + value.bytes.size
                if (start > cursor) break
                if (entryEnd <= cursor) continue
                val count = (minOf(end, entryEnd) - cursor).toInt()
                value.bytes.copyInto(output, (cursor - resource.position).toInt(), (cursor - start).toInt(), (cursor - start).toInt() + count)
                cursor += count
                finalUri = value.finalUri
                if (cursor == end) return@synchronized DashStartupBytes(resource, output, finalUri)
            }
            null
        }
    }

    /** Commits only a complete, validated set after the caller's cancellation fence. */
    fun store(scope: DashStartupScope, values: List<DashStartupBytes>, headers: Map<String, String>, expectedGeneration: Long = currentGeneration(), maximumBytes: Long = MAX_WARMUP_BYTES): Boolean {
        require(values.size <= MAX_ENTRIES)
        val bytes = values.sumOf { it.bytes.size.toLong() }
        val budget = minOf(maximumBytes, MAX_WARMUP_BYTES)
        require(bytes <= budget && values.all { it.bytes.size.toLong() <= MAX_RESOURCE_BYTES })
        val prepared = values.map { value ->
            require(value.bytes.isNotEmpty() && (value.resource.length == null || value.resource.length == value.bytes.size.toLong()))
            val key = resourceKey(scope, value.resource.uri, headers)
            Triple(key + ":" + value.resource.position + ":" + value.resource.length, key, value.copy(bytes = value.bytes.copyOf()))
        }
        return synchronized(lock) {
            if (expectedGeneration != generation) return@synchronized false
            expire()
            val now = nowMs()
            val sourceRemaining = scope.sourceExpiresAtMs?.let { it - wallNowMs() } ?: 30_000
            val expires = now + minOf(sourceRemaining, scope.ttlMs.coerceIn(1, 30_000))
            if (expires <= now) return@synchronized false
            while (entries.values.filter { it.owner == scope.owner }.sumOf { it.value.bytes.size.toLong() } + bytes > budget) {
                val oldest = entries.entries.firstOrNull { it.value.owner == scope.owner }?.key ?: break
                remove(oldest)
            }
            for ((entryKey, key, value) in prepared) {
                sizeBytes -= entries.remove(entryKey)?.value?.bytes?.size ?: 0
                while (entries.isNotEmpty() && (entries.size >= MAX_ENTRIES || sizeBytes + value.bytes.size > MAX_CACHE_BYTES)) {
                    remove(entries.keys.first())
                }
                entries[entryKey] = Entry(key, scope.owner, value, expires)
                sizeBytes += value.bytes.size
            }
            true
        }
    }

    fun clear() = synchronized(lock) { entries.clear(); sizeBytes = 0L; generation++ }
    fun inventory(): Pair<Int, Long> = synchronized(lock) { expire(); entries.size to sizeBytes }
    private fun remove(key: String) { sizeBytes -= entries.remove(key)?.value?.bytes?.size ?: 0 }
    private fun expire() {
        val now = nowMs()
        entries.filterValues { it.expiresAt <= now }.keys.toList().forEach(::remove)
    }

    internal fun resourceKey(scope: DashStartupScope, uri: String, headers: Map<String, String>): String {
        val digest = MessageDigest.getInstance("SHA-256")
        fun field(value: String) {
            val bytes = value.toByteArray(Charsets.UTF_8)
            digest.update(bytes.size.toString().toByteArray(Charsets.US_ASCII))
            digest.update(0.toByte())
            digest.update(bytes)
        }
        field(scope.namespace)
        field(uri)
        headers.entries.sortedWith(compareBy({ it.key.lowercase(Locale.ROOT) }, { it.value })).forEach {
            field(it.key.lowercase(Locale.ROOT)); field(it.value)
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    companion object {
        const val MAX_ENTRIES = 64
        const val MAX_RESOURCE_BYTES = 8 * 1024 * 1024L
        const val MAX_WARMUP_BYTES = 16 * 1024 * 1024L
        const val MAX_CACHE_BYTES = 32 * 1024 * 1024L
        val shared = VesperDashStartupCache()
    }
}
