package io.github.umbrella22.vesper.player.android

import java.nio.ByteBuffer
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class VesperDashStartupTest {
    private val uri = "https://example.test/manifest.mpd?token=one"
    private val manifest = """<MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="100" codecs="avc1.640028"><BaseURL>v.mp4</BaseURL><SegmentBase indexRange="10-53"><Initialization range="0-9"/></SegmentBase></Representation></AdaptationSet></Period></MPD>""".toByteArray()
    private fun sidx() = ByteBuffer.allocate(44).putInt(44).putInt(0x73696478).putInt(0).putInt(1)
        .putInt(1000).putInt(0).putInt(0).putShort(0).putShort(1).putInt(20).putInt(1000).putInt(0).array()

    @Test fun warmupCommitsAllResourcesAndFormalReadsReuseExactBytes() = runBlocking {
        var now = 100L
        val cache = VesperDashStartupCache(nowMs = { now })
        val scope = DashStartupScope(ttlMs = 30)
        val source = VesperPlayerSource.remote(uri, label = "test", headers = mapOf("Authorization" to "Bearer one"))
        var requests = 0
        val transport = DashStartupTransport { resource, _, _, _ ->
            requests++
            DashStartupBytes(resource, when {
                resource.uri == uri -> manifest
                resource.position == 10L -> sidx()
                resource.position == 0L -> ByteArray(10) { it.toByte() }
                else -> ByteArray(20) { (it + 54).toByte() }
            })
        }
        warmDashStartup(source, scope, 1000, transport, cache)
        assertEquals(4, requests)
        val media = "https://example.test/v.mp4"
        assertArrayEquals(manifest, cache.read(scope, DashStartupResource(uri), source.headers)?.bytes)
        // Media3 may merge adjacent initialization and index ranges.
        assertArrayEquals(ByteArray(10) { it.toByte() } + sidx(), cache.read(scope, DashStartupResource(media, 0, 54), source.headers)?.bytes)
        assertArrayEquals(ByteArray(20) { (it + 54).toByte() }, cache.read(scope, DashStartupResource(media, 54, 20), source.headers)?.bytes)
        assertTrue(warmDashStartup(source, scope, 1000, transport, cache).second)
        assertEquals(4, requests)
        assertNull(cache.read(scope.copy(namespace = "new-revision"), DashStartupResource(uri), source.headers))
        assertNull(cache.read(scope, DashStartupResource(uri), mapOf("Authorization" to "Bearer two")))
        now += 31
        assertNull(cache.read(scope, DashStartupResource(uri), source.headers))
        warmDashStartup(source, scope, 1000, transport, cache)
        assertEquals(8, requests)
        cache.clear()
        assertNull(cache.read(scope, DashStartupResource(uri), source.headers))
    }

    @Test fun partialFailureDoesNotCommitStartupBytes() = runBlocking {
        val cache = VesperDashStartupCache()
        val scope = DashStartupScope()
        val source = VesperPlayerSource.remote(uri, label = "test")
        try {
            warmDashStartup(source, scope, 1000, DashStartupTransport { resource, _, _, _ ->
                if (resource.uri != uri) error("failed index")
                DashStartupBytes(resource, manifest)
            }, cache)
            fail("warmup must fail")
        } catch (_: IllegalStateException) { }
        assertEquals(0, cache.inventory().first)
    }

    @Test fun cancellationFenceClearAndConfiguredBudgetPreventCommit() = runBlocking {
        for (mode in listOf("cancel", "clear", "budget")) {
            val cache = VesperDashStartupCache()
            val scope = DashStartupScope()
            val source = VesperPlayerSource.remote(uri, label = "test")
            var requests = 0
            val transport = DashStartupTransport { resource, _, _, _ ->
                requests++
                if (mode == "clear" && requests == 4) cache.clear()
                DashStartupBytes(resource, when {
                    resource.uri == uri -> manifest
                    resource.position == 10L -> sidx()
                    resource.position == 0L -> ByteArray(10)
                    else -> ByteArray(20)
                })
            }
            try {
                warmDashStartup(source, scope, 1000, transport, cache,
                    commitFence = { if (mode == "cancel") false else it() },
                    maximumBytes = if (mode == "budget") 1 else VesperDashStartupCache.MAX_WARMUP_BYTES)
                fail("$mode must prevent commit")
            } catch (_: IllegalStateException) {} catch (_: IllegalArgumentException) {}
            assertEquals(0, cache.inventory().first)
        }
    }

    @Test fun sequenceMemoryBudgetEvictsOlderSourceScopes() {
        val cache = VesperDashStartupCache()
        val first = DashStartupScope(owner = "sequence")
        val second = DashStartupScope(owner = "sequence")
        val resource = DashStartupResource(uri)
        val value = DashStartupBytes(resource, byteArrayOf(1, 2))
        cache.store(first, listOf(value), emptyMap(), maximumBytes = 2)
        cache.store(second, listOf(value), emptyMap(), maximumBytes = 2)
        assertNull(cache.read(first, resource, emptyMap()))
        assertArrayEquals(value.bytes, cache.read(second, resource, emptyMap())?.bytes)
        assertEquals(2L, cache.inventory().second)
    }

    @Test fun manifestRejectsDynamicProtectedAndExternalEntityInputs() {
        for (xml in listOf(String(manifest).replace("static", "dynamic"), String(manifest).replace("<Period>", "<Period><ContentProtection/>"),
            "<!DOCTYPE MPD [<!ENTITY ext SYSTEM 'file:///private'>]>" + String(manifest))) {
            try { VesperDashStartupPlanner.parseManifest(xml.toByteArray(), uri); fail("must reject") }
            catch (_: IllegalArgumentException) { }
        }
    }
}
