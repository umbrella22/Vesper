package io.github.umbrella22.vesper.player.android.external.internal.relay

import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

internal class VesperRelayEntryStore(
    private val tokenTtlMillis: Long?,
    private val nowMillisProvider: () -> Long,
    private val onInvalidate: (String) -> Unit,
) {
    private val lock = ReentrantLock()
    private val entries = mutableMapOf<String, RelayEntry>()
    val isNotEmpty: Boolean get() = lock.withLock { entries.isNotEmpty() }

    fun put(token: String, source: VesperPlayerSource, adaptation: VesperRelayFormatAdaptationRegistration?): RelayEntry {
        val entry = RelayEntry(source, adaptation, tokenExpiresAtMillis())
        lock.withLock {
            check(token !in entries) { "Relay token is already registered." }
            entries[token] = entry
        }
        return entry
    }

    fun remove(token: String) {
        val detached = lock.withLock { entries.remove(token)?.let { detach(token, it) } }
        detached?.cleanup?.forEach { resource -> runCatching { resource.close() } }
    }

    fun invalidate(token: String) {
        val detached = lock.withLock { entries.remove(token)?.let { detach(token, it) } }
        detached?.cleanup?.forEach { resource -> runCatching { resource.close() } }
        onInvalidate(token)
    }

    fun invalidateAll() = detachAllForInvalidation().forEach(::invalidateDetached)

    // State is detached atomically; sockets, input operations, and JNI close
    // only after the caller releases any server monitor.
    fun detachAllForInvalidation(): List<DetachedRelayEntry> = lock.withLock {
        entries.map { (token, entry) -> detach(token, entry) }.also { entries.clear() }
    }

    fun invalidateDetached(detached: DetachedRelayEntry) {
        detached.cleanup.forEach { resource -> runCatching { resource.close() } }
        onInvalidate(detached.token)
    }

    fun pin(token: String): Boolean = lock.withLock {
        val entry = entries[token] ?: return@withLock false
        if (entry.expiresAtMillis?.let { it <= nowMillisProvider() } == true) return@withLock false
        entries[token] = entry.copy(expiresAtMillis = null)
        true
    }

    fun entryForToken(token: String): RelayEntry? {
        var expired: DetachedRelayEntry? = null
        val entry = lock.withLock {
            val current = entries[token]
            if (current?.expiresAtMillis?.let { it <= nowMillisProvider() } == true) {
                expired = entries.remove(token)?.let { detach(token, it) }
                null
            } else { current }
        }
        expired?.let(::invalidateDetached)
        return entry
    }

    fun pruneExpiredEntries() {
        val now = nowMillisProvider()
        val expired = lock.withLock {
            val removed = entries.filterValues { it.expiresAtMillis?.let { expiry -> expiry <= now } == true }
            removed.keys.forEach(entries::remove)
            removed.map { (token, entry) -> detach(token, entry) }
        }
        expired.forEach(::invalidateDetached)
    }

    private fun detach(token: String, entry: RelayEntry) =
        DetachedRelayEntry(token, entry, entry.lifetime.retire())

    private fun tokenExpiresAtMillis(): Long? = tokenTtlMillis?.takeIf { it > 0 }?.let { nowMillisProvider() + it }
}

internal data class RelayEntry(
    val source: VesperPlayerSource,
    val adaptation: VesperRelayFormatAdaptationRegistration?,
    val expiresAtMillis: Long?,
    val lifetime: VesperRelayLifetime = VesperRelayLifetime(),
)

internal data class DetachedRelayEntry(val token: String, val entry: RelayEntry, val cleanup: List<java.io.Closeable> = emptyList())
