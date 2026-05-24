package io.github.ikaros.vesper.player.android.external.internal.relay

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import io.github.ikaros.vesper.player.android.external.internal.net.isLikelyTunnelInterfaceName
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface

fun findLanIpv4Address(): InetAddress? =
    NetworkInterface.getNetworkInterfaces()
        .asSequence()
        .filter { it.isUsableLanInterface() }
        .flatMap { it.inetAddresses.asSequence() }
        .filterIsInstance<Inet4Address>()
        .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }

@Suppress("DEPRECATION")
internal fun Context.findWifiLanIpv4Address(): InetAddress? {
    val connectivityManager =
        getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            ?: return null
    return runCatching {
        connectivityManager.allNetworks
            .asSequence()
            .mapNotNull { network ->
                val capabilities = connectivityManager.getNetworkCapabilities(network)
                    ?: return@mapNotNull null
                if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) ||
                    (!capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
                        !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET))
                ) {
                    return@mapNotNull null
                }
                val linkProperties = connectivityManager.getLinkProperties(network)
                    ?: return@mapNotNull null
                val interfaceName = linkProperties.interfaceName
                val networkInterface = interfaceName
                    ?.let { runCatching { NetworkInterface.getByName(it) }.getOrNull() }
                if (!networkInterface.isUsableLanInterface(interfaceName)) {
                    return@mapNotNull null
                }
                linkProperties.linkAddresses
                    .asSequence()
                    .map { it.address }
                    .filterIsInstance<Inet4Address>()
                    .firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                    ?: networkInterface
                        ?.inetAddresses
                        ?.asSequence()
                        ?.filterIsInstance<Inet4Address>()
                        ?.firstOrNull { !it.isLoopbackAddress && !it.isLinkLocalAddress }
            }
            .firstOrNull()
    }.getOrNull()
}

internal fun InetAddress.isBindableLanAddress(): Boolean =
    !isLinkLocalAddress

internal fun InetAddress.isAdvertisableLanAddress(): Boolean =
    isBindableLanAddress() && !isAnyLocalAddress

internal fun InetAddress.hasSameHostAddress(other: InetAddress): Boolean =
    hostAddress == other.hostAddress

private fun NetworkInterface.isUsableLanInterface(): Boolean =
    isUp && !isLoopback && !isPointToPoint && !isLikelyTunnelInterface()

private fun NetworkInterface?.isUsableLanInterface(interfaceName: String?): Boolean {
    if (interfaceName?.isLikelyTunnelInterfaceName() == true) {
        return false
    }
    val networkInterface = this ?: return true
    return runCatching { networkInterface.isUsableLanInterface() }.getOrDefault(false)
}

private fun NetworkInterface.isLikelyTunnelInterface(): Boolean {
    return name.isLikelyTunnelInterfaceName()
}

internal fun InetAddress.toRelayHost(): String =
    if (address.size == IPV6_ADDRESS_BYTES) {
        "[$hostAddress]"
    } else {
        hostAddress
    }

private const val IPV6_ADDRESS_BYTES = 16
