package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.ByteArrayOutputStream
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Device regression for the Media3 transport used by source-session preloads. */
@RunWith(AndroidJUnit4::class)
class VesperPlaybackSequenceWarmupInstrumentationTest {
    @Test
    fun media3TransportReadsOnlyRequestedRangeWithoutClaimingCacheReuse() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val fixture = File.createTempFile("vesper-warmup-transport-", ".bin", context.cacheDir)
        val bytes = ByteArray(256 * 1024) { index -> (index % 251).toByte() }
        fixture.writeBytes(bytes)
        try {
            val transport = VesperMedia3SequenceWarmupTransport(context)
            val request = VesperSequenceWarmupReadRequest(
                uri = Uri.fromFile(fixture).toString(),
                headers = emptyMap(),
                cacheKey = "device-transport-fixture",
                position = 137L,
                length = 64L * 1024L,
                timeoutMillis = 5_000L,
            )
            // Reopening the download-only transport must not invent a physical cache hit.
            repeat(2) {
                val stream = transport.open(request)
                try {
                    assertFalse(stream.cacheHit)
                    val received = ByteArrayOutputStream()
                    val buffer = ByteArray(8 * 1024)
                    while (true) {
                        val read = stream.read(buffer, 0, buffer.size)
                        if (read == -1) break
                        assertTrue("local transport must make progress", read > 0)
                        received.write(buffer, 0, read)
                        assertTrue("transport exceeded requested range", received.size() <= request.length)
                    }
                    assertEquals(request.length, received.size().toLong())
                    assertArrayEquals(bytes.copyOfRange(137, 137 + request.length.toInt()), received.toByteArray())
                } finally {
                    stream.close()
                }
            }
        } finally {
            fixture.delete()
        }
    }
}
