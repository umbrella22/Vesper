package io.github.ikaros.vesper.player.android.external.internal.dlna

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.security.NetworkSecurityPolicy
import android.util.Log
import io.github.ikaros.vesper.player.android.external.internal.net.isLikelyTunnelInterfaceName
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.MulticastSocket
import java.net.NetworkInterface
import java.net.SocketAddress
import java.net.SocketTimeoutException
import java.net.URL
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.ThreadLocalRandom
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
    private val listener: Listener,
) {
    interface Listener {
        fun onRoutesChanged(routes: List<VesperDlnaDevice>)
        fun onDiscoveryError(message: String)
        fun onDiscoveryDiagnostic(diagnostic: VesperDlnaDiscoveryDiagnostic) = Unit
    }

    private val appContext = context.applicationContext
    private val running = AtomicBoolean(false)
    private val discoveryGeneration = AtomicLong(0)
    private val wakeLock = ReentrantLock()
    private val wakeCondition = wakeLock.newCondition()
    private val routeLock = Any()
    private val devices = ConcurrentHashMap<String, VesperDlnaDevice>()
    private val pendingDescriptionFetches = ConcurrentHashMap.newKeySet<String>()
    private var executor: ExecutorService? = null
    private var notifyExecutor: ExecutorService? = null
    private var notifySocket: MulticastSocket? = null
    private var notifyBindingKey: String? = null
    private var multicastLock: WifiManager.MulticastLock? = null

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

    private fun wakeDiscoveryLoop() {
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

    private fun searchOnce(binding: DlnaNetworkBinding, generation: Long): Int {
        try {
            MulticastSocket(null as SocketAddress?).use { socket ->
                socket.reuseAddress = true
                socket.timeToLive = SSDP_TTL
                binding.networkInterface?.let(socket::setNetworkInterface)
                socket.bind(InetSocketAddress(binding.localAddress, 0))
                bindSocketToNetwork(socket, binding)
                socket.soTimeout = SSDP_RECEIVE_TIMEOUT_MS
                val address = InetAddress.getByName(SSDP_ADDRESS)
                var responseCount = 0
                repeat(M_SEARCH_ROUNDS) { round ->
                    val mx = ThreadLocalRandom.current().nextInt(1, 4)
                    for (target in M_SEARCH_TARGETS) {
                        val payload = mSearchPayload(target, mx).toByteArray(Charsets.UTF_8)
                        socket.send(DatagramPacket(payload, payload.size, address, SSDP_PORT))
                    }
                    emitDiagnostic(
                        code = "m_search_sent",
                        severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                        message = "DLNA M-SEARCH probes were sent on the LAN interface.",
                        details = binding.details("round" to (round + 1).toString()),
                    )
                    responseCount += receiveSearchResponses(socket, binding, generation)
                }
                return responseCount
            }
        } catch (error: SecurityException) {
            emitDiagnostic(
                code = "m_search_permission_denied",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
                message = error.message ?: "Permission denied while sending DLNA M-SEARCH probes.",
                details = binding.details(),
            )
            return 0
        } catch (error: IOException) {
            emitDiagnostic(
                code = "m_search_unavailable",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                message = error.message ?: "DLNA M-SEARCH could not be sent on the LAN interface.",
                details = binding.details("error" to error.javaClass.simpleName),
            )
            return 0
        }
    }

    private fun receiveSearchResponses(
        socket: DatagramSocket,
        binding: DlnaNetworkBinding,
        generation: Long,
    ): Int {
        val buffer = ByteArray(SSDP_BUFFER_BYTES)
        val deadline = System.currentTimeMillis() + SSDP_RECEIVE_WINDOW_MS
        var responseCount = 0
        while (isDiscoveryActive(generation) && System.currentTimeMillis() < deadline) {
            val packet = DatagramPacket(buffer, buffer.size)
            try {
                val remainingMs = (deadline - System.currentTimeMillis()).coerceAtLeast(1L)
                socket.soTimeout = minOf(SSDP_RECEIVE_TIMEOUT_MS, remainingMs.toInt())
                socket.receive(packet)
                responseCount += 1
                val raw = String(packet.data, packet.offset, packet.length, Charsets.UTF_8)
                handleSsdp(raw, binding, generation)
            } catch (_: SocketTimeoutException) {
                continue
            } catch (error: IOException) {
                if (running.get()) {
                    emitDiagnostic(
                        code = "ssdp_receive_failed",
                        severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                        message = error.message ?: "Failed to receive an SSDP response.",
                    )
                }
                break
            }
        }
        return responseCount
    }

    private fun handleSsdp(raw: String, binding: DlnaNetworkBinding, generation: Long) {
        if (!isDiscoveryActive(generation)) {
            return
        }
        val message = VesperSsdpParser.parse(raw) ?: return
        if (message.isByebyeNotify) {
            val usn = message.usn ?: return
            val routeId = canonicalDlnaRouteId(usn)
            if (removeDevice(routeId, generation)) {
                emitDiagnostic(
                    code = "route_byebye",
                    severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                    message = "DLNA device announced that it is leaving.",
                    details = mapOf("routeId" to routeId),
                )
            }
            return
        }
        if (!message.shouldFetchDescription) {
            return
        }
        val request = message.toDescriptionRequest(System.currentTimeMillis()) ?: return
        if (refreshKnownDevice(request, binding, generation)) {
            return
        }
        val fetchKey = request.descriptionFetchKey()
        if (!pendingDescriptionFetches.add(fetchKey)) {
            emitDiagnostic(
                code = "description_fetch_coalesced",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                message = "A duplicate DLNA device description fetch is already in progress.",
                details = request.details("fetchKey" to fetchKey),
            )
            return
        }
        try {
            val device = fetchDevice(request, binding, generation) ?: return
            upsertDevice(device, generation)
        } finally {
            pendingDescriptionFetches.remove(fetchKey)
        }
    }

    private fun fetchDevice(
        request: VesperDlnaDescriptionRequest,
        binding: DlnaNetworkBinding,
        generation: Long,
    ): VesperDlnaDevice? {
        if (!isDiscoveryActive(generation)) {
            return null
        }
        if (request.location.protocol.equals("http", ignoreCase = true) &&
            !NetworkSecurityPolicy.getInstance().isCleartextTrafficPermitted(request.location.host)
        ) {
            emitDiagnostic(
                code = "cleartext_http_blocked",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
                message = "Android cleartext HTTP policy blocks the DLNA device description request.",
                details = request.details("host" to request.location.host),
            )
            return null
        }
        var connection: HttpURLConnection? = null
        return try {
            connection = binding.network.openConnection(request.location) as HttpURLConnection
            connection.connectTimeout = DESCRIPTION_TIMEOUT_MS
            connection.readTimeout = DESCRIPTION_TIMEOUT_MS
            connection.instanceFollowRedirects = true
            val status = connection.responseCode
            if (!isDiscoveryActive(generation)) {
                return null
            }
            if (status !in 200..299) {
                emitDiagnostic(
                    code = "description_http_status",
                    severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                    message = "DLNA device description returned HTTP $status.",
                    details = request.details("status" to status.toString()),
                )
                return null
            }
            val xml = connection.inputStream.bufferedReader().use { it.readText() }
            val device = try {
                VesperDlnaDeviceDescriptionParser.parse(
                    xml = xml,
                    location = request.location,
                    usn = request.usn,
                    expiresAtMillis = request.expiresAtMillis,
                )
            } catch (error: IllegalArgumentException) {
                emitDiagnostic(
                    code = "description_not_media_renderer",
                    severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                    message = error.message ?: "Device description is not a DLNA media renderer.",
                    details = request.details(),
                )
                return null
            } catch (error: Exception) {
                emitDiagnostic(
                    code = "description_parse_failed",
                    severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                    message = error.message ?: "Failed to parse DLNA device description.",
                    details = request.details("error" to error.javaClass.simpleName),
                )
                return null
            }
            if (!device.supportsPlayback) {
                emitDiagnostic(
                    code = "missing_av_transport",
                    severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                    message = "DLNA media renderer does not expose AVTransport.",
                    details = request.details("routeId" to device.routeId),
                )
                return null
            }
            val boundDevice = device.copy(
                network = binding.network,
                localAddress = binding.localAddress,
                interfaceName = binding.interfaceName,
            )
            emitDiagnostic(
                code = "route_accepted",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                message = "DLNA media renderer was accepted.",
                details = request.details(
                    "routeId" to boundDevice.routeId,
                    "name" to boundDevice.friendlyName,
                    "interface" to binding.interfaceName.orEmpty(),
                    "localAddress" to binding.localAddress.hostAddress.orEmpty(),
                ),
            )
            boundDevice
        } catch (_: SocketTimeoutException) {
            emitDiagnostic(
                code = "description_timeout",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                message = "Timed out while fetching DLNA device description.",
                details = request.details(),
            )
            null
        } catch (error: SecurityException) {
            emitDiagnostic(
                code = "description_permission_denied",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
                message = error.message ?: "Permission denied while fetching DLNA device description.",
                details = request.details(),
            )
            null
        } catch (error: IOException) {
            emitDiagnostic(
                code = "description_fetch_failed",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                message = error.message ?: "Failed to fetch DLNA device description.",
                details = request.details("error" to error.javaClass.simpleName),
            )
            null
        } finally {
            connection?.disconnect()
        }
    }

    private fun pruneExpired(generation: Long) {
        val now = System.currentTimeMillis()
        synchronized(routeLock) {
            if (!isDiscoveryActive(generation)) {
                return
            }
            val removed = devices.entries.removeIf { it.value.expiresAtMillis <= now }
            if (removed) {
                emitRoutesLocked()
            }
        }
    }

    private fun upsertDevice(device: VesperDlnaDevice, generation: Long) {
        synchronized(routeLock) {
            if (!isDiscoveryActive(generation)) {
                return
            }
            devices[device.routeId] = device
            emitRoutesLocked()
        }
    }

    private fun refreshKnownDevice(
        request: VesperDlnaDescriptionRequest,
        binding: DlnaNetworkBinding,
        generation: Long,
    ): Boolean =
        synchronized(routeLock) {
            if (!isDiscoveryActive(generation)) {
                return@synchronized false
            }
            val entry = devices.entries.firstOrNull { (_, device) ->
                device.matchesDescriptionRequest(request)
            } ?: return@synchronized false
            val refreshed = entry.value.copy(
                usn = request.usn,
                network = binding.network,
                localAddress = binding.localAddress,
                interfaceName = binding.interfaceName,
                expiresAtMillis = maxOf(entry.value.expiresAtMillis, request.expiresAtMillis),
            )
            devices[entry.key] = refreshed
            emitRoutesLocked()
            emitDiagnostic(
                code = "description_fetch_skipped_known_route",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Info,
                message = "Known DLNA route was refreshed from SSDP without refetching its description.",
                details = request.details("routeId" to refreshed.routeId),
            )
            true
        }

    private fun removeDevice(routeId: String, generation: Long): Boolean =
        synchronized(routeLock) {
            if (!isDiscoveryActive(generation)) {
                return@synchronized false
            }
            val directRemoved = devices.remove(routeId) != null
            val aliasKey = if (directRemoved) {
                null
            } else {
                devices.entries
                    .firstOrNull { (_, device) -> device.matchesRouteId(routeId) }
                    ?.key
            }
            val aliasRemoved = aliasKey?.let { devices.remove(it) != null } == true
            val removed = directRemoved || aliasRemoved
            if (removed) {
                emitRoutesLocked()
            }
            removed
        }

    private fun emitRoutesLocked() {
        listener.onRoutesChanged(
            devices.values
                .filter { it.supportsPlayback }
                .sortedBy { it.friendlyName.lowercase(Locale.US) },
        )
    }

    @Suppress("DEPRECATION")
    private fun resolveLanBindings(): List<DlnaNetworkBinding> {
        val connectivityManager =
            appContext.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return emptyList()
        val activeNetwork = connectivityManager.activeNetwork
        return try {
            connectivityManager.allNetworks
                .asSequence()
                .mapNotNull { network ->
                    val capabilities = connectivityManager.getNetworkCapabilities(network)
                        ?: return@mapNotNull null
                    val transportRank = capabilities.dlnaTransportRank()
                    if (transportRank == null) {
                        return@mapNotNull null
                    }
                    val linkProperties = connectivityManager.getLinkProperties(network)
                        ?: return@mapNotNull null
                    val interfaceName = linkProperties.interfaceName
                    val networkInterface = interfaceName
                        ?.let { runCatching { NetworkInterface.getByName(it) }.getOrNull() }
                    if (!networkInterface.isUsableDlnaInterface(interfaceName)) {
                        return@mapNotNull null
                    }
                    val address = linkProperties.linkAddresses
                        .asSequence()
                        .map { it.address }
                        .filterIsInstance<Inet4Address>()
                        .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                        ?: networkInterface
                            ?.inetAddresses
                            ?.asSequence()
                            ?.filterIsInstance<Inet4Address>()
                            ?.firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                        ?: return@mapNotNull null
                    DlnaNetworkBinding(
                        network = network,
                        interfaceName = interfaceName,
                        localAddress = address,
                        networkInterface = networkInterface,
                        transportRank = transportRank,
                        active = network == activeNetwork,
                    )
                }
                .distinctBy { it.key }
                .sortedWith(
                    compareByDescending<DlnaNetworkBinding> { it.active }
                        .thenBy { it.transportRank }
                        .thenBy { it.interfaceName.orEmpty() }
                        .thenBy { it.localAddress.hostAddress.orEmpty() },
                )
                .toList()
        } catch (error: SecurityException) {
            emitDiagnostic(
                code = "network_permission_denied",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Error,
                message = error.message ?: "Permission denied while resolving the LAN network.",
            )
            emptyList()
        } catch (error: Exception) {
            emitDiagnostic(
                code = "network_resolution_failed",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                message = error.message ?: "Failed to resolve the LAN network for DLNA discovery.",
                details = mapOf("error" to error.javaClass.simpleName),
            )
            emptyList()
        }
    }

    private fun ensureNotifyListener(binding: DlnaNetworkBinding, generation: Long) {
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

    private fun runNotifyLoop(binding: DlnaNetworkBinding, generation: Long) {
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
                bindSocketToNetwork(socket, binding)
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

    private fun bindNotifySocket(socket: MulticastSocket, binding: DlnaNetworkBinding): Int {
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

    private fun isDiscoveryActive(generation: Long): Boolean =
        running.get() && discoveryGeneration.get() == generation

    private fun stopNotifyListener() {
        notifyBindingKey = null
        runCatching { notifySocket?.close() }
        notifySocket = null
        notifyExecutor?.shutdownNow()
        notifyExecutor = null
    }

    private fun acquireMulticastLock() {
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

    private fun bindSocketToNetwork(
        socket: DatagramSocket,
        binding: DlnaNetworkBinding,
    ) {
        try {
            binding.network.bindSocket(socket)
        } catch (error: Exception) {
            emitDiagnostic(
                code = "network_bind_socket_failed",
                severity = VesperDlnaDiscoveryDiagnosticSeverity.Warning,
                message = error.message ?: "Socket could not be bound to the Android network.",
                details = binding.details("error" to error.javaClass.simpleName),
            )
        }
    }

    private fun emitDiagnostic(
        code: String,
        severity: VesperDlnaDiscoveryDiagnosticSeverity,
        message: String,
        details: Map<String, String> = emptyMap(),
    ) {
        if (!running.get()) {
            return
        }
        val filteredDetails = details.filterValues { it.isNotBlank() }
        logDiagnostic(code, severity, message, filteredDetails)
        listener.onDiscoveryDiagnostic(
            VesperDlnaDiscoveryDiagnostic(
                code = code,
                severity = severity,
                message = message,
                details = filteredDetails,
            ),
        )
    }

    private fun logDiagnostic(
        code: String,
        severity: VesperDlnaDiscoveryDiagnosticSeverity,
        message: String,
        details: Map<String, String>,
    ) {
        val detailText = details.entries.joinToString(", ") { (key, value) -> "$key=$value" }
        val logMessage = if (detailText.isBlank()) {
            "[$code] $message"
        } else {
            "[$code] $message | $detailText"
        }
        runCatching {
            when (severity) {
                VesperDlnaDiscoveryDiagnosticSeverity.Info -> Log.d(LOG_TAG, logMessage)
                VesperDlnaDiscoveryDiagnosticSeverity.Warning -> Log.w(LOG_TAG, logMessage)
                VesperDlnaDiscoveryDiagnosticSeverity.Error -> Log.e(LOG_TAG, logMessage)
            }
        }
    }
}

private data class DlnaNetworkBinding(
    val network: Network,
    val interfaceName: String?,
    val localAddress: Inet4Address,
    val networkInterface: NetworkInterface?,
    val transportRank: Int,
    val active: Boolean,
) {
    val key: String
        get() = "${interfaceName.orEmpty()}@${localAddress.hostAddress}"
}

private fun DlnaNetworkBinding.details(
    vararg entries: Pair<String, String>,
): Map<String, String> =
    buildMap {
        put("interface", interfaceName.orEmpty())
        put("localAddress", localAddress.hostAddress.orEmpty())
        put("transport", if (transportRank == TRANSPORT_RANK_WIFI) "wifi" else "ethernet")
        put("active", active.toString())
        entries.forEach { (key, value) -> put(key, value) }
    }

private fun List<DlnaNetworkBinding>.details(): Map<String, String> =
    buildMap {
        put("bindingCount", size.toString())
        put("interfaces", joinToString(",") { it.interfaceName.orEmpty() })
        put("localAddresses", joinToString(",") { it.localAddress.hostAddress.orEmpty() })
    }

private fun NetworkCapabilities.dlnaTransportRank(): Int? =
    when {
        hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> null
        hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> TRANSPORT_RANK_WIFI
        hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> TRANSPORT_RANK_ETHERNET
        else -> null
    }

private fun NetworkInterface?.isUsableDlnaInterface(interfaceName: String?): Boolean {
    if (interfaceName?.isLikelyTunnelInterfaceName() == true) {
        return false
    }
    val networkInterface = this ?: return true
    return runCatching {
        networkInterface.isUp &&
            !networkInterface.isLoopback &&
            !networkInterface.isPointToPoint &&
            !networkInterface.name.isLikelyTunnelInterfaceName()
    }.getOrDefault(false)
}

private fun VesperDlnaDescriptionRequest.details(
    vararg entries: Pair<String, String>,
): Map<String, String> =
    buildMap {
        put("location", location.toString())
        put("usn", usn)
        entries.forEach { (key, value) -> put(key, value) }
    }

internal fun VesperDlnaDescriptionRequest.descriptionFetchKey(): String =
    "${location.toExternalForm()}|${dlnaRouteIdentityKey(usn)}"

internal fun VesperDlnaDevice.matchesDescriptionRequest(
    request: VesperDlnaDescriptionRequest,
): Boolean =
    location.sameFile(request.location) ||
        matchesRouteId(request.usn)

private fun mSearchPayload(target: String, mx: Int): String =
    buildString {
        append("M-SEARCH * HTTP/1.1\r\n")
        append("HOST: $SSDP_ADDRESS:$SSDP_PORT\r\n")
        append("MAN: \"ssdp:discover\"\r\n")
        append("MX: ").append(mx.coerceIn(1, 3)).append("\r\n")
        append("ST: ").append(target).append("\r\n")
        append("\r\n")
    }

private val M_SEARCH_TARGETS = listOf(
    "urn:schemas-upnp-org:device:MediaRenderer:1",
    "ssdp:all",
    "upnp:rootdevice",
)
private const val M_SEARCH_ROUNDS = 3
private const val SSDP_TTL = 2
private const val SSDP_ADDRESS = "239.255.255.250"
private const val SSDP_PORT = 1900
private const val SSDP_BUFFER_BYTES = 65_535
private const val SSDP_RECEIVE_TIMEOUT_MS = 900
private const val SSDP_RECEIVE_WINDOW_MS = 4_000L
private const val NOTIFY_RECEIVE_TIMEOUT_MS = 1_000
private const val DESCRIPTION_TIMEOUT_MS = 5_000
private const val DISCOVERY_INTERVAL_MS = 8_000L
private const val TRANSPORT_RANK_WIFI = 0
private const val TRANSPORT_RANK_ETHERNET = 1
private const val LOG_TAG = "VesperDlnaDiscovery"
