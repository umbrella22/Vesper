package io.github.ikaros.vesper.player.android.external.internal.dlna

import java.io.StringReader
import java.net.URL
import javax.xml.parsers.DocumentBuilder
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Element
import org.w3c.dom.Node
import org.xml.sax.InputSource

object VesperDlnaDeviceDescriptionParser {
    fun parse(
        xml: String,
        location: URL,
        usn: String,
        expiresAtMillis: Long = Long.MAX_VALUE,
    ): VesperDlnaDevice {
        val document = secureDocumentBuilder()
            .parse(InputSource(StringReader(xml)))
        val base = document.documentElement
            ?.childText("URLBase")
            ?.let { runCatching { URL(location, it) }.getOrNull() }
            ?: location
        val device = document.descendantsByLocalName("device")
            .mapNotNull { it as? Element }
            .map { candidate ->
                val services = candidate.childElementsByLocalName("serviceList")
                    .flatMap { it.childElementsByLocalName("service") }
                    .mapNotNull { it.toService(base) }
                    .toList()
                candidate to services
            }
            .firstOrNull { (candidate, services) ->
                candidate.isMediaRendererDevice() ||
                    services.any { it.serviceType.isAvTransportService() }
            }
            ?: throw IllegalArgumentException(
                "DLNA device description does not contain a MediaRenderer device.",
            )
        val (deviceElement, services) = device
        val udn = deviceElement.childText("UDN")
        val deviceType = deviceElement.childText("deviceType")
        return VesperDlnaDevice(
            routeId = canonicalDlnaRouteId(udn ?: usn),
            location = location,
            usn = usn,
            friendlyName = deviceElement.childText("friendlyName") ?: location.host,
            manufacturer = deviceElement.childText("manufacturer"),
            modelName = deviceElement.childText("modelName"),
            udn = udn,
            deviceType = deviceType,
            avTransport = services.firstOrNull { it.serviceType.isAvTransportService() },
            renderingControl = services.firstOrNull { it.serviceType.isRenderingControlService() },
            connectionManager = services.firstOrNull { it.serviceType.isConnectionManagerService() },
            expiresAtMillis = expiresAtMillis,
        )
    }

    private fun Element.toService(base: URL): VesperDlnaService? {
        val serviceType = childText("serviceType") ?: return null
        val serviceId = childText("serviceId") ?: serviceType
        val controlUrl = childText("controlURL")?.let { URL(base, it) } ?: return null
        return VesperDlnaService(
            serviceType = serviceType,
            serviceId = serviceId,
            controlUrl = controlUrl,
            eventSubUrl = childText("eventSubURL")?.let { URL(base, it) },
            scpdUrl = childText("SCPDURL")?.let { URL(base, it) },
        )
    }

    private fun Element.isMediaRendererDevice(): Boolean =
        childText("deviceType")?.contains("MediaRenderer", ignoreCase = true) == true
}

internal fun secureDocumentBuilderFactory(): DocumentBuilderFactory =
    DocumentBuilderFactory.newInstance().apply {
        isNamespaceAware = true
        runCatching { setFeature("http://xml.org/sax/features/external-general-entities", false) }
        runCatching { setFeature("http://xml.org/sax/features/external-parameter-entities", false) }
        runCatching { setFeature("http://apache.org/xml/features/nonvalidating/load-external-dtd", false) }
        runCatching { setAttribute(XML_ACCESS_EXTERNAL_DTD, "") }
        runCatching { setAttribute(XML_ACCESS_EXTERNAL_SCHEMA, "") }
        runCatching { isXIncludeAware = false }
        isExpandEntityReferences = false
    }

internal fun secureDocumentBuilder(): DocumentBuilder =
    secureDocumentBuilderFactory()
        .newDocumentBuilder()
        .apply {
            setEntityResolver { _, _ -> InputSource(StringReader("")) }
        }

internal fun Element.childText(localName: String): String? =
    childElementsByLocalName(localName)
        .firstOrNull()
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }

internal fun Element.childElementsByLocalName(localName: String): Sequence<Element> =
    childNodes.asSequence()
        .filterIsInstance<Element>()
        .filter { it.localName == localName || it.nodeName == localName }

internal fun Node.descendantsByLocalName(localName: String): Sequence<Node> =
    sequence {
        val nodes = childNodes
        for (index in 0 until nodes.length) {
            val child = nodes.item(index)
            if (child.localName == localName || child.nodeName == localName) {
                yield(child)
            }
            yieldAll(child.descendantsByLocalName(localName))
        }
    }

internal fun org.w3c.dom.NodeList.asSequence(): Sequence<Node> =
    sequence {
        for (index in 0 until length) {
            yield(item(index))
        }
    }

private const val XML_ACCESS_EXTERNAL_DTD = "http://javax.xml.XMLConstants/property/accessExternalDTD"
private const val XML_ACCESS_EXTERNAL_SCHEMA =
    "http://javax.xml.XMLConstants/property/accessExternalSchema"
