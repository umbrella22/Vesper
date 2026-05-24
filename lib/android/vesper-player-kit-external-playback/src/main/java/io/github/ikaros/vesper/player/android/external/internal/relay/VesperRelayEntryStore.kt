package io.github.ikaros.vesper.player.android.external.internal.relay

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.util.concurrent.ConcurrentHashMap

internal class VesperRelayEntryStore(
    private val tokenTtlMillis: Long?,
    private val nowMillisProvider: () -> Long,
    private val onInvalidate: (String) -> Unit,
) {
    private val entries = ConcurrentHashMap<String, RelayEntry>()

    val isNotEmpty: Boolean
        get() = entries.isNotEmpty()

    fun put(
        token: String,
        source: VesperPlayerSource,
        adaptation: VesperRelayFormatAdaptationRegistration?,
    ) {
        entries[token] = RelayEntry(
            source = source,
            adaptation = adaptation,
            expiresAtMillis = tokenExpiresAtMillis(),
        )
    }

    fun remove(token: String) {
        entries.remove(token)
    }

    fun invalidate(token: String) {
        entries.remove(token)
        onInvalidate(token)
    }

    fun invalidateAll() {
        entries.keys.forEach(onInvalidate)
        entries.clear()
    }

    fun entryForToken(token: String): RelayEntry? {
        val entry = entries[token] ?: return null
        val expiresAtMillis = entry.expiresAtMillis ?: return entry
        if (expiresAtMillis <= nowMillisProvider()) {
            entries.remove(token, entry)
            onInvalidate(token)
            return null
        }
        return entry
    }

    fun pruneExpiredEntries() {
        val now = nowMillisProvider()
        entries.forEach { (token, entry) ->
            val expired = entry.expiresAtMillis?.let { it <= now } ?: false
            if (expired && entries.remove(token, entry)) {
                onInvalidate(token)
            }
        }
    }

    private fun tokenExpiresAtMillis(): Long? {
        val ttl = tokenTtlMillis?.takeIf { it > 0L } ?: return null
        return nowMillisProvider() + ttl
    }
}

internal data class RelayEntry(
    val source: VesperPlayerSource,
    val adaptation: VesperRelayFormatAdaptationRegistration?,
    val expiresAtMillis: Long?,
)
