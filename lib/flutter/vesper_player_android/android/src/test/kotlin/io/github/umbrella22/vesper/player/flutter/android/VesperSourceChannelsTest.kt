package io.github.umbrella22.vesper.player.flutter.android

import android.content.Context
import android.content.ContextWrapper
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class VesperSourceChannelsTest {
    @Test
    fun integerOptionsRejectFractionalFloatingAndOverflowValues() {
        for (value in listOf(1.5, 1.0, "1", true, null)) {
            val error = runCatching { mapOf("timeoutMs" to value).activationOptions() }.exceptionOrNull()
            assertTrue("value=$value", error is IllegalArgumentException)
            assertEquals("invalid_timeoutMs", error?.message)
        }
        assertEquals(4, mapOf("maxSources" to 4L).int("maxSources", 128))
        val overflow = runCatching { mapOf("maxSources" to 4_294_967_297L).int("maxSources", 128) }
        assertEquals("invalid_maxSources", overflow.exceptionOrNull()?.message)
    }

    @Test
    fun malformedPresentOptionsNeverBecomeDefaults() {
        assertEquals("invalid_playWhenReady", runCatching {
            mapOf("playWhenReady" to 1).activationOptions()
        }.exceptionOrNull()?.message)
        assertEquals("invalid_playbackRate", runCatching {
            mapOf("playbackRate" to "fast").activationOptions()
        }.exceptionOrNull()?.message)
        assertEquals("invalid_options", runCatching {
            mapOf("options" to "default").nested("options")
        }.exceptionOrNull()?.message)
    }

    @Test
    fun anotherSourcesCompletionDoesNotEvictALateAwait() = runBlocking {
        val context = object : ContextWrapper(null) {
            override fun getApplicationContext(): Context = this
        }
        val channels = VesperSourceChannels(context) { error("No player should be created") }
        try {
            val session = channels.execute("createSourceSession", mapOf("configuration" to mapOf("maxSources" to 130))) as Map<*, *>
            val sessionId = session["sessionId"]
            var first: Map<*, *>? = null
            repeat(130) {
                val source = channels.execute("registerSource", mapOf(
                    "sessionId" to sessionId, "expiresAtEpochMs" to null,
                    "source" to mapOf("uri" to "https://test/$it.m3u8", "kind" to "remote", "protocol" to "hls"),
                )) as Map<*, *>
                val task = channels.execute("preloadSource", mapOf(
                    "sessionId" to sessionId, "sourceId" to source["sourceId"],
                )) as Map<*, *>
                if (first == null) first = task
            }
            val result = channels.execute("awaitSourcePreload", mapOf(
                "sessionId" to sessionId, "taskId" to first!!["taskId"],
            )) as Map<*, *>
            assertEquals(first!!["taskId"], result["taskId"])
            assertEquals("unsupported", result["status"])
        } finally { channels.close() }
    }
}
