package io.github.umbrella22.vesper.player.android.external.internal.relay

import java.io.Closeable
import java.io.InterruptedIOException

/** One token's resources, including operations that have not published a result. */
internal class VesperRelayLifetime : Closeable {
    private val lock = Any()
    private val resources = linkedSetOf<Closeable>()
    @Volatile
    var isActive: Boolean = true
        private set

    fun attach(resource: Closeable): Boolean {
        val attached = synchronized(lock) {
            if (isActive) { resources.add(resource); true } else { false }
        }
        if (!attached) runCatching { resource.close() }
        return attached
    }

    fun detach(resource: Closeable) {
        synchronized(lock) { resources.remove(resource) }
    }

    fun checkActive() {
        if (!isActive) throw InterruptedIOException("Relay session was cancelled.")
    }

    fun retire(): List<Closeable> = synchronized(lock) {
            if (!isActive) return emptyList()
            isActive = false
            resources.toList().also { resources.clear() }
    }

    override fun close() { retire().forEach { resource -> runCatching { resource.close() } } }
}

internal class VesperRelayCloseOnce(private val release: () -> Unit) : Closeable {
    private val closed = java.util.concurrent.atomic.AtomicBoolean(false)
    override fun close() { if (closed.compareAndSet(false, true)) release() }
}
