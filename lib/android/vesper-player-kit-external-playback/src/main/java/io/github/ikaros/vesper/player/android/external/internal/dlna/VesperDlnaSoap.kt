package io.github.ikaros.vesper.player.android.external.internal.dlna

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import java.io.StringReader
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLConnection
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.w3c.dom.Element
import org.xml.sax.InputSource

data class VesperDlnaSoapResponse(
    val status: Int,
    val body: String,
) {
    val fault: VesperDlnaSoapFault?
        get() = VesperDlnaSoapFaultParser.parse(body)
}

data class VesperDlnaSoapFault(
    val code: String?,
    val description: String?,
)

object VesperDlnaSoapEnvelopeBuilder {
    fun build(action: String, serviceType: String, arguments: Map<String, String>): String =
        buildString {
            append("""<?xml version="1.0" encoding="utf-8"?>""")
            append("""<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" """)
            append("""s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">""")
            append("<s:Body><u:").append(action).append(""" xmlns:u="""")
            append(serviceType.xmlEscaped()).append("""">""")
            arguments.forEach { (name, value) ->
                append("<").append(name).append(">")
                append(value.xmlEscaped())
                append("</").append(name).append(">")
            }
            append("</u:").append(action).append("></s:Body></s:Envelope>")
        }
}

object VesperDlnaSoapFaultParser {
    fun parse(xml: String): VesperDlnaSoapFault? {
        if (!xml.contains("Fault", ignoreCase = true)) {
            return null
        }
        val document = runCatching {
            secureDocumentBuilder()
                .parse(InputSource(StringReader(xml)))
        }.getOrNull() ?: return VesperDlnaSoapFault(null, null)
        val fault = document.descendantsByLocalName("Fault").firstOrNull() as? Element
        val code = fault?.childText("faultcode")
        val description = document.descendantsByLocalName("errorDescription")
            .firstOrNull()
            ?.textContent
            ?.trim()
        return VesperDlnaSoapFault(code = code, description = description)
    }
}

object VesperDlnaProtocolInfoParser {
    fun supportsHls(protocolInfo: String): Boolean =
        protocolInfo.split(',')
            .any {
                val entry = it.lowercase(Locale.US)
                entry.contains("application/vnd.apple.mpegurl") ||
                    entry.contains("application/x-mpegurl") ||
                    entry.contains("mpegurl") ||
                    entry.contains(".m3u8")
            }

    fun supportsDash(protocolInfo: String): Boolean =
        supportsMime(protocolInfo, "application/dash+xml")

    fun supportsMpegTs(protocolInfo: String): Boolean =
        protocolInfo.split(',')
            .any {
                val entry = it.lowercase(Locale.US)
                entry.contains("video/mp2t") ||
                    entry.contains("video/mpeg-ts") ||
                    entry.contains("video/mpeg") ||
                    entry.contains("mpegts") ||
                    entry.contains("mpeg_ts") ||
                    entry.contains("mp2t") ||
                    entry.contains("dlna.org_pn=mpeg_ts")
            }

