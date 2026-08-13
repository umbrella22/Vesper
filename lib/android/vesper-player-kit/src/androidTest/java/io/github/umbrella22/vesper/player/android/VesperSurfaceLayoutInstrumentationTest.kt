package io.github.umbrella22.vesper.player.android

import android.app.Activity
import android.net.Uri
import android.os.Bundle
import android.view.Gravity
import android.view.SurfaceView
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperSurfaceLayoutInstrumentationTest {
    @Test
    fun videoAspectFitSurvivesSurfaceHostRecreation() {
        val appContext = ApplicationProvider.getApplicationContext<android.content.Context>()
        val mediaFile = File(appContext.cacheDir, "vesper-surface-layout.m4v")
        appContext.assets.open("tiny-h264-aac.m4v").use { input ->
            mediaFile.outputStream().use(input::copyTo)
        }
        val source = VesperPlayerSource.local(Uri.fromFile(mediaFile).toString(), "surface layout")
        var controller: VesperPlayerController? = null
        var host: FrameLayout? = null

        ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
            try {
                scenario.onActivity { activity ->
                    host = activity.replaceSurfaceHost()
                    controller =
                        VesperPlayerControllerFactory.createDefault(
                            context = activity.applicationContext,
                            initialSource = source,
                            surfaceKind = VesperVideoSurfaceKind.SurfaceView,
                            keepScreenOnDuringPlayback = false,
                        ).also { player ->
                            player.attachSurfaceHost(requireNotNull(host))
                            player.initialize()
                            player.play()
                        }
                }

                assertAspectFit(scenario) { requireNotNull(host) }

                scenario.onActivity { activity ->
                    val previousHost = requireNotNull(host)
                    requireNotNull(controller).detachSurfaceHost(previousHost)
                    host = activity.replaceSurfaceHost()
                    requireNotNull(controller).attachSurfaceHost(requireNotNull(host))
                }

                assertAspectFit(scenario) { requireNotNull(host) }
            } finally {
                scenario.onActivity {
                    controller?.dispose()
                    controller = null
                }
                mediaFile.delete()
            }
        }
    }

    private fun assertAspectFit(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
        hostProvider: () -> FrameLayout,
    ) {
        var observedHostSize = 0 to 0
        var observedSurfaceSize = 0 to 0
        var observedLayoutParamsSize = 0 to 0
        val fitted = awaitCondition(15) {
            scenario.onActivity {
                val host = hostProvider()
                val surfaceView = host.findSurfaceView()
                observedHostSize = host.width to host.height
                observedSurfaceSize = (surfaceView?.width ?: 0) to (surfaceView?.height ?: 0)
                observedLayoutParamsSize =
                    (surfaceView?.layoutParams?.width ?: 0) to
                        (surfaceView?.layoutParams?.height ?: 0)
            }
            observedHostSize == (400 to 300) && observedSurfaceSize == (400 to 225)
        }

        assertTrue(
            "expected 16:9 SurfaceView 400x225 inside 400x300 host; " +
                "host=${observedHostSize.first}x${observedHostSize.second}, " +
                "surface=${observedSurfaceSize.first}x${observedSurfaceSize.second}, " +
                "layoutParams=${observedLayoutParamsSize.first}x${observedLayoutParamsSize.second}",
            fitted,
        )
        assertEquals(400 to 300, observedHostSize)
        assertEquals(400 to 225, observedSurfaceSize)
        assertEquals(400 to 225, observedLayoutParamsSize)
    }

    private fun awaitCondition(
        timeoutSeconds: Long,
        predicate: () -> Boolean,
    ): Boolean {
        val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(timeoutSeconds)
        while (System.nanoTime() < deadlineNanos) {
            if (predicate()) return true
            Thread.sleep(25L)
        }
        return predicate()
    }
}

class VesperSurfaceLayoutTestActivity : Activity() {
    private lateinit var root: FrameLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        root = FrameLayout(this)
        setContentView(root)
    }

    fun replaceSurfaceHost(): FrameLayout =
        FrameLayout(this).also { host ->
            root.removeAllViews()
            root.addView(
                host,
                FrameLayout.LayoutParams(400, 300, Gravity.CENTER),
            )
        }
}

private fun ViewGroup.findSurfaceView(): SurfaceView? {
    repeat(childCount) { index ->
        when (val child = getChildAt(index)) {
            is SurfaceView -> return child
            is ViewGroup -> child.findSurfaceView()?.let { return it }
        }
    }
    return null
}
