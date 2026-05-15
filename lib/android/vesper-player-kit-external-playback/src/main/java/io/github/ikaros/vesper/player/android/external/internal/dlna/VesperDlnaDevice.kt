package io.github.ikaros.vesper.player.android.external.internal.dlna

import java.net.URL

data class VesperDlnaService(
    val serviceType: String,
    val serviceId: String,
    val controlUrl: URL,
    val eventSubUrl: URL?,
    val scpdUrl: URL?,
)

data class VesperDlnaDevice(
    val routeId: String,
    val location: URL,
    val usn: String,
    val friendlyName: String,
    val manufacturer: String? = null,
    val modelName: String? = null,
    val udn: String? = null,
    val deviceType: String? = null,
    val avTransport: VesperDlnaService? = null,
    val renderingControl: VesperDlnaService? = null,
    val connectionManager: VesperDlnaService? = null,
    val expiresAtMillis: Long = Long.MAX_VALUE,
) {
    val supportsPlayback: Boolean
        get() = avTransport != null
}

internal fun String.isAvTransportService(): Boolean =
    startsWith("urn:schemas-upnp-org:service:AVTransport:", ignoreCase = true)

internal fun String.isRenderingControlService(): Boolean =
    startsWith("urn:schemas-upnp-org:service:RenderingControl:", ignoreCase = true)

internal fun String.isConnectionManagerService(): Boolean =
    startsWith("urn:schemas-upnp-org:service:ConnectionManager:", ignoreCase = true)
