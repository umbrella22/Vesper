package io.github.umbrella22.vesper.player.android.external.internal.dlna

import android.content.Context
import android.net.wifi.WifiManager
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.MulticastSocket
import java.net.SocketAddress
import java.net.SocketTimeoutException
import java.util.concurrent.Executors

internal fun VesperDlnaDiscovery.ensureNotifyListener(binding: DlnaNetworkBinding, generation: Long) {
    val bindingKey = binding.key
    if (notifyBindingKey == bindingKey && notifyExecutor != null) {
        return
    }
    stopNotifyListener()
    notifyBindingKey = bindingKey
    notifyExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "vesper-dlna-notify").apply { isDaemon = true }
    }
    notifyExecutor?.execute { runNotifyLoop(binding, generation) }
}

internal fun VesperDlnaDiscovery.runNotifyLoop(binding: DlnaNetworkBinding, generation: Long) {
    val networkInterface = binding.networkInterface
    if (networkInterface == null) {
        emitDiagnostic(
            code = "notify_interface_unavailable",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
            message = "No LAN interface is available for SSDP NOTIFY listening.",
            details = binding.details(),
        )
        return
    }
    val group = InetSocketAddress(InetAddress.getByName(SSDP_ADDRESS), SSDP_PORT)
    var joined = false
    try {
        MulticastSocket(null as SocketAddress?).use { socket ->
            socket.reuseAddress = true
            socket.soTimeout = NOTIFY_RECEIVE_TIMEOUT_MS
            val boundPort = bindNotifySocket(socket, binding)
            if (!bindSocketToNetwork(socket, binding)) {
                return
            }
            socket.setNetworkInterface(networkInterface)
            try {
                socket.joinGroup(group, networkInterface)
            } catch (error: IOException) {
                emitDiagnostic(
                    code = "notify_join_interface_failed",
                    severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                    message = error.message ?: "SSDP NOTIFY multicast join failed on the LAN interface.",
                    details = binding.details("error" to error.javaClass.simpleName),
                )
                @Suppress("DEPRECATION")
                socket.joinGroup(group.address)
            }
            joined = true
            notifySocket = socket
            emitDiagnostic(
                code = "notify_listener_started",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                message = "SSDP NOTIFY listener started on the LAN interface.",
                details = binding.details("port" to boundPort.toString()),
            )
            try {
                val buffer = ByteArray(SSDP_BUFFER_BYTES)
                while (isDiscoveryActive(generation) && !Thread.currentThread().isInterrupted) {
                    val packet = DatagramPacket(buffer, buffer.size)
                    try {
                        socket.receive(packet)
                        val raw = String(packet.data, packet.offset, packet.length, Charsets.UTF_8)
                        handleSsdp(raw, binding, generation)
                    } catch (_: SocketTimeoutException) {
                    }
                }
            } finally {
                if (joined) {
                    runCatching {
                        socket.leaveGroup(group, networkInterface)
                    }.onFailure {
                        @Suppress("DEPRECATION")
                        runCatching { socket.leaveGroup(group.address) }
                    }
                    joined = false
                }
            }
        }
    } catch (error: SecurityException) {
        if (running.get()) {
            emitDiagnostic(
                code = "notify_permission_denied",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
                message = error.message ?: "Permission denied while starting SSDP NOTIFY listening.",
                details = binding.details(),
            )
        }
    } catch (error: IOException) {
        if (running.get()) {
            emitDiagnostic(
                code = "notify_listener_unavailable",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                message = error.message ?: "SSDP NOTIFY listener could not be started.",
                details = binding.details("error" to error.javaClass.simpleName),
            )
        }
    } finally {
        notifySocket = null
    }
}

internal fun VesperDlnaDiscovery.bindNotifySocket(socket: MulticastSocket, binding: DlnaNetworkBinding): Int {
    try {
        socket.bind(InetSocketAddress(SSDP_PORT))
        return SSDP_PORT
    } catch (error: IOException) {
        emitDiagnostic(
            code = "notify_port_unavailable",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
            message = error.message ?: "SSDP NOTIFY port 1900 is already in use; falling back to an ephemeral listener.",
            details = binding.details("error" to error.javaClass.simpleName),
        )
    }

    socket.bind(InetSocketAddress(0))
    return socket.localPort
}

internal fun VesperDlnaDiscovery.isDiscoveryActive(generation: Long): Boolean =
    running.get() && discoveryGeneration.get() == generation

internal fun VesperDlnaDiscovery.stopNotifyListener() {
    notifyBindingKey = null
    runCatching { notifySocket?.close() }
    notifySocket = null
    notifyExecutor?.shutdownNow()
    notifyExecutor = null
}

internal fun VesperDlnaDiscovery.acquireMulticastLock() {
    val wifiManager = appContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
    if (wifiManager == null) {
        emitDiagnostic(
            code = "multicast_lock_unavailable",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
            message = "WifiManager is unavailable, so DLNA multicast lock was not acquired.",
        )
        return
    }
    try {
        multicastLock = wifiManager
            .createMulticastLock("vesper-player-dlna-discovery")
            .apply {
                setReferenceCounted(false)
                acquire()
            }
    } catch (error: SecurityException) {
        emitDiagnostic(
            code = "multicast_lock_permission_denied",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
            message = error.message ?: "Permission denied while acquiring Wi-Fi multicast lock.",
        )
    }
}

internal fun VesperDlnaDiscovery.bindSocketToNetwork(
    socket: DatagramSocket,
    binding: DlnaNetworkBinding,
): Boolean {
    try {
        binding.network.bindSocket(socket)
        return true
    } catch (error: Exception) {
        emitDiagnostic(
            code = "network_bind_socket_failed",
            severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
            message = error.message ?: "Socket could not be bound to the Android network.",
            details = binding.details("error" to error.javaClass.simpleName),
        )
        return false
    }
}
