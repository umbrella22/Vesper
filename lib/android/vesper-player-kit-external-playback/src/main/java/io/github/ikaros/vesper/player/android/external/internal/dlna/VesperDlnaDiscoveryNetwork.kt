package io.github.ikaros.vesper.player.android.external.internal.dlna

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import io.github.ikaros.vesper.player.android.external.internal.net.isLikelyTunnelInterfaceName
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.NetworkInterface
@Suppress("DEPRECATION")
internal fun VesperDlnaDiscovery.resolveLanBindings(): List<DlnaNetworkBinding> {
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
