package io.github.ikaros.vesper.player.android.external.internal.dlna

import android.security.NetworkSecurityPolicy
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.MulticastSocket
import java.net.SocketAddress
import java.net.SocketTimeoutException
import java.net.URL
import java.util.concurrent.ThreadLocalRandom

internal fun VesperDlnaDiscovery.searchOnce(binding: DlnaNetworkBinding, generation: Long): Int {
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

internal fun VesperDlnaDiscovery.receiveSearchResponses(
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

internal fun VesperDlnaDiscovery.handleSsdp(raw: String, binding: DlnaNetworkBinding, generation: Long) {
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

internal fun VesperDlnaDiscovery.fetchDevice(
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
