package io.github.umbrella22.vesper.player.android

import android.app.Activity
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], manifest = Config.NONE)
@LooperMode(LooperMode.Mode.PAUSED)
class VesperNativeSurfaceHostKeepScreenOnTest {
    @Test
    fun movingPlayingSurfaceViewReleasesPreviousVisibleHost() {
        assertPlayingMigration(NativeVideoSurfaceKind.SurfaceView)
    }

    @Test
    fun movingPlayingTextureViewReleasesPreviousVisibleHost() {
        assertPlayingMigration(NativeVideoSurfaceKind.TextureView)
    }

    @Test
    fun movingPlayingSurfaceViewWithReentrantDetachSyncReleasesPreviousHost() {
        withHosts(NativeVideoSurfaceKind.SurfaceView) { owner, first, second ->
            owner.attach(first)
            owner.setKeepScreenOn(true)
            owner.setKeepScreenOn(false)
            owner.setKeepScreenOn(true)
            val render = first.getChildAt(0) as SurfaceView
            val subtitle = first.getChildAt(1)
            var detachCallbacks = 0
            val listener = object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(view: View) = Unit

                override fun onViewDetachedFromWindow(view: View) {
                    detachCallbacks += 1
                    // Native surface detachment can synchronously refresh still-playing state.
                    owner.setKeepScreenOn(true)
                }
            }
            render.addOnAttachStateChangeListener(listener)
            try {
                owner.attach(first)
                assertEquals(0, detachCallbacks)
                assertSame(render, first.getChildAt(0))
                assertTrue(first.keepScreenOn)
                owner.attach(second)
            } finally {
                render.removeOnAttachStateChangeListener(listener)
            }
            // The public bridge also refreshes playback state after the move.
            owner.setKeepScreenOn(true)

            assertEquals("The real render view must synchronously detach once", 1, detachCallbacks)
            assertSame(render, second.getChildAt(0))
            assertSame(subtitle, second.getChildAt(1))
            assertSame(second, render.parent)
            assertEquals(0, first.childCount)
            assertTrue(first.isShown)
            assertTrue(second.isShown)
            assertFalse("Reentrant sync must not restore the previous host's screen request", first.keepScreenOn)
            assertTrue(second.keepScreenOn)
            assertTrue(render.keepScreenOn)

            owner.detach(expectedHost = first)
            assertSame(second, render.parent)
            assertTrue(second.keepScreenOn)
            owner.setKeepScreenOn(false)
            assertFalse(first.keepScreenOn)
            assertFalse(second.keepScreenOn)
            assertFalse(render.keepScreenOn)
            owner.setKeepScreenOn(true)
            owner.close()
            owner.detach(expectedHost = second)
            owner.close()
            owner.detach(expectedHost = second)

            assertFalse(first.keepScreenOn)
            assertFalse(second.keepScreenOn)
            assertFalse(render.keepScreenOn)
            assertNull(render.parent)
            assertEquals(0, first.childCount)
            assertEquals(0, second.childCount)
        }
    }

    @Test
    fun disposalAfterMovingClearsAllOwnedFlags() {
        forEachKind { owner, first, second ->
            owner.attach(first)
            owner.setKeepScreenOn(true)
            val render = first.getChildAt(0)
            owner.attach(second)
            owner.setKeepScreenOn(true)
            owner.close()
            owner.detach(expectedHost = second)

            assertFalse(first.keepScreenOn)
            assertFalse(second.keepScreenOn)
            assertFalse(render.keepScreenOn)
            assertNull(render.parent)
            assertEquals(0, first.childCount)
            assertEquals(0, second.childCount)
        }
    }

    @Test
    fun sameHostAttachPreservesRenderAndPlayingRequest() {
        forEachKind { owner, first, _ ->
            owner.attach(first)
            owner.setKeepScreenOn(true)
            val render = first.getChildAt(0)
            val subtitle = first.getChildAt(1)
            owner.attach(first)

            assertSame(render, first.getChildAt(0))
            assertSame(subtitle, first.getChildAt(1))
            assertTrue(first.keepScreenOn)
            assertTrue(render.keepScreenOn)
            owner.setKeepScreenOn(false)
            assertFalse(first.keepScreenOn)
            assertFalse(render.keepScreenOn)
        }
    }

    @Test
    fun pausedMigrationAndStaleHostDetachPreserveCurrentOwner() {
        forEachKind { owner, first, second ->
            owner.attach(first)
            owner.setKeepScreenOn(true)
            owner.setKeepScreenOn(false)
            val render = first.getChildAt(0)
            owner.attach(second)
            owner.setKeepScreenOn(false)

            assertFalse(first.keepScreenOn)
            assertFalse(second.keepScreenOn)
            assertFalse(render.keepScreenOn)
            owner.setKeepScreenOn(true)
            owner.detach(expectedHost = first)
            assertSame(second, render.parent)
            assertEquals(2, second.childCount)
            assertTrue(second.keepScreenOn)
            assertTrue(render.keepScreenOn)
            owner.detach(expectedHost = second)
            assertFalse(second.keepScreenOn)
            assertFalse(render.keepScreenOn)
        }
    }

    @Test
    fun idleMigrationDoesNotClearAnApplicationSetHostFlag() {
        forEachKind { owner, first, second ->
            owner.attach(first)
            owner.setKeepScreenOn(false)
            first.keepScreenOn = true
            owner.attach(second)
            owner.setKeepScreenOn(false)

            assertTrue(first.keepScreenOn)
            assertFalse(second.keepScreenOn)
        }
    }

    @Test
    fun reentrantSurfaceViewDetachRemovesChildren() {
        assertReentrantDetach(NativeVideoSurfaceKind.SurfaceView, checkFlags = false)
    }

    @Test
    fun reentrantTextureViewDetachRemovesChildren() {
        assertReentrantDetach(NativeVideoSurfaceKind.TextureView, checkFlags = false)
    }

    @Test
    fun reentrantSurfaceViewDetachReleasesOwnedFlags() {
        assertReentrantDetach(NativeVideoSurfaceKind.SurfaceView, checkFlags = true)
    }

    @Test
    fun reentrantTextureViewDetachReleasesOwnedFlags() {
        assertReentrantDetach(NativeVideoSurfaceKind.TextureView, checkFlags = true)
    }

    @Test
    fun reentrantSurfaceViewDetachReleasesRenderFlag() {
        assertReentrantDetach(NativeVideoSurfaceKind.SurfaceView, checkFlags = true, checkRenderOnly = true)
    }

    @Test
    fun reentrantTextureViewDetachReleasesRenderFlag() {
        assertReentrantDetach(NativeVideoSurfaceKind.TextureView, checkFlags = true, checkRenderOnly = true)
    }

    @Test
    fun reentrantSurfaceViewOutputPathDetachReleasesOwnedFlags() {
        assertReentrantDetach(NativeVideoSurfaceKind.SurfaceView, checkFlags = true, outputSync = true)
    }

    @Test
    fun reentrantTextureViewOutputPathDetachReleasesOwnedFlags() {
        assertReentrantDetach(NativeVideoSurfaceKind.TextureView, checkFlags = true, outputSync = true)
    }

    @Test
    fun reentrantDetachWithoutNativeNotificationReleasesOwnedFlags() {
        NativeVideoSurfaceKind.entries.forEach { kind ->
            assertReentrantDetach(kind, checkFlags = true, notifyNative = false)
        }
    }

    private fun assertReentrantDetach(
        kind: NativeVideoSurfaceKind,
        checkFlags: Boolean,
        checkRenderOnly: Boolean = false,
        outputSync: Boolean = false,
        notifyNative: Boolean = true,
    ) {
        var outputSyncOwner: VesperNativeSurfaceHost? = null
        var outputCallbacks = 0
        withHosts(kind, onOutputPathChanged = { path ->
            if (path == null && outputSyncOwner != null) {
                outputCallbacks += 1
                outputSyncOwner?.setKeepScreenOn(true)
            }
        }) { owner, first, second ->
            owner.attach(first)
            owner.setKeepScreenOn(true)
            val render = first.getChildAt(0)
            val subtitle = first.getChildAt(1)
            assertTrue("The render child must be attached to a visible window", render.isAttachedToWindow)
            assertTrue(render.isShown)
            assertTrue(first.keepScreenOn)
            assertTrue(render.keepScreenOn)
            var detachCallbacks = 0
            var renderFlagDuringCallback = false
            val listener = object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(view: View) = Unit

                override fun onViewDetachedFromWindow(view: View) {
                    detachCallbacks += 1
                    // A still-playing native state refresh synchronously reenters the owner.
                    owner.setKeepScreenOn(true)
                    renderFlagDuringCallback = render.keepScreenOn
                }
            }
            if (outputSync) outputSyncOwner = owner else render.addOnAttachStateChangeListener(listener)
            try {
                // A stale expectedHost must not trigger any callback or alter current ownership.
                owner.detach(expectedHost = second)
                assertEquals(0, detachCallbacks)
                assertEquals(0, outputCallbacks)
                assertSame(first, render.parent)
                assertSame(subtitle, first.getChildAt(1))
                assertTrue(first.keepScreenOn)
                assertTrue(render.keepScreenOn)
                if (notifyNative) owner.detach(expectedHost = first)
                else owner.detachWithoutNativeNotification(expectedHost = first)
            } finally {
                outputSyncOwner = null
                render.removeOnAttachStateChangeListener(listener)
            }
            if (outputSync) {
                assertEquals("The output-path reset must synchronously reenter once", 1, outputCallbacks)
            } else {
                assertEquals("The real render child must synchronously detach once", 1, detachCallbacks)
                assertTrue("The regression must restore the render flag during the callback", renderFlagDuringCallback)
            }
            assertEquals("Detach must remove all host children", 0, first.childCount)
            assertNull("The render child must leave its host", render.parent)
            assertNull("The subtitle child must leave its host", subtitle.parent)
            if (!checkFlags) return@withHosts
            if (!checkRenderOnly) {
                assertFalse("Detach must release the previous host's screen request", first.keepScreenOn)
            }
            assertFalse("Detach must release the render child's own screen request", render.keepScreenOn)

            // Disposal after public detach no longer has a host reference to repair flags.
            owner.close()
            owner.detach(expectedHost = first)
            owner.detach()
            owner.detach()
            assertFalse(first.keepScreenOn)
            assertFalse(render.keepScreenOn)
            assertEquals(0, first.childCount)

            owner.attach(second)
            val replacement = second.getChildAt(0)
            assertEquals(2, second.childCount)
            assertFalse(second.keepScreenOn)
            assertFalse(replacement.keepScreenOn)
            owner.setKeepScreenOn(true)
            owner.detach(expectedHost = first)
            assertSame(second, replacement.parent)
            assertTrue(second.keepScreenOn)
            assertTrue(replacement.keepScreenOn)
            owner.detach(expectedHost = second)
            assertEquals(0, second.childCount)
            assertFalse(second.keepScreenOn)
            assertFalse(replacement.keepScreenOn)
        }
    }

    private fun assertPlayingMigration(kind: NativeVideoSurfaceKind) {
        withHosts(kind) { owner, first, second ->
            owner.attach(first)
            owner.setKeepScreenOn(true)
            val render = first.getChildAt(0)
            val subtitle = first.getChildAt(1)
            assertTrue(first.keepScreenOn)
            owner.attach(second)
            // The public bridge refreshes and syncs playback state after attach.
            owner.setKeepScreenOn(true)

            assertSame(render, second.getChildAt(0))
            assertSame(subtitle, second.getChildAt(1))
            assertSame(second, render.parent)
            assertEquals(0, first.childCount)
            assertTrue(first.isShown)
            assertTrue(second.isShown)
            assertFalse("Previous visible host must release its SDK screen request", first.keepScreenOn)
            assertTrue(second.keepScreenOn)
            assertTrue(render.keepScreenOn)
            owner.setKeepScreenOn(false)
            assertFalse(first.keepScreenOn)
            assertFalse(second.keepScreenOn)
            assertFalse(render.keepScreenOn)
        }
    }

    private fun forEachKind(block: (VesperNativeSurfaceHost, FrameLayout, FrameLayout) -> Unit) {
        NativeVideoSurfaceKind.entries.forEach { kind -> withHosts(kind, block = block) }
    }

    private fun withHosts(
        kind: NativeVideoSurfaceKind,
        onOutputPathChanged: (String?) -> Unit = {},
        block: (VesperNativeSurfaceHost, FrameLayout, FrameLayout) -> Unit,
    ) {
        val activityController = Robolectric.buildActivity(Activity::class.java).setup().visible()
        val activity = activityController.get()
        val root = LinearLayout(activity).apply { orientation = LinearLayout.HORIZONTAL }
        val first = FrameLayout(activity)
        val second = FrameLayout(activity)
        root.addView(first, LinearLayout.LayoutParams(400, 240))
        root.addView(second, LinearLayout.LayoutParams(400, 240))
        activity.setContentView(root)
        root.measure(
            View.MeasureSpec.makeMeasureSpec(800, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(240, View.MeasureSpec.EXACTLY),
        )
        root.layout(0, 0, 800, 240)
        val owner = VesperNativeSurfaceHost(MissingVesperNativeBindings(), kind, onOutputPathChanged)
        try {
            assertTrue(first.isAttachedToWindow)
            assertTrue(second.isAttachedToWindow)
            assertTrue(first.isShown)
            assertTrue(second.isShown)
            block(owner, first, second)
        } finally {
            owner.close()
            owner.detach()
            activityController.pause().stop().destroy()
        }
    }
}
