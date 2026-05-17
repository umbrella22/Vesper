package io.github.ikaros.vesper.player.android.external.internal.dlna

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLConnection
import java.net.URLStreamHandler
import kotlinx.coroutines.runBlocking
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
    fun appliesMinimumSsdpRouteTtl() {
        val raw = """
            HTTP/1.1 200 OK
            CACHE-CONTROL: max-age=1
            LOCATION: http://192.168.1.10:8000/desc.xml
            ST: urn:schemas-upnp-org:device:MediaRenderer:1
            USN: uuid:device-1::urn:schemas-upnp-org:device:MediaRenderer:1
        """.trimIndent()

        val request = VesperSsdpParser.parse(raw)!!.toDescriptionRequest(nowMillis = 1000L)

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
    fun postsSoapBodyWithFixedLengthAndCloseConnection() {
        val connection = RecordingSoapConnection(URL("http://192.168.1.10/control/av"))
        val controlUrl = URL(null, "http://192.168.1.10/control/av", RecordingUrlHandler(connection))
        val device = VesperDlnaDevice(
            routeId = "uuid:device-1",
            location = URL("http://192.168.1.10/description.xml"),
            usn = "uuid:device-1",
            friendlyName = "Living Room TV",
            avTransport = VesperDlnaService(
                serviceType = "urn:schemas-upnp-org:service:AVTransport:1",
                serviceId = "urn:upnp-org:serviceId:AVTransport",
                controlUrl = controlUrl,
                eventSubUrl = null,
                scpdUrl = null,
            ),
        )

        val response = VesperDlnaSoapClient(timeoutMs = 100).setAvTransportUri(
            device = device,
            source = VesperPlayerSource.remote(
                uri = "http://192.168.1.2:9000/media.mp4",
                label = "Episode",
            ),
            metadata = null,
        )

        assertEquals(200, response.status)
        assertEquals("POST", connection.requestMethod)
        assertEquals("close", connection.requestProperties["Connection"])
        assertEquals("text/xml; charset=\"utf-8\"", connection.requestProperties["Content-Type"])
        assertEquals(
            "\"urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI\"",
            connection.requestProperties["SOAPACTION"],
        )
        assertEquals(connection.fixedLength, connection.postedBody.size)
        assertTrue(connection.postedBodyText.contains("<u:SetAVTransportURI"))
        assertTrue(connection.disconnected)
    }

    @Test
    fun postsSoapBodyAsyncWithFixedLengthAndCloseConnection() = runBlocking {
        val connection = RecordingSoapConnection(URL("http://192.168.1.10/control/av"))
        val controlUrl = URL(null, "http://192.168.1.10/control/av", RecordingUrlHandler(connection))
        val device = VesperDlnaDevice(
            routeId = "uuid:device-1",
            location = URL("http://192.168.1.10/description.xml"),
            usn = "uuid:device-1",
            friendlyName = "Living Room TV",
            avTransport = VesperDlnaService(
                serviceType = "urn:schemas-upnp-org:service:AVTransport:1",
                serviceId = "urn:upnp-org:serviceId:AVTransport",
                controlUrl = controlUrl,
                eventSubUrl = null,
                scpdUrl = null,
            ),
        )

        val response = VesperDlnaSoapClient(timeoutMs = 100).setAvTransportUriAsync(
            device = device,
            source = VesperPlayerSource.remote(
                uri = "http://192.168.1.2:9000/media.mp4",
                label = "Episode",
            ),
            metadata = null,
        )

        assertEquals(200, response.status)
        assertEquals("POST", connection.requestMethod)
        assertEquals("close", connection.requestProperties["Connection"])
        assertEquals("text/xml; charset=\"utf-8\"", connection.requestProperties["Content-Type"])
        assertEquals(
            "\"urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI\"",
            connection.requestProperties["SOAPACTION"],
        )
        assertEquals(connection.fixedLength, connection.postedBody.size)
        assertTrue(connection.postedBodyText.contains("<u:SetAVTransportURI"))
        assertTrue(connection.disconnected)
    }

    @Test
    fun reportsMissingDlnaServicesConsistentlyForSyncAndAsyncCalls() = runBlocking {
        val device = VesperDlnaDevice(
            routeId = "uuid:device-1",
            location = URL("http://192.168.1.10/description.xml"),
            usn = "uuid:device-1",
            friendlyName = "Living Room TV",
        )
        val client = VesperDlnaSoapClient(timeoutMs = 100)

        val syncAvTransport = client.play(device)
        val asyncAvTransport = client.playAsync(device)
        val syncConnectionManager = client.getProtocolInfo(device)
        val asyncConnectionManager = client.getProtocolInfoAsync(device)
        val syncRenderingControl = client.getVolume(device)
        val asyncRenderingControl = client.getVolumeAsync(device)

        assertEquals(0, syncAvTransport.status)
        assertEquals(syncAvTransport, asyncAvTransport)
        assertEquals("AVTransport service is not available.", syncAvTransport.body)
        assertEquals(0, syncConnectionManager.status)
        assertEquals(syncConnectionManager, asyncConnectionManager)
        assertEquals("ConnectionManager service is not available.", syncConnectionManager.body)
        assertEquals(0, syncRenderingControl.status)
        assertEquals(syncRenderingControl, asyncRenderingControl)
        assertEquals("RenderingControl service is not available.", syncRenderingControl.body)
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
        assertTrue(
            VesperDlnaProtocolInfoParser.supportsHls(
                "http-get:*:APPLICATION/X-MPEGURL:*,http-get:*:video/mp4:*",
            ),
        )
        assertTrue(
            VesperDlnaProtocolInfoParser.supportsHls(
                "http-get:*:video/mp4:DLNA.ORG_PN=HLS;URI=.m3u8",
            ),
        )
        assertFalse(VesperDlnaProtocolInfoParser.supportsHls("http-get:*:video/mp4:*"))
    }

    @Test
    fun parsesProtocolInfoForDashAndMpegTsSupport() {
        val protocolInfo = "http-get:*:application/dash+xml:*,http-get:*:video/mp2t:*"

        assertTrue(VesperDlnaProtocolInfoParser.supportsDash(protocolInfo))
        assertTrue(VesperDlnaProtocolInfoParser.supportsMpegTs(protocolInfo))
        assertTrue(VesperDlnaProtocolInfoParser.supportsMpegTs("http-get:*:video/mpeg-ts:*"))
        assertTrue(VesperDlnaProtocolInfoParser.supportsMpegTs("http-get:*:video/MP2T:*"))
        assertTrue(
            VesperDlnaProtocolInfoParser.supportsMpegTs(
                "http-get:*:video/mpeg:DLNA.ORG_PN=MPEG_TS_HD_NA",
            ),
        )
        assertFalse(VesperDlnaProtocolInfoParser.supportsDash("http-get:*:video/mp4:*"))
    }

    @Test
    fun buildsHlsDidlWithDlnaCompatibleMime() {
        val didl = VesperDlnaDidlBuilder.build(
            VesperPlayerSource.hls(
                uri = "http://192.168.1.2:9000/media/token/playlist.m3u8",
                label = "Episode",
            ),
            null,
        )

        assertTrue(didl.contains("protocolInfo=\"http-get:*:application/x-mpegURL:"))
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
    fun normalizesDlnaRouteIdentityAliases() {
        assertEquals(
            "dlna-debug-renderer-001",
            dlnaRouteIdentityKey(
                "uuid:dlna-debug-renderer-001::urn:schemas-upnp-org:device:MediaRenderer:1",
            ),
        )
        assertEquals("dlna-debug-renderer-001", dlnaRouteIdentityKey("dlna-debug-renderer-001"))
        assertEquals("device-1", dlnaRouteIdentityKey("URN:UUID:DEVICE-1"))
    }

    @Test
    fun matchesDlnaDeviceRouteAliases() {
        val device = VesperDlnaDevice(
            routeId = "uuid:dlna-debug-renderer-001",
            location = URL("http://192.168.1.10:8000/desc.xml"),
            usn = "uuid:dlna-debug-renderer-001::urn:schemas-upnp-org:device:MediaRenderer:1",
            friendlyName = "Debug Renderer",
            udn = "uuid:dlna-debug-renderer-001",
        )

        assertTrue(device.matchesRouteId("uuid:dlna-debug-renderer-001"))
        assertTrue(device.matchesRouteId("dlna-debug-renderer-001"))
        assertTrue(
            device.matchesRouteId(
                "UUID:DLNA-DEBUG-RENDERER-001::urn:schemas-upnp-org:device:MediaRenderer:1",
            ),
        )
        assertFalse(device.matchesRouteId("uuid:another-renderer"))
    }

    @Test
    fun matchesKnownDeviceDescriptionRequests() {
        val device = VesperDlnaDevice(
            routeId = "uuid:dlna-debug-renderer-001",
            location = URL("http://192.168.1.10:8000/description.xml"),
            usn = "uuid:dlna-debug-renderer-001::urn:schemas-upnp-org:device:MediaRenderer:1",
            friendlyName = "Debug Renderer",
            udn = "uuid:dlna-debug-renderer-001",
        )
        val sameLocation = VesperDlnaDescriptionRequest(
            location = URL("http://192.168.1.10:8000/description.xml"),
            usn = "uuid:other-response::upnp:rootdevice",
            expiresAtMillis = 42L,
        )
        val sameIdentity = VesperDlnaDescriptionRequest(
            location = URL("http://192.168.1.10:8000/other.xml"),
            usn = "UUID:DLNA-DEBUG-RENDERER-001::upnp:rootdevice",
            expiresAtMillis = 42L,
        )
        val different = VesperDlnaDescriptionRequest(
            location = URL("http://192.168.1.11:8000/description.xml"),
            usn = "uuid:another-renderer::upnp:rootdevice",
            expiresAtMillis = 42L,
        )

        assertTrue(device.matchesDescriptionRequest(sameLocation))
        assertTrue(device.matchesDescriptionRequest(sameIdentity))
        assertFalse(device.matchesDescriptionRequest(different))
    }

    @Test
    fun descriptionFetchKeyIncludesLocationAndIdentity() {
        val request = VesperDlnaDescriptionRequest(
            location = URL("http://192.168.1.10:8000/description.xml"),
            usn = "UUID:DLNA-DEBUG-RENDERER-001::upnp:rootdevice",
            expiresAtMillis = 42L,
        )

        assertEquals(
            "http://192.168.1.10:8000/description.xml|dlna-debug-renderer-001",
            request.descriptionFetchKey(),
        )
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

private class RecordingUrlHandler(
    private val connection: URLConnection,
) : URLStreamHandler() {
    override fun openConnection(url: URL): URLConnection = connection
}

private class RecordingSoapConnection(url: URL) : HttpURLConnection(url) {
    val requestProperties = linkedMapOf<String, String>()
    val output = ByteArrayOutputStream()
    var fixedLength: Int = -1
    var disconnected: Boolean = false

    val postedBody: ByteArray
        get() = output.toByteArray()

    val postedBodyText: String
        get() = postedBody.toString(Charsets.UTF_8)

    override fun disconnect() {
        disconnected = true
    }

    override fun usingProxy(): Boolean = false

    override fun connect() {
        connected = true
    }

    override fun setFixedLengthStreamingMode(contentLength: Int) {
        fixedLength = contentLength
    }

    override fun setRequestProperty(key: String, value: String) {
        requestProperties[key] = value
    }

    override fun getOutputStream(): ByteArrayOutputStream = output

    override fun getResponseCode(): Int = HTTP_OK

    override fun getInputStream(): InputStream =
        "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body /></s:Envelope>"
            .byteInputStream()
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
