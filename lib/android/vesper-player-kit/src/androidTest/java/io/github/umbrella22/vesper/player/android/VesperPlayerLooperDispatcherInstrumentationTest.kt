package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.graphics.SurfaceTexture
import android.os.Looper
import android.view.Surface
import androidx.media3.exoplayer.ExoPlayer
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertSame
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperPlayerLooperDispatcherInstrumentationTest {
    @Test
    fun workerStopInvalidatesHdrOnMainBeforeStoppingPlayback() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val controllerReference = AtomicReference<VesperPlayerController>()
        val outputLooper = AtomicReference<Looper>()
        val stateAtInvalidation = AtomicReference<PlaybackStateUi>()
        val generationAtInvalidation = AtomicReference<Long>()
        instrumentation.runOnMainSync {
            val bridge = VesperNativePlayerBridge()
            bridge._uiState.value = bridge._uiState.value.copy(playbackState = PlaybackStateUi.Playing)
            bridge.hdrOutputTracker.outputPathChanged("display-0")
            assertTrue(bridge.hdrOutputTracker.apply(
                bridge.hdrOutputTracker.capture(),
                VesperHdrOutputObservation(VesperHdrOutputState.Hdr, evidence = "testOutputObserver"),
            ))
            val controller = VesperPlayerController(bridge)
            controllerReference.set(controller)
            controller.setOnHdrOutputChangedListener { output ->
                if (output.state == VesperHdrOutputState.Unknown) {
                    outputLooper.set(Looper.myLooper())
                    stateAtInvalidation.set(controller.uiState.value.playbackState)
                    generationAtInvalidation.set(output.outputGeneration)
                }
            }
        }
        val controller = checkNotNull(controllerReference.get())
        val previousGeneration = controller.hdrOutput!!.value.outputGeneration
        val worker = Executors.newSingleThreadExecutor()
        try {
            worker.submit { controller.stop() }.get(5, TimeUnit.SECONDS)

            assertSame(Looper.getMainLooper(), outputLooper.get())
            assertEquals(PlaybackStateUi.Playing, stateAtInvalidation.get())
            assertEquals(previousGeneration + 1, generationAtInvalidation.get())
            assertEquals(PlaybackStateUi.Ready, controller.uiState.value.playbackState)
            assertEquals("display-0", controller.hdrOutput!!.value.displayId)
        } finally {
            worker.shutdownNow()
            assertTrue(worker.awaitTermination(1, TimeUnit.SECONDS))
            instrumentation.runOnMainSync(controller::dispose)
        }
    }

    @Test
    fun workerSurfaceMutationsRunOnMedia3ApplicationLooper() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val playerReference = AtomicReference<ExoPlayer>()
        instrumentation.runOnMainSync {
            playerReference.set(
                ExoPlayer.Builder(context)
                    .setLooper(Looper.getMainLooper())
                    .build(),
            )
        }
        val player = checkNotNull(playerReference.get())
        val surfaceTexture = SurfaceTexture(0)
        val surface = Surface(surfaceTexture)
        val attachLooper = AtomicReference<Looper>()
        val clearLooper = AtomicReference<Looper>()
        val worker = Executors.newSingleThreadExecutor()

        try {
            worker.submit {
                runPlayerSurfaceOperation(player, "instrumentation surface attach") {
                    attachLooper.set(Looper.myLooper())
                    it.setVideoSurface(surface)
                }
                runPlayerSurfaceOperation(player, "instrumentation surface clear") {
                    clearLooper.set(Looper.myLooper())
                    it.clearVideoSurface()
                }
            }.get(5, TimeUnit.SECONDS)

            assertSame(player.applicationLooper, attachLooper.get())
            assertSame(player.applicationLooper, clearLooper.get())
        } finally {
            worker.shutdownNow()
            assertTrue(worker.awaitTermination(1, TimeUnit.SECONDS))
            instrumentation.runOnMainSync(player::release)
            surface.release()
            surfaceTexture.release()
        }
    }
}
