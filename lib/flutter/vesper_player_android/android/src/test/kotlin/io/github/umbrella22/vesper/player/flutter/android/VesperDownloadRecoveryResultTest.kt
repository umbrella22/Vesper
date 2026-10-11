package io.github.umbrella22.vesper.player.flutter.android

import io.flutter.plugin.common.MethodChannel
import io.github.umbrella22.vesper.player.android.VesperDownloadRecoveredTaskPlan
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperDownloadRecoveryResultTest {
    private val validPlan = mapOf(
        "source" to mapOf("source" to mapOf("uri" to "https://fixture.test/index.mpd")),
        "profile" to emptyMap<String, Any?>(),
        "assetIndex" to mapOf("resources" to listOf(
            mapOf("resourceId" to "manifest", "uri" to "https://fixture.test/index.mpd"),
        )),
    )

    @Test
    fun channelErrorCallbackPropagatesCodeMessageAndOriginalDetailsToAwait() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        val details = mapOf("field" to "targetOutputFormat", "nested" to listOf(7, null))
        callback.error("provider_failed", "Unknown caller targetOutputFormat", details)
        assertTrue(deferred.isCompleted)
        try {
            deferred.await()
            fail("Channel errors must not become a declined recovery")
        } catch (error: DownloadRecoveryChannelException) {
            assertEquals("provider_failed", error.errorCode)
            assertEquals("Unknown caller targetOutputFormat", error.errorMessage)
            assertSame(details, error.errorDetails)
            assertTrue(error.message.orEmpty().contains("provider_failed"))
            assertTrue(error.message.orEmpty().contains("targetOutputFormat"))
        }
    }

    @Test
    fun channelErrorRetainsNullMessageAndDetails() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        callback.error("provider_failed", null, null)
        try {
            deferred.await()
            fail("Channel errors must propagate")
        } catch (error: DownloadRecoveryChannelException) {
            assertEquals("provider_failed", error.errorCode)
            assertNull(error.errorMessage)
            assertNull(error.errorDetails)
        }
    }

    @Test
    fun firstErrorCallbackWinsOverLateSuccessAndNotImplemented() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        callback.error("first_failure", "first", null)
        callback.success(validPlan)
        callback.notImplemented()
        callback.error("late_failure", "late", null)
        try {
            deferred.await()
            fail("The first callback failure must remain observable")
        } catch (error: DownloadRecoveryChannelException) {
            assertEquals("first_failure", error.errorCode)
        }
    }

    @Test
    fun validSuccessWinsOverLateErrorCallback() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        callback.success(validPlan)
        callback.error("late_failure", "late", null)
        assertEquals("manifest", deferred.await()?.assetIndex?.resources?.single()?.resourceId)
    }

    @Test
    fun nilAndNotImplementedStillDeclineRecovery() = runBlocking {
        for (notImplemented in listOf(false, true)) {
            val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
            val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
            if (notImplemented) callback.notImplemented() else callback.success(null)
            assertNull(deferred.await())
        }
    }

    @Test
    fun malformedSuccessCallbackFailsAwaitInsteadOfWaitingForTimeout() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        callback.success(validPlan + ("assetIndex" to mapOf("segments" to listOf(null))))
        assertTrue(deferred.isCompleted)
        try {
            deferred.await()
            fail("Malformed success DTOs must fail recovery")
        } catch (error: IllegalArgumentException) {
            assertTrue(error.message.orEmpty().contains("segments[0]"))
        }
    }

    @Test
    fun boundedTimeoutStillReturnsNullAndLateCallbacksDoNotThrow() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        assertNull(withTimeoutOrNull(1L) { deferred.await() })
        callback.error("late_failure", "late", null)
        callback.success(validPlan)
        callback.notImplemented()
    }

    @Test
    fun awaitingCancellationCompletesWithoutCallbackAndAcceptsLateCallbacks() = runBlocking {
        val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        val callback: MethodChannel.Result = DownloadRecoveryResult(deferred)
        var cancelled = false
        val waiter = launch(start = CoroutineStart.UNDISPATCHED) {
            try {
                deferred.await()
                fail("Cancelled recovery must not succeed")
            } catch (_: CancellationException) {
                cancelled = true
            }
        }
        waiter.cancel()
        waiter.join()
        assertTrue(cancelled)
        callback.error("late_failure", "late", null)
        callback.success(validPlan)
    }
}
