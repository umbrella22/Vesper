package io.github.ikaros.vesper.player.android.dlna

import android.content.Context
import android.net.wifi.WifiManager
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.URL
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class VesperDlnaDiscovery(
    context: Context,
    private val listener: Listener,
) {
    interface Listener {
        fun onRoutesChanged(routes: List<VesperDlnaDevice>)
        fun onDiscoveryError(message: String)
    }

    private val appContext = context.applicationContext
    private val running = AtomicBoolean(false)
    private val devices = ConcurrentHashMap<String, VesperDlnaDevice>()
    private var executor: ExecutorService? = null
    private var multicastLock: WifiManager.MulticastLock? = null

    fun start() {
        if (!running.compareAndSet(false, true)) {
            return
        }
        acquireMulticastLock()
        executor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "vesper-dlna-discovery").apply { isDaemon = true }
        }
        executor?.execute(::runDiscoveryLoop)
    }

    fun stop() {
        running.set(false)
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
        devices.clear()
        listener.onRoutesChanged(emptyList())
    }

    private fun runDiscoveryLoop() {
        while (running.get() && !Thread.currentThread().isInterrupted) {
            val keepRunning = runCatching {
                pruneExpired()
                searchOnce()
                true
            }.getOrElse { error ->
                if (error is InterruptedException) {
                    Thread.currentThread().interrupt()
                    false
                } else {
                    if (running.get()) {
                        listener.onDiscoveryError(error.message ?: "DLNA discovery failed.")
                    }
                    true
                }
            }
            if (!keepRunning || !running.get()) {
                break
            }
            try {
                Thread.sleep(DISCOVERY_INTERVAL_MS)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                break
            }
        }
    }

    private fun searchOnce() {
        DatagramSocket().use { socket ->
            socket.soTimeout = 2_000
            val address = InetAddress.getByName(SSDP_ADDRESS)
            val targets = listOf(
                "urn:schemas-upnp-org:device:MediaRenderer:1",
                "ssdp:all",
            )
            for (target in targets) {
                val payload = mSearchPayload(target).toByteArray(Charsets.UTF_8)
                socket.send(DatagramPacket(payload, payload.size, address, SSDP_PORT))
            }
            val buffer = ByteArray(SSDP_BUFFER_BYTES)
            val deadline = System.currentTimeMillis() + 2_500
            while (running.get() && System.currentTimeMillis() < deadline) {
                val packet = DatagramPacket(buffer, buffer.size)
                runCatching {
                    socket.receive(packet)
                    val raw = String(packet.data, packet.offset, packet.length, Charsets.UTF_8)
                    handleSsdp(raw)
                }
            }
        }
    }

    private fun handleSsdp(raw: String) {
        if (!running.get()) {
            return
        }
        val message = VesperSsdpParser.parse(raw) ?: return
        if (message.isByebyeNotify) {
            val usn = message.usn ?: return
            devices.remove(usn)
            emitRoutes()
            return
        }
        if (!message.isMediaRenderer) {
            return
        }
        val request = message.toDescriptionRequest(System.currentTimeMillis()) ?: return
        val device = fetchDevice(request) ?: return
        if (!running.get()) {
            return
        }
        devices[device.usn] = device
        emitRoutes()
    }

    private fun fetchDevice(request: VesperDlnaDescriptionRequest): VesperDlnaDevice? {
        val connection = request.location.openConnection() as HttpURLConnection
        connection.connectTimeout = 5_000
        connection.readTimeout = 5_000
        val xml = connection.inputStream.bufferedReader().use { it.readText() }
        connection.disconnect()
        return runCatching {
            VesperDlnaDeviceDescriptionParser.parse(
                xml = xml,
                location = request.location,
                usn = request.usn,
                expiresAtMillis = request.expiresAtMillis,
            )
        }.getOrNull()
    }

    private fun pruneExpired() {
        val now = System.currentTimeMillis()
        val removed = devices.entries.removeIf { it.value.expiresAtMillis <= now }
        if (removed) {
            emitRoutes()
        }
    }

    private fun emitRoutes() {
        listener.onRoutesChanged(
            devices.values
                .filter { it.supportsPlayback }
                .sortedBy { it.friendlyName.lowercase() },
        )
    }

    private fun acquireMulticastLock() {
        val wifiManager = appContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        multicastLock = wifiManager
            ?.createMulticastLock("vesper-player-dlna-discovery")
            ?.apply {
                setReferenceCounted(false)
                acquire()
            }
    }
}

private fun mSearchPayload(target: String): String =
    buildString {
        append("M-SEARCH * HTTP/1.1\r\n")
        append("HOST: $SSDP_ADDRESS:$SSDP_PORT\r\n")
        append("MAN: \"ssdp:discover\"\r\n")
        append("MX: 2\r\n")
        append("ST: ").append(target).append("\r\n")
        append("\r\n")
    }

private const val SSDP_ADDRESS = "239.255.255.250"
private const val SSDP_PORT = 1900
private const val SSDP_BUFFER_BYTES = 65_535
private const val DISCOVERY_INTERVAL_MS = 8_000L
