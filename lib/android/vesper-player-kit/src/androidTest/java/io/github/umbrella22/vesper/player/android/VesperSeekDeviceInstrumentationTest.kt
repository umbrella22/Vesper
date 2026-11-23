package io.github.umbrella22.vesper.player.android

import android.net.Uri
import android.util.Log
import androidx.media3.common.PlaybackException
import androidx.media3.exoplayer.SeekParameters
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import kotlin.math.abs
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperSeekDeviceInstrumentationTest {
    @Test
    fun defaultPlayerSeeksByTheCurrentHeadOnVodHls() = runBlocking {
        val arguments = InstrumentationRegistry.getArguments()
        val hlsUrl = arguments.getString("vesperSeekHlsUrl")
        val localPlaylist = arguments.getString("vesperSeekHlsFile")?.let(::File)
        assumeTrue(hlsUrl != null || localPlaylist != null)
        val playlist = if (hlsUrl != null) {
            readPlaylistFromOrigin(hlsUrl)
        } else {
            checkNotNull(localPlaylist).readText()
        }
        assertTrue("The fixture must have a complete VOD playlist", "#EXT-X-ENDLIST" in playlist)
        assertTrue("#EXT-X-PLAYLIST-TYPE:VOD" in playlist)
        val source = if (hlsUrl != null) {
            VesperPlayerSource.hls(hlsUrl, "Seek regression VOD")
        } else {
            VesperPlayerSource.local(Uri.fromFile(checkNotNull(localPlaylist)).toString(), "Seek regression VOD")
        }

        ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
            lateinit var controller: VesperPlayerController
            scenario.onActivity { activity ->
                controller = VesperPlayerControllerFactory.createDefault(activity.applicationContext)
                controller.attachSurfaceHost(activity.replaceSurfaceHost())
            }
            try {
                controller.selectSourceAsync(source)
                withContext(Dispatchers.Main.immediate) { controller.play() }

                for (awaitable in listOf(false, true)) {
                    withTimeout(50_000L) {
                        val minimumGapMs = if (awaitable) 6_000L else 28_000L
                        val event = async(start = CoroutineStart.UNDISPATCHED) {
                            controller.playbackEvents.first {
                                it is VesperPlaybackEvent.SeekCompleted ||
                                    it is VesperPlaybackEvent.SeekAdjusted || it is VesperPlaybackEvent.SeekFailed
                            }
                        }
                        val (snapshot, before) = withContext(Dispatchers.Main.immediate) {
                            var sample: TimelineUiState
                            var published: Long
                            do {
                                delay(200L)
                                sample = checkNotNull(controller.sampleTimeline())
                                published = controller.uiState.value.timeline.positionMs
                            } while (sample.positionMs - published < minimumGapMs)
                            assertEquals(TimelineKind.Vod, sample.kind)
                            assertEquals(150_000L, sample.durationMs)
                            if (awaitable) controller.seekByAsync(10_000L) else controller.seekBy(10_000L)
                            published to sample.positionMs
                        }
                        val outcome = event.await()
                        val (target, actual) = when (outcome) {
                            is VesperPlaybackEvent.SeekCompleted -> outcome.targetPositionMs to outcome.positionMs
                            is VesperPlaybackEvent.SeekAdjusted -> outcome.targetPositionMs to outcome.positionMs
                            else -> error("Unexpected seek outcome: $outcome")
                        }
                        val sampledAfter = checkNotNull(controller.sampleTimeline()).positionMs
                        Log.i("VesperSeekDevice", "async=$awaitable snapshot=$snapshot before=$before target=$target actual=$actual after=$sampledAfter")
                        assertTrue("Seek target $target did not use the sampled head $before", abs(target - before - 10_000L) <= 250L)
                        assertTrue("Seek landed at $actual, expected $target", abs(actual - target) <= 1_000L)
                        assertTrue("Playback head $sampledAfter did not reach $actual", abs(sampledAfter - actual) <= 1_000L)
                    }
                }
                val absolute = controller.seekToAsync(95_000L)
                assertTrue("Absolute seek landed at $absolute", abs(absolute - 95_000L) <= 1_000L)
            } finally {
                scenario.onActivity { controller.dispose() }
            }
        }
    }

    @Test
    fun media3SeekAdjustmentReportsActualLanding() = runBlocking {
        withLocalVideo { controller, bindings ->
            val observed = async(start = CoroutineStart.UNDISPATCHED) {
                withTimeout(5_000L) { controller.playbackEvents.first() }
            }
            withContext(Dispatchers.Main.immediate) {
                checkNotNull(bindings.player).setSeekParameters(SeekParameters.PREVIOUS_SYNC)
            }
            val targetMs = 103_000L
            val actualMs = withTimeout(5_000L) { controller.seekToAsync(targetMs) }
            assertEquals(VesperPlaybackEvent.SeekAdjusted(targetMs, actualMs), observed.await())
            val afterMs = checkNotNull(controller.sampleTimeline()).positionMs
            Log.i("VesperSeekDevice", "adjusted target=$targetMs actual=$actualMs after=$afterMs")
            assertTrue("Expected the previous sync point, received $actualMs", abs(actualMs - 100_000L) <= 300L)
            assertTrue(abs(afterMs - actualMs) <= 300L)
        }
    }

    @Test
    fun liveWindowRecoveryReportsReadBackAfterPrepare() = runBlocking {
        withLocalVideo { controller, bindings ->
            controller.seekToAsync(20_000L)
            val observed = async(start = CoroutineStart.UNDISPATCHED) {
                withTimeout(5_000L) {
                    controller.playbackEvents.first { it is VesperPlaybackEvent.LiveWindowRecovered }
                }
            }
            withContext(Dispatchers.Main.immediate) {
                val error = PlaybackException(
                    "Injected behind-live-window error",
                    null,
                    PlaybackException.ERROR_CODE_BEHIND_LIVE_WINDOW,
                )
                assertTrue(bindings.handleBehindLiveWindowError(error, bindings.sourceCallbackGeneration))
            }
            val landed = (observed.await() as VesperPlaybackEvent.LiveWindowRecovered).positionMs
            val afterMs = checkNotNull(controller.sampleTimeline()).positionMs
            Log.i("VesperSeekDevice", "injected_live_window_recovery actual=$landed after=$afterMs")
            assertTrue(landed in 0L..300L)
            assertTrue(abs(afterMs - landed) <= 300L)
        }
    }

    @Test
    fun synchronousSeekRejectionReachesCustomControls() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val controller = VesperPlayerControllerFactory.createDefault(context)
        try {
            val observation = async(start = CoroutineStart.UNDISPATCHED) {
                withTimeout(5_000L) { controller.playbackEvents.first() }
            }
            val failure = runCatching { controller.seekBy(10_000L) }.exceptionOrNull()
            assertTrue(failure is VesperPlayerCommandException)
            val errorState = (failure as VesperPlayerCommandException).errorState
            assertEquals("seekCommandNotReady", errorState.details["commandReason"])
            assertEquals(VesperPlaybackEvent.SeekFailed(errorState), observation.await())
        } finally {
            withContext(Dispatchers.Main.immediate) { controller.dispose() }
        }
    }

    @Test
    fun failedMediaLoadPublishesRetryWithoutBuiltInControls() = runBlocking {
        val retryUrl = InstrumentationRegistry.getArguments().getString("vesperSeekRetryUrl")
        assumeTrue(retryUrl != null)
        ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
            lateinit var controller: VesperPlayerController
            scenario.onActivity { activity ->
                controller = VesperPlayerControllerFactory.createDefault(activity.applicationContext)
            }
            try {
                val observed = async(start = CoroutineStart.UNDISPATCHED) {
                    withTimeout(20_000L) {
                        controller.playbackEvents.first { it is VesperPlaybackEvent.RetryScheduled }
                    }
                }
                withContext(Dispatchers.Main.immediate) {
                    controller.selectSource(VesperPlayerSource.hls(checkNotNull(retryUrl), "Retry fixture"))
                }
                val retry = observed.await() as VesperPlaybackEvent.RetryScheduled
                Log.i("VesperSeekDevice", "media_load_retry attempt=${retry.attempt} delayMs=${retry.delayMs}")
                assertEquals(1, retry.attempt)
                assertTrue(retry.delayMs >= 0L)
            } finally {
                scenario.onActivity { controller.dispose() }
            }
        }
    }

    private suspend fun withLocalVideo(
        test: suspend (VesperPlayerController, VesperNativeJniBindings) -> Unit,
    ) {
        val path = InstrumentationRegistry.getArguments().getString("vesperSeekVideoFile")
        assumeTrue(path != null)
        val source = VesperPlayerSource.local(Uri.fromFile(File(checkNotNull(path))).toString(), "Seek fixture")
        ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
            lateinit var controller: VesperPlayerController
            lateinit var bindings: VesperNativeJniBindings
            scenario.onActivity { activity ->
                val context = activity.applicationContext
                bindings = VesperNativeJniBindings(context)
                controller = VesperPlayerController(VesperNativePlayerBridge(bindings, appContext = context))
                controller.attachSurfaceHost(activity.replaceSurfaceHost())
            }
            try {
                controller.selectSourceAsync(source)
                withContext(Dispatchers.Main.immediate) { controller.pause() }
                test(controller, bindings)
            } finally {
                scenario.onActivity { controller.dispose() }
            }
        }
    }

    private fun readPlaylistFromOrigin(hlsUrl: String): String {
        val connection = URI(hlsUrl).toURL().openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 3_000
            connection.readTimeout = 3_000
            return connection.inputStream.bufferedReader().use { it.readText() }
        } finally {
            connection.disconnect()
        }
    }

}
