package io.github.ikaros.vesper.player.android.external

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationResult
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperExternalPlaybackRelayLifecycleTest {
    @Test
    fun prepareFailuresAreControllerScoped() {
        val first = VesperExternalPreparationState()
        val second = VesperExternalPreparationState()
        first.lastFailure = VesperExternalPlaybackResult.Unsupported("first controller failure")

        assertEquals(
            VesperExternalPlaybackResult.Unsupported("first controller failure"),
            first.lastFailure,
        )
        assertEquals(
            VesperExternalPlaybackResult.Unsupported("No playable external playback source is available."),
            second.lastFailure,
        )
    }

    @Test
    fun replaceActiveRelayTokensInvalidatesPreviousTokensAndKeepsCurrentToken() {
        val tokens = ConcurrentHashMap.newKeySet<String>()
        tokens.add("old-a")
        tokens.add("old-b")
        val invalidated = mutableListOf<String>()

        replaceActiveRelayTokens(tokens, "new", invalidated::add)

        assertEquals(setOf("new"), tokens)
        assertEquals(listOf("old-a", "old-b").sorted(), invalidated.sorted())
    }

    @Test
    fun replaceActiveRelayTokensInvalidatesPreviousTokensForDirectSource() {
        val tokens = ConcurrentHashMap.newKeySet<String>()
        tokens.add("old")
        val invalidated = mutableListOf<String>()

        replaceActiveRelayTokens(tokens, null, invalidated::add)

        assertTrue(tokens.isEmpty())
        assertEquals(listOf("old"), invalidated)
    }

    @Test
    fun prepareExternalSourceOnIoCleansPreparedRelayWhenCancelledBeforeReturn() = runBlocking {
        val cleaned = mutableListOf<String>()

        try {
            prepareExternalSourceOnIo(
                cleanup = { prepared ->
                    prepared.relayToken?.let(cleaned::add)
                },
                isCancelled = { true },
            ) {
                VesperExternalSourcePreparationResult.Prepared(
                    source = VesperPlayerSource.remote(
                        uri = "http://127.0.0.1/video.mp4",
                        label = "Relayed",
                        protocol = VesperPlayerSourceProtocol.Progressive,
                    ),
                    relayToken = "relay-token",
                    relayEnabled = true,
                )
            }
            fail("cancelled prepare should not return")
        } catch (_: CancellationException) {
            // Expected.
        }

        assertEquals(listOf("relay-token"), cleaned)
    }

    @Test
    fun preparedRelayLoadCancellationCleansRelayToken() = runBlocking {
        val cleaned = mutableListOf<String>()
        val prepared =
            VesperExternalSourcePreparationResult.Prepared(
                source = VesperPlayerSource.remote(
                    uri = "http://127.0.0.1/video.mp4",
                    label = "Relayed",
                    protocol = VesperPlayerSourceProtocol.Progressive,
                ),
                relayToken = "dlna-relay-token",
                relayEnabled = true,
            )

        try {
            withPreparedRelayLoadCancellationCleanup(
                prepared = prepared,
                cleanup = { relayPrepared ->
                    relayPrepared.relayToken?.let(cleaned::add)
                },
            ) {
                throw CancellationException("DLNA load cancelled")
            }
            fail("cancelled load should not return")
        } catch (_: CancellationException) {
            // Expected.
        }

        assertEquals(listOf("dlna-relay-token"), cleaned)
    }
}