    fun supportsMime(protocolInfo: String, mimeType: String): Boolean =
        protocolInfo.split(',').any { it.contains(mimeType, ignoreCase = true) }
}

class VesperDlnaSoapClient(
    private val timeoutMs: Int = 8_000,
) {
    fun setAvTransportUri(
        device: VesperDlnaDevice,
        source: VesperPlayerSource,
        metadata: VesperSystemPlaybackMetadata?,
    ): VesperDlnaSoapResponse =
        avTransport(device, "SetAVTransportURI", avTransportUriArguments(source, metadata))

    suspend fun setAvTransportUriAsync(
        device: VesperDlnaDevice,
        source: VesperPlayerSource,
        metadata: VesperSystemPlaybackMetadata?,
    ): VesperDlnaSoapResponse =
        avTransportAsync(device, "SetAVTransportURI", avTransportUriArguments(source, metadata))

    fun play(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransport(device, "Play", mapOf("InstanceID" to "0", "Speed" to "1"))

    suspend fun playAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransportAsync(device, "Play", mapOf("InstanceID" to "0", "Speed" to "1"))

    fun pause(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransport(device, "Pause", mapOf("InstanceID" to "0"))

    suspend fun pauseAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransportAsync(device, "Pause", mapOf("InstanceID" to "0"))

    fun stop(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransport(device, "Stop", mapOf("InstanceID" to "0"))

    suspend fun stopAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransportAsync(device, "Stop", mapOf("InstanceID" to "0"))

    fun seek(device: VesperDlnaDevice, positionMs: Long): VesperDlnaSoapResponse =
        avTransport(
            device,
            "Seek",
            mapOf(
                "InstanceID" to "0",
                "Unit" to "REL_TIME",
                "Target" to positionMs.toDlnaTime(),
            ),
        )

    suspend fun seekAsync(device: VesperDlnaDevice, positionMs: Long): VesperDlnaSoapResponse =
        avTransportAsync(
            device,
            "Seek",
            mapOf(
                "InstanceID" to "0",
                "Unit" to "REL_TIME",
                "Target" to positionMs.toDlnaTime(),
            ),
        )

    fun getPositionInfo(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransport(device, "GetPositionInfo", mapOf("InstanceID" to "0"))

    suspend fun getPositionInfoAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransportAsync(device, "GetPositionInfo", mapOf("InstanceID" to "0"))

    fun getTransportInfo(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransport(device, "GetTransportInfo", mapOf("InstanceID" to "0"))

    suspend fun getTransportInfoAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        avTransportAsync(device, "GetTransportInfo", mapOf("InstanceID" to "0"))

    fun getProtocolInfo(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        connectionManager(device, "GetProtocolInfo", emptyMap())

    suspend fun getProtocolInfoAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        connectionManagerAsync(device, "GetProtocolInfo", emptyMap())

    fun getVolume(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        renderingControl(device, "GetVolume", renderingControlArguments())

    suspend fun getVolumeAsync(device: VesperDlnaDevice): VesperDlnaSoapResponse =
        renderingControlAsync(device, "GetVolume", renderingControlArguments())

    fun setVolume(device: VesperDlnaDevice, volume: Int): VesperDlnaSoapResponse =
        renderingControl(device, "SetVolume", renderingControlArguments(volume))

    suspend fun setVolumeAsync(device: VesperDlnaDevice, volume: Int): VesperDlnaSoapResponse =
        renderingControlAsync(device, "SetVolume", renderingControlArguments(volume))

    private fun avTransport(
        device: VesperDlnaDevice,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val service = device.avTransport ?: return missingService("AVTransport")
        return post(device, service, action, arguments)
    }

    private suspend fun avTransportAsync(
        device: VesperDlnaDevice,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val service = device.avTransport ?: return missingService("AVTransport")
        return postAsync(device, service, action, arguments)
    }

    private fun connectionManager(
        device: VesperDlnaDevice,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val service = device.connectionManager ?: return missingService("ConnectionManager")
        return post(device, service, action, arguments)
    }

    private suspend fun connectionManagerAsync(
        device: VesperDlnaDevice,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val service = device.connectionManager ?: return missingService("ConnectionManager")
        return postAsync(device, service, action, arguments)
    }

    private fun renderingControl(
        device: VesperDlnaDevice,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val service = device.renderingControl ?: return missingService("RenderingControl")
        return post(device, service, action, arguments)
    }

    private suspend fun renderingControlAsync(
        device: VesperDlnaDevice,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val service = device.renderingControl ?: return missingService("RenderingControl")
        return postAsync(device, service, action, arguments)
    }

    private fun post(
        device: VesperDlnaDevice,
        service: VesperDlnaService,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse =
        mapPostFailure(service, action) {
            runBlocking(Dispatchers.IO) {
                postBlocking(device, service, action, arguments)
            }
        }

    private suspend fun postAsync(
        device: VesperDlnaDevice,
        service: VesperDlnaService,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse =
        mapPostFailure(service, action) {
            withContext(Dispatchers.IO) {
                postBlocking(device, service, action, arguments)
            }
        }

    private inline fun mapPostFailure(
        service: VesperDlnaService,
        action: String,
        block: () -> VesperDlnaSoapResponse,
    ): VesperDlnaSoapResponse =
        runCatching {
            block()
        }.getOrElse { error ->
            VesperDlnaSoapResponse(
                status = 0,
                body = error.toDlnaControlFailureMessage(action, service.controlUrl.toString()),
            )
        }

    private fun postBlocking(
        device: VesperDlnaDevice,
        service: VesperDlnaService,
        action: String,
        arguments: Map<String, String>,
    ): VesperDlnaSoapResponse {
        val body = VesperDlnaSoapEnvelopeBuilder.build(action, service.serviceType, arguments)
        val bodyBytes = body.toByteArray(Charsets.UTF_8)
        val connection = openConnection(service.controlUrl, device) as HttpURLConnection
        connection.connectTimeout = timeoutMs
        connection.readTimeout = timeoutMs
        connection.requestMethod = "POST"
        connection.doOutput = true
        connection.setFixedLengthStreamingMode(bodyBytes.size)
        connection.setRequestProperty("Content-Type", "text/xml; charset=\"utf-8\"")
        connection.setRequestProperty("SOAPACTION", "\"${service.serviceType}#$action\"")
        connection.setRequestProperty("Connection", "close")
        try {
            connection.outputStream.use { it.write(bodyBytes) }
            val status = connection.responseCode
            val responseBody = runCatching { connection.inputStream }
                .getOrElse { connection.errorStream }
                ?.bufferedReader()
                ?.use { it.readText() }
                .orEmpty()
            return VesperDlnaSoapResponse(status = status, body = responseBody)
        } finally {
            connection.disconnect()
        }
    }

    private fun openConnection(url: URL, device: VesperDlnaDevice): URLConnection =
        device.network?.openConnection(url) ?: url.openConnection()

    private fun missingService(name: String): VesperDlnaSoapResponse =
        VesperDlnaSoapResponse(0, "$name service is not available.")
}

private fun avTransportUriArguments(
    source: VesperPlayerSource,
    metadata: VesperSystemPlaybackMetadata?,
): Map<String, String> =
    mapOf(
        "InstanceID" to "0",
        "CurrentURI" to source.uri,
        "CurrentURIMetaData" to VesperDlnaDidlBuilder.build(source, metadata),
    )

private fun renderingControlArguments(volume: Int? = null): Map<String, String> =
    buildMap {
        put("InstanceID", "0")
        put("Channel", "Master")
        volume?.let { put("DesiredVolume", it.coerceIn(0, 100).toString()) }
    }

private fun Throwable.toDlnaControlFailureMessage(action: String, controlUrl: String): String {
    val cause = message?.takeIf { it.isNotBlank() } ?: javaClass.simpleName
    return "DLNA $action request failed for $controlUrl: $cause"
}

private fun Long.toDlnaTime(): String {
    val totalSeconds = (coerceAtLeast(0L) / 1000L)
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return "%d:%02d:%02d".format(hours, minutes, seconds)
}
