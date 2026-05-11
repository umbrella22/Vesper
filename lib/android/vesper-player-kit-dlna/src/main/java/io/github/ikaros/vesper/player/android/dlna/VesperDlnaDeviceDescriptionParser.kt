package io.github.ikaros.vesper.player.android.dlna

import java.io.StringReader
import java.net.URL
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
        val document = secureDocumentBuilderFactory()
            .newDocumentBuilder()
            .parse(InputSource(StringReader(xml)))
        val device = document.descendantsByLocalName("device").firstOrNull() as? Element
            ?: throw IllegalArgumentException("DLNA device description is missing a device element.")
        val services = device.descendantsByLocalName("service")
            .mapNotNull { it as? Element }
            .mapNotNull { it.toService(location) }
            .toList()
        return VesperDlnaDevice(
            routeId = usn,
            location = location,
            usn = usn,
            friendlyName = device.childText("friendlyName") ?: location.host,
            manufacturer = device.childText("manufacturer"),
            modelName = device.childText("modelName"),
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
}

internal fun secureDocumentBuilderFactory(): DocumentBuilderFactory =
    DocumentBuilderFactory.newInstance().apply {
        isNamespaceAware = true
        runCatching { setFeature("http://apache.org/xml/features/disallow-doctype-decl", true) }
        runCatching { setFeature("http://xml.org/sax/features/external-general-entities", false) }
        runCatching { setFeature("http://xml.org/sax/features/external-parameter-entities", false) }
        runCatching { setFeature("http://apache.org/xml/features/nonvalidating/load-external-dtd", false) }
        isExpandEntityReferences = false
    }

internal fun Element.childText(localName: String): String? =
    childNodes.asSequence()
        .filterIsInstance<Element>()
        .firstOrNull { it.localName == localName || it.nodeName == localName }
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }

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
