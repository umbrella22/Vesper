package io.github.umbrella22.vesper.player.android

import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import com.sun.net.httpserver.HttpServer
import java.io.ByteArrayOutputStream
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], manifest = Config.NONE)
class VesperDashStartupDataSourceTest {
    @Test fun formalFactoryKeepsSourcePreloadIndependentOfControllerCachePolicy() = runBlocking {
        val count = AtomicInteger()
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        val root = "http://127.0.0.1:${server.address.port}"
        val manifest = """<MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v" codecs="avc1.640028"><BaseURL>video.mp4</BaseURL><SegmentBase indexRange="10-53"><Initialization range="0-9"/></SegmentBase></Representation></AdaptationSet></Period></MPD>""".toByteArray()
        val index = ByteBuffer.allocate(44).putInt(44).putInt(0x73696478).putInt(0).putInt(1)
            .putInt(1000).putInt(0).putInt(0).putShort(0).putShort(1).putInt(20).putInt(1000).putInt(0).array()
        val video = ByteArray(10) { it.toByte() } + index + ByteArray(40) { (it + 54).toByte() }
        server.createContext("/") { exchange ->
            count.incrementAndGet()
            val full = if (exchange.requestURI.path.endsWith(".mpd")) manifest else video
            val range = exchange.requestHeaders.getFirst("Range")?.removePrefix("bytes=")?.split('-')
            val data = if (range != null) {
                val first = range[0].toInt()
                val last = range[1].takeIf(String::isNotEmpty)?.toInt() ?: full.lastIndex
                exchange.responseHeaders.add("Content-Range", "bytes $first-$last/${full.size}")
                full.copyOfRange(first, last + 1)
            } else full
            exchange.sendResponseHeaders(if (range == null) 200 else 206, data.size.toLong())
            exchange.responseBody.use { it.write(data) }
        }
        server.start()
        try {
            val scope = DashStartupScope()
            val headers = mapOf("Authorization" to "Bearer fixture")
            val source = VesperPlayerSource.remote("$root/manifest.mpd", label = "test", headers = headers)
            warmDashStartup(source, scope, 2000)
            assertEquals(4, count.get())
            val context = RuntimeEnvironment.getApplication()
            val enabled = NativeCachePolicy(0, true, 1024 * 1024, true, 1024 * 1024)
            val disabled = NativeCachePolicy(0, true, 0, true, 0)
            val factory = buildDataSourceFactory(context, enabled, headers, scope)
            assertArrayEquals(manifest, read(factory, "$root/manifest.mpd"))
            assertArrayEquals(video.copyOfRange(0, 54), read(factory, "$root/video.mp4", 0, 54))
            assertArrayEquals(video.copyOfRange(54, 74), read(factory, "$root/video.mp4", 54, 20))
            assertEquals(4, count.get())
            assertArrayEquals(video.copyOfRange(74, 94), read(factory, "$root/video.mp4", 74, 20))
            assertEquals(5, count.get())
            read(factory, "$root/video.mp4", 74, 20)
            assertEquals("Unwarmed bytes must still use the original disk cache", 5, count.get())
            read(buildDataSourceFactory(context, disabled, headers, scope), "$root/video.mp4", 54, 20)
            assertEquals("Controller policy must preserve the source session's warm bytes", 5, count.get())
            read(buildDataSourceFactory(context, disabled, headers, scope), "$root/video.mp4", 74, 20)
            assertEquals("Disabled controller cache must still bypass the disk cache", 6, count.get())
            read(buildDataSourceFactory(context, enabled, mapOf("Authorization" to "Bearer different"), scope), "$root/video.mp4", 74, 20)
            assertEquals("Disk keys must isolate credentials too", 7, count.get())
        } finally { server.stop(0) }
    }

    private fun read(factory: DataSource.Factory, uri: String, position: Long = 0, length: Long = -1): ByteArray {
        val source = factory.createDataSource()
        try {
            source.open(DataSpec.Builder().setUri(uri).setPosition(position).setLength(length).build())
            assertNotNull(source.uri)
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(17)
            while (true) {
                val count = source.read(buffer, 0, buffer.size)
                if (count == -1) break
                output.write(buffer, 0, count)
            }
            return output.toByteArray()
        } finally { source.close() }
    }
}
