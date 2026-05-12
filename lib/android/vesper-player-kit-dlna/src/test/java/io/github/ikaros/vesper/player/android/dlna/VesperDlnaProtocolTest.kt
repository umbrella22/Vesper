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
    fun parsesDeviceDescriptionWithUpnpDoctype() {
        val device = VesperDlnaDeviceDescriptionParser.parse(
            xml = DEVICE_XML_WITH_DOCTYPE,
            location = URL("http://192.168.1.10:8000/desc.xml"),
            usn = "uuid:device-1::urn:schemas-upnp-org:device:MediaRenderer:1",
        )

        assertEquals("uuid:device-1", device.routeId)
        assertEquals("Living Room TV", device.friendlyName)
        assertTrue(device.supportsPlayback)
    }

    @Test
    fun blocksExternalEntityExpansionInDeviceDescription() {
        val device = VesperDlnaDeviceDescriptionParser.parse(
            xml = DEVICE_XML_WITH_EXTERNAL_ENTITY,
            location = URL("http://192.168.1.10:8000/desc.xml"),
            usn = "uuid:device-1",
        )

        assertEquals("192.168.1.10", device.friendlyName)
        assertFalse(device.friendlyName.contains("secret", ignoreCase = true))
        assertTrue(device.supportsPlayback)
    }

    @Test
    fun parsesEmbeddedRendererAndUrlBase() {
        val device = VesperDlnaDeviceDescriptionParser.parse(
            xml = EMBEDDED_RENDERER_XML,
            location = URL("http://192.168.1.10:8000/root/desc.xml"),
            usn = "uuid:root-device::upnp:rootdevice",
        )

        assertEquals("uuid:renderer-device", device.routeId)
        assertEquals("Bedroom TV", device.friendlyName)
        assertEquals(URL("http://192.168.1.10:9000/control/av"), device.avTransport?.controlUrl)
        assertEquals(URL("http://192.168.1.10:9000/event/av"), device.avTransport?.eventSubUrl)
        assertEquals(URL("http://192.168.1.10:9000/scpd/av.xml"), device.avTransport?.scpdUrl)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsDescriptionWithoutRendererOrAvTransport() {
        VesperDlnaDeviceDescriptionParser.parse(
            xml = MEDIA_SERVER_XML,
            location = URL("http://192.168.1.10:8000/desc.xml"),
            usn = "uuid:media-server::upnp:rootdevice",
        )
    }

    @Test
    fun parsesRendererWithoutAvTransportAsUnsupportedPlayback() {
        val device = VesperDlnaDeviceDescriptionParser.parse(
            xml = RENDERER_WITHOUT_AV_TRANSPORT_XML,
            location = URL("http://192.168.1.10:8000/desc.xml"),
            usn = "uuid:renderer::urn:schemas-upnp-org:device:MediaRenderer:1",
        )

        assertEquals("uuid:renderer", device.routeId)
        assertFalse(device.supportsPlayback)
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
        assertTrue(didl.contains("object.item.videoItem"))
        assertTrue(didl.contains("protocolInfo=\"http-get:*:video/mp4:DLNA.ORG_OP=01;DLNA.ORG_CI=0;"))
        assertTrue(didl.contains("DLNA.ORG_FLAGS=01500000000000000000000000000000"))
        assertTrue(didl.contains("duration=\"0:01:05\""))
    }

    @Test
    fun buildsDidlLiteMetadataForAudioAndImages() {
        val audio = VesperPlayerSource.remote(
            uri = "http://192.168.1.2:9000/media/token/song.flac",
            label = "Song",
        )
        val image = VesperPlayerSource.remote(
            uri = "http://192.168.1.2:9000/media/token/photo",
            label = "cover.jpg",
        )

        val audioDidl = VesperDlnaDidlBuilder.build(audio, null)
        val imageDidl = VesperDlnaDidlBuilder.build(image, null)

        assertTrue(audioDidl.contains("object.item.audioItem.musicTrack"))
        assertTrue(audioDidl.contains("protocolInfo=\"http-get:*:audio/flac:DLNA.ORG_OP=01;"))
        assertTrue(imageDidl.contains("object.item.imageItem.photo"))
        assertTrue(imageDidl.contains("protocolInfo=\"http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_SM;"))
    }

    @Test
    fun acceptsSsdpAllAndRootdeviceForDescriptionFetch() {
        val all = VesperSsdpParser.parse(
            """
            HTTP/1.1 200 OK
            CACHE-CONTROL: max-age=60
            LOCATION: http://192.168.1.10:8000/desc.xml
            ST: ssdp:all
            USN: uuid:device-1::upnp:rootdevice
            """.trimIndent(),
        )
        val root = VesperSsdpParser.parse(
            """
            NOTIFY * HTTP/1.1
            HOST: 239.255.255.250:1900
            CACHE-CONTROL: max-age=60
            LOCATION: http://192.168.1.10:8000/desc.xml
            NT: upnp:rootdevice
            NTS: ssdp:alive
            USN: uuid:device-1::upnp:rootdevice
            """.trimIndent(),
        )

        assertFalse(all!!.isMediaRenderer)
        assertTrue(all.shouldFetchDescription)
        assertFalse(root!!.isMediaRenderer)
        assertTrue(root.isAliveNotify)
        assertTrue(root.shouldFetchDescription)
    }

    @Test
    fun parsesMixedCaseSsdpHeadersAndCanonicalRouteId() {
        val message = VesperSsdpParser.parse(
            """
            HTTP/1.1 200 OK
            Cache-Control: max-age=90
            Location: http://192.168.1.10:8000/desc.xml
            St: urn:schemas-upnp-org:device:MediaRenderer:1
            Usn: uuid:device-1::urn:schemas-upnp-org:device:MediaRenderer:1
            """.trimIndent(),
        )

        assertEquals("http://192.168.1.10:8000/desc.xml", message?.location)
        assertEquals(90L, message?.cacheMaxAgeSeconds)
        assertTrue(message?.isMediaRenderer == true)
        assertEquals("uuid:device-1", canonicalDlnaRouteId(message!!.usn!!))
    }

    @Test
    fun parsesSsdpByebyeNotify() {
        val message = VesperSsdpParser.parse(
            """
            NOTIFY * HTTP/1.1
            HOST: 239.255.255.250:1900
            NT: urn:schemas-upnp-org:device:MediaRenderer:1
            NTS: ssdp:byebye
            USN: uuid:device-1::urn:schemas-upnp-org:device:MediaRenderer:1
            """.trimIndent(),
        )

        assertTrue(message?.isByebyeNotify == true)
        assertFalse(message!!.shouldFetchDescription)
    }
}

