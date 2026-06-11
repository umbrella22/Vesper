package io.github.ikaros.vesper.player.android.external.internal.dlna

import android.content.Context
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.MulticastSocket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

enum class VesperDlnaDiscoveryDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

data class VesperDlnaDiscoveryDiagnostic(
    val code: String,
    val severity: VesperDlnaDiscoveryDiagnosticSeverity,
    val message: String,
    val details: Map<String, String> = emptyMap(),
)

class VesperDlnaDiscovery(
    context: Context,
    internal val listener: Listener,
) {
    interface Listener {
        fun onRoutesChanged(routes: List<VesperDlnaDevice>)
        fun onDiscoveryError(message: String)
        fun onDiscoveryDiagnostic(diagnostic: VesperDlnaDiscoveryDiagnostic) = Unit
    }

    internal val appContext = context.applicationContext
    internal val running = AtomicBoolean(false)
    internal val discoveryGeneration = AtomicLong(0)
    internal val wakeLock = ReentrantLock()
    internal val wakeCondition = wakeLock.newCondition()
    internal val routeLock = Any()
    internal val devices = ConcurrentHashMap<String, VesperDlnaDevice>()
    internal val pendingDescriptionFetches = ConcurrentHashMap.newKeySet<String>()
    private var executor: ExecutorService? = null
    internal var notifyExecutor: ExecutorService? = null
    internal var notifySocket: MulticastSocket? = null
    internal var notifyBindingKey: String? = null
    internal var multicastLock: android.net.wifi.WifiManager.MulticastLock? = null

    fun start() {
        if (!running.compareAndSet(false, true)) {
            emitDiagnostic(
                code = "discovery_refresh_requested",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                message = "DLNA discovery refresh was requested while discovery is already running.",
            )
            wakeDiscoveryLoop()
            return
        }
        val generation = discoveryGeneration.incrementAndGet()
        acquireMulticastLock()
        emitDiagnostic(
            code = "discovery_started",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
            message = "DLNA discovery started.",
            details = mapOf("generation" to generation.toString()),
        )
        executor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "vesper-dlna-discovery").apply { isDaemon = true }
        }
        executor?.execute { runDiscoveryLoop(generation) }
    }

    fun stop() {
        running.set(false)
        discoveryGeneration.incrementAndGet()
        wakeDiscoveryLoop()
        stopNotifyListener()
        pendingDescriptionFetches.clear()
        executor?.shutdownNow()
        executor = null
        multicastLock?.let { lock ->
            runCatching {
                if (lock.isHeld) {
                    lock.release()
                }
            }
        }
        multicastLock = null
        synchronized(routeLock) {
            devices.clear()
            listener.onRoutesChanged(emptyList())
        }
    }

    internal fun wakeDiscoveryLoop() {
        wakeLock.withLock {
            wakeCondition.signalAll()
        }
    }

    private fun runDiscoveryLoop(generation: Long) {
        while (isDiscoveryActive(generation) && !Thread.currentThread().isInterrupted) {
            val keepRunning = runCatching {
                pruneExpired(generation)
                val bindings = resolveLanBindings()
                if (bindings.isEmpty()) {
                    emitDiagnostic(
                        code = "lan_network_unavailable",
                        severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                        message = "No Wi-Fi or Ethernet network with an IPv4 address is available for DLNA discovery.",
                    )
                } else {
                    ensureNotifyListener(bindings.first(), generation)
                    val responseCount = bindings.sumOf { binding -> searchOnce(binding, generation) }
                    if (responseCount == 0) {
                        emitDiagnostic(
                            code = "ssdp_no_response",
                            severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                            message = "No SSDP responses were received on the LAN interfaces.",
                            details = bindings.details(),
                        )
                    }
                }
                true
            }.getOrElse { error ->
                if (error is InterruptedException) {
                    Thread.currentThread().interrupt()
                    false
                } else {
                    if (running.get()) {
                        val message = error.message ?: "DLNA discovery failed."
                        listener.onDiscoveryError(message)
                        emitDiagnostic(
                            code = "discovery_loop_failed",
                            severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
                            message = message,
                        )
                    }
                    true
                }
            }
            if (!keepRunning || !running.get()) {
                break
            }
            try {
                wakeLock.withLock {
                    wakeCondition.await(DISCOVERY_INTERVAL_MS, TimeUnit.MILLISECONDS)
                }
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                break
            }
        }
    }


}
