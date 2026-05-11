package io.github.ikaros.vesper.player.android.dlna

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import java.net.URL
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperDlnaProtocolTest {
    @Test
    fun parsesSsdpResponseHeadersAndMaxAge() {
        val raw = """
            HTTP/1.1 200 OK
            CACHE-CONTROL: max-age=120
            LOCATION: http://192.168.1.10:8000/desc.xml
            ST: urn:schemas-upnp-org:device:MediaRenderer:1
            USN: uuid:device-1::urn:schemas-upnp-org:device:MediaRenderer:1
            SERVER: Linux/5.0 UPnP/1.0 Demo/1.0
        """.trimIndent()

        val message = VesperSsdpParser.parse(raw)

        assertNotNull(message)
        assertEquals("http://192.168.1.10:8000/desc.xml", message?.location)
        assertEquals(120L, message?.cacheMaxAgeSeconds)
        assertTrue(message?.isMediaRenderer == true)
        val request = message!!.toDescriptionRequest(nowMillis = 1000L)
        assertEquals(URL("http://192.168.1.10:8000/desc.xml"), request?.location)
        assertEquals(121000L, request?.expiresAtMillis)
    }

    @Test
    fun parsesDeviceDescriptionServices() {
        val device = VesperDlnaDeviceDescriptionParser.parse(
            xml = DEVICE_XML,
            location = URL("http://192.168.1.10:8000/root/desc.xml"),
            usn = "uuid:device-1",
            expiresAtMillis = 42L,
        )

        assertEquals("Living Room TV", device.friendlyName)
        assertEquals("DemoCorp", device.manufacturer)
        assertEquals("Model X", device.modelName)
        assertEquals(URL("http://192.168.1.10:8000/upnp/control/av"), device.avTransport?.controlUrl)
        assertEquals(URL("http://192.168.1.10:8000/upnp/control/cm"), device.connectionManager?.controlUrl)
        assertEquals(42L, device.expiresAtMillis)
        assertTrue(device.supportsPlayback)
    }

    @Test
    fun buildsSoapEnvelopeAndEscapesArguments() {
        val envelope = VesperDlnaSoapEnvelopeBuilder.build(
            action = "SetAVTransportURI",
            serviceType = "urn:schemas-upnp-org:service:AVTransport:1",
            arguments = mapOf(
                "InstanceID" to "0",
                "CurrentURI" to "https://example.com/a?b=1&c=2",
            ),
        )

        assertTrue(envelope.contains("<u:SetAVTransportURI"))
        assertTrue(envelope.contains("https://example.com/a?b=1&amp;c=2"))
    }

    @Test
    fun parsesSoapFault() {
        val fault = VesperDlnaSoapFaultParser.parse(
            """
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
              <s:Body>
                <s:Fault>
                  <faultcode>s:Client</faultcode>
                  <detail>
                    <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
                      <errorCode>701</errorCode>
                      <errorDescription>Transition not available</errorDescription>
                    </UPnPError>
                  </detail>
                </s:Fault>
              </s:Body>
            </s:Envelope>
            """.trimIndent(),
        )

        assertEquals("s:Client", fault?.code)
        assertEquals("Transition not available", fault?.description)
    }

    @Test
    fun parsesProtocolInfoForHlsSupport() {
        assertTrue(
            VesperDlnaProtocolInfoParser.supportsHls(
                "http-get:*:video/mp4:*,http-get:*:application/vnd.apple.mpegurl:*",
            ),
        )
        assertFalse(VesperDlnaProtocolInfoParser.supportsHls("http-get:*:video/mp4:*"))
    }

    @Test
    fun buildsDidlLiteMetadata() {
        val source = VesperPlayerSource.remote(
            uri = "http://192.168.1.2:9000/media/token",
            label = "Episode <1>",
        )
        val didl = VesperDlnaDidlBuilder.build(
            source,
            VesperSystemPlaybackMetadata(
                title = "Episode & Finale",
                artworkUri = "https://example.com/art.jpg",
                durationMs = 65_000,
            ),
        )

        assertTrue(didl.contains("<dc:title>Episode &amp; Finale</dc:title>"))
        assertTrue(didl.contains("object.item.videoItem.movie"))
        assertTrue(didl.contains("protocolInfo=\"http-get:*:video/mp4:*\""))
        assertTrue(didl.contains("duration=\"0:01:05\""))
    }
}

private val DEVICE_XML = """
    <?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <friendlyName>Living Room TV</friendlyName>
        <manufacturer>DemoCorp</manufacturer>
        <modelName>Model X</modelName>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
            <controlURL>/upnp/control/av</controlURL>
            <eventSubURL>/upnp/event/av</eventSubURL>
            <SCPDURL>/upnp/av.xml</SCPDURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId>
            <controlURL>/upnp/control/cm</controlURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>
            <controlURL>/upnp/control/rc</controlURL>
          </service>
        </serviceList>
      </device>
    </root>
""".trimIndent()
