package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayLifetime
import java.io.Closeable
import java.io.InterruptedIOException
import java.util.concurrent.CancellationException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/** Publishes the preparation slot before any network, FIFO, or JNI work. */
internal class VesperRelaySessionRegistry<T : Closeable> {
    private val sessions = ConcurrentHashMap<String, Preparing<T>>()

    fun acquire(key: String, owner: VesperRelayLifetime, prepare: (VesperRelayLifetime) -> T): T {
        owner.checkActive()
        val candidate = Preparing(owner, prepare)
        val slot = sessions.putIfAbsent(key, candidate) ?: candidate
        if (!owner.attach(slot)) {
            sessions.remove(key, slot)
            throw InterruptedIOException("Relay input preparation was cancelled.")
        }
        try {
            slot.task.run()
            return slot.task.get(90, TimeUnit.SECONDS).also {
                owner.checkActive()
                slot.lifetime.checkActive()
            }
        } catch (error: ExecutionException) {
            sessions.remove(key, slot)
            slot.close()
            owner.detach(slot)
            throw (error.cause ?: error)
        } catch (error: Exception) {
            if (error is CancellationException || error is TimeoutException || error is InterruptedException || error is InterruptedIOException) {
                sessions.remove(key, slot)
                slot.close()
                owner.detach(slot)
                if (error is InterruptedException) Thread.currentThread().interrupt()
                throw InterruptedIOException("Relay input preparation was cancelled or exceeded its deadline.")
            }
            throw error
        }
    }

    fun invalidate(key: String) { sessions.remove(key)?.close() }

    private class Preparing<T : Closeable>(private val owner: VesperRelayLifetime, prepare: (VesperRelayLifetime) -> T) : Closeable {
        val lifetime = VesperRelayLifetime()
        val task = FutureTask {
            lifetime.checkActive()
            val session = prepare(lifetime)
            if (!lifetime.attach(session)) throw InterruptedIOException("Relay input preparation was cancelled.")
            session
        }

        override fun close() {
            owner.detach(this)
            task.cancel(true)
            lifetime.close()
        }
    }
}
