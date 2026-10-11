package io.github.umbrella22.vesper.player.android.external

import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperPlayerSourceProtocol
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperExternalSourcePreparationResult
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperExternalPlaybackRelayLifecycleTest {
    @Test
    fun protocolInfoCancellationNeverStartsPreparationOrPrewarm() = runBlocking {
        var preparations = 0
        val cancellation = CancellationException("Protocol info cancelled")
        try {
            prepareDlnaSourceWithProtocolInfoAsync({ throw cancellation }) {
                preparations += 1
                null
            }
            fail("Cancelled protocol info must propagate")
        } catch (error: CancellationException) {
            assertTrue(error === cancellation)
        }
        assertEquals(0, preparations)
    }

    @Test
    fun ownerCancellationDuringProtocolInfoNeverStartsPreparationOrPrewarm() = runBlocking {
        val querying = CompletableDeferred<Unit>()
        var preparations = 0
        val owner = launch {
            prepareDlnaSourceWithProtocolInfoAsync({
                querying.complete(Unit)
                awaitCancellation()
            }) {
                preparations += 1
                null
            }
        }
        withTimeout(5_000) { querying.await() }
        owner.cancel()
        withTimeout(5_000) { owner.join() }
        assertTrue(owner.isCancelled)
        assertEquals(0, preparations)
    }

    @Test
    fun unavailableProtocolInfoStillPreparesWithUnknownCapabilities() = runBlocking {
        var preparations = 0
        prepareDlnaSourceWithProtocolInfoAsync({ throw IllegalStateException("Unavailable") }) {
            assertEquals("", it)
            preparations += 1
            null
        }
        assertEquals(1, preparations)
    }

    @Test
    fun alreadyCancelledCallerNeverStartsNonCancellablePreparation() = runBlocking {
        var preparations = 0
        val owner = launch {
            kotlinx.coroutines.currentCoroutineContext()[kotlinx.coroutines.Job]!!.cancel()
            prepareExternalSourceOnIo(cleanup = { fail("Nothing was prepared") }) {
                preparations += 1
                null
            }
        }
        owner.join()
        assertTrue(owner.isCancelled)
        assertEquals(0, preparations)
    }

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
    @Test
    fun actualOwnerCancellationDuringIoPreparationCleansExactlyOnce() = runBlocking {
        val ready = CompletableDeferred<Unit>()
        val gate = CountDownLatch(1)
        val preparedCount = AtomicInteger()
        val cleaned = AtomicInteger()
        val owner = launch {
            prepareExternalSourceOnIo(cleanup = { cleaned.incrementAndGet() }) {
                preparedCount.incrementAndGet()
                ready.complete(Unit)
                check(gate.await(5, TimeUnit.SECONDS))
                VesperExternalSourcePreparationResult.Prepared(
                    source = VesperPlayerSource.remote("http://127.0.0.1/video.mp4", "Relayed"),
                    relayToken = "owned-before-dispatcher-return",
                    relayEnabled = true,
                )
            }
        }
        try {
            withTimeout(5_000) { ready.await() }
            owner.cancel()
        } finally {
            gate.countDown()
        }
        withTimeout(5_000) { owner.join() }
        assertTrue(owner.isCancelled)
        assertEquals(1, preparedCount.get())
        assertEquals(1, cleaned.get())
    }

    @Test
    fun activationPinsNewTokenBeforeInvalidatingOldAndFailurePreservesOld() {
        val tokens = ConcurrentHashMap.newKeySet<String>()
        tokens.add("old")
        val events = mutableListOf<String>()
        assertTrue(pinAndReplaceActiveRelayTokens(tokens, "new", { events += "pin:$it"; true }, { events += "invalidate:$it" }))
        assertEquals(listOf("pin:new", "invalidate:old"), events)
        assertEquals(setOf("new"), tokens)
        events.clear()
        assertTrue(!pinAndReplaceActiveRelayTokens(tokens, "expired", { events += "pin:$it"; false }, { events += "invalidate:$it" }))
        assertEquals(listOf("pin:expired"), events)
        assertEquals(setOf("new"), tokens)
        assertTrue(pinAndReplaceActiveRelayTokens(tokens, null, { fail("direct source should not pin"); false }, { events += "invalidate:$it" }))
        assertTrue(tokens.isEmpty())
        assertEquals("invalidate:new", events.last())
    }

    @Test
    fun unsupportedPreparationPreservesTypedCodeAndDetails() {
        val result = VesperExternalSourcePreparationResult.Unsupported(
            "unsupported raw HLS relay", code = "unsupported_hls_relay", details = mapOf("protocol" to "hls"),
        ).toExternalUnsupportedResult()
        assertEquals("unsupported_hls_relay", result.code)
        assertEquals(mapOf("protocol" to "hls"), result.details)
    }

}