private val DEVICE_XML = """
    <?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <UDN>uuid:device-1</UDN>
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

private val DEVICE_XML_WITH_DOCTYPE = """
    <?xml version="1.0"?>
    <!DOCTYPE root PUBLIC "-//UPnP//DTD Device 1.0//EN" "http://www.upnp.org/xml/device-1-0.dtd">
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <UDN>uuid:device-1</UDN>
        <friendlyName>Living Room TV</friendlyName>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
            <controlURL>/upnp/control/av</controlURL>
          </service>
        </serviceList>
      </device>
    </root>
""".trimIndent()

private val DEVICE_XML_WITH_EXTERNAL_ENTITY = """
    <?xml version="1.0"?>
    <!DOCTYPE root [
      <!ENTITY xxe SYSTEM "file:///tmp/vesper-secret.txt">
    ]>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <UDN>uuid:device-1</UDN>
        <friendlyName>&xxe;</friendlyName>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
            <controlURL>/upnp/control/av</controlURL>
          </service>
        </serviceList>
      </device>
    </root>
""".trimIndent()

private val EMBEDDED_RENDERER_XML = """
    <?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <URLBase>http://192.168.1.10:9000/base/</URLBase>
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
        <UDN>uuid:root-device</UDN>
        <friendlyName>Root Device</friendlyName>
        <deviceList>
          <device>
            <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
            <UDN>uuid:renderer-device</UDN>
            <friendlyName>Bedroom TV</friendlyName>
            <serviceList>
              <service>
                <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
                <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
                <controlURL>/control/av</controlURL>
                <eventSubURL>/event/av</eventSubURL>
                <SCPDURL>/scpd/av.xml</SCPDURL>
              </service>
            </serviceList>
          </device>
        </deviceList>
      </device>
    </root>
""".trimIndent()

private val MEDIA_SERVER_XML = """
    <?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
        <UDN>uuid:media-server</UDN>
        <friendlyName>NAS</friendlyName>
      </device>
    </root>
""".trimIndent()

private val RENDERER_WITHOUT_AV_TRANSPORT_XML = """
    <?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <UDN>uuid:renderer</UDN>
        <friendlyName>Limited TV</friendlyName>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>
            <controlURL>/upnp/control/rc</controlURL>
          </service>
        </serviceList>
      </device>
    </root>
""".trimIndent()
