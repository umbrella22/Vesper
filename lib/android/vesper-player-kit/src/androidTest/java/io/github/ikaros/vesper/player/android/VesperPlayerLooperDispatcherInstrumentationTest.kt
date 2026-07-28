package io.github.ikaros.vesper.player.android

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
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperPlayerLooperDispatcherInstrumentationTest {
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
