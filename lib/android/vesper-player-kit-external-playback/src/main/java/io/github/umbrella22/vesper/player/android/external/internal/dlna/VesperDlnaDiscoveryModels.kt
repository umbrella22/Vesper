package io.github.umbrella22.vesper.player.android.external.internal.dlna

import android.net.Network
import android.net.NetworkCapabilities
import io.github.umbrella22.vesper.player.android.external.internal.net.isLikelyTunnelInterfaceName
import java.net.Inet4Address
import java.net.NetworkInterface

internal data class DlnaNetworkBinding(
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

internal fun DlnaNetworkBinding.details(
    vararg entries: Pair<String, String>,
): Map<String, String> =
    buildMap {
        put("interface", interfaceName.orEmpty())
        put("localAddress", localAddress.hostAddress.orEmpty())
        put("transport", if (transportRank == TRANSPORT_RANK_WIFI) "wifi" else "ethernet")
        put("active", active.toString())
        entries.forEach { (key, value) -> put(key, value) }
    }

internal fun List<DlnaNetworkBinding>.details(): Map<String, String> =
    buildMap {
        put("bindingCount", size.toString())
        put("interfaces", joinToString(",") { it.interfaceName.orEmpty() })
        put("localAddresses", joinToString(",") { it.localAddress.hostAddress.orEmpty() })
    }

internal fun NetworkCapabilities.dlnaTransportRank(): Int? =
    when {
        hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> null
        hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> TRANSPORT_RANK_WIFI
        hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> TRANSPORT_RANK_ETHERNET
        else -> null
    }

internal fun NetworkInterface?.isUsableDlnaInterface(interfaceName: String?): Boolean {
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

internal fun VesperDlnaDescriptionRequest.details(
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

internal fun mSearchPayload(target: String, mx: Int): String =
    buildString {
        append("M-SEARCH * HTTP/1.1\r\n")
        append("HOST: $SSDP_ADDRESS:$SSDP_PORT\r\n")
        append("MAN: \"ssdp:discover\"\r\n")
        append("MX: ").append(mx.coerceIn(1, 3)).append("\r\n")
        append("ST: ").append(target).append("\r\n")
        append("\r\n")
    }

internal val M_SEARCH_TARGETS = listOf(
    "urn:schemas-upnp-org:device:MediaRenderer:1",
    "ssdp:all",
    "upnp:rootdevice",
)
internal const val M_SEARCH_ROUNDS = 3
internal const val SSDP_TTL = 2
internal const val SSDP_ADDRESS = "239.255.255.250"
internal const val SSDP_PORT = 1900
internal const val SSDP_BUFFER_BYTES = 65_535
internal const val SSDP_RECEIVE_TIMEOUT_MS = 900
internal const val SSDP_RECEIVE_WINDOW_MS = 4_000L
internal const val NOTIFY_RECEIVE_TIMEOUT_MS = 1_000
internal const val DESCRIPTION_TIMEOUT_MS = 5_000
internal const val DISCOVERY_INTERVAL_MS = 8_000L
internal const val TRANSPORT_RANK_WIFI = 0
internal const val TRANSPORT_RANK_ETHERNET = 1
internal const val LOG_TAG = "VesperDlnaDiscovery"
