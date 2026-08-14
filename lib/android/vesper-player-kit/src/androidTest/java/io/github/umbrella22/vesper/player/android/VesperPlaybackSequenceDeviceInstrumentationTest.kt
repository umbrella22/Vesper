package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.net.Uri
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Physical-device proof for progressive VOD sequence activation and warmup. */
@RunWith(AndroidJUnit4::class)
class VesperPlaybackSequenceDeviceInstrumentationTest {
    @Test
    fun progressiveVodActivatesNextPreviousAndWarmsCurrentWindow() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        File(context.cacheDir, "vesper-sequence-cache").deleteRecursively()
        val fixtureRoot = File(context.cacheDir, "vesper-sequence-device-playback")
        fixtureRoot.deleteRecursively()
        check(fixtureRoot.mkdirs()) { "failed to create sequence playback fixture directory" }
        val firstFile = copyFixture(context, File(fixtureRoot, "item-a.m4v"))
        val secondFile = copyFixture(context, File(fixtureRoot, "item-b.m4v"))
        val firstItem = item("item-a", "device item A", firstFile)
        val secondItem = item("item-b", "device item B", secondFile)

        var controller: VesperPlayerController? = null
        var sequence: VesperPlaybackSequence? = null
        try {
            ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
                try {
                    scenario.onActivity { activity ->
                        val surfaceHost: FrameLayout = activity.replaceSurfaceHost()
                        controller =
                            VesperPlayerControllerFactory.createDefault(
                                context = activity.applicationContext,
                                resiliencePolicy = VesperPlaybackResiliencePolicy.streaming(),
                                decoderBackend = VesperDecoderBackend.SystemOnly,
                                surfaceKind = VesperVideoSurfaceKind.SurfaceView,
                                keepScreenOnDuringPlayback = false,
                            ).also { it.attachSurfaceHost(surfaceHost) }
                        sequence =
                            VesperPlaybackSequence(
                                VesperPlaybackSequenceConfiguration(
                                    sequenceId = "android-device-progressive-sequence",
                                    forwardWindow = 1,
                                ),
                            )
                        requireNotNull(sequence).attach(requireNotNull(controller))
                        requireNotNull(sequence).replace(
                            listOf(firstItem, secondItem),
                            activeItemId = firstItem.itemId,
                        )
                    }
                    val activeController = requireNotNull(controller)
                    val activeSequence = requireNotNull(sequence)

                    awaitPlayback(activeController, activeSequence, firstItem.itemId, "device item A")
                    awaitWarmup(activeSequence, expectedCompleted = 2L)

                    scenario.onActivity { activeSequence.next() }
                    awaitPlayback(activeController, activeSequence, secondItem.itemId, "device item B")

                    scenario.onActivity { activeSequence.previous() }
                    awaitPlayback(activeController, activeSequence, firstItem.itemId, "device item A")

                    val warmup = activeSequence.warmupSnapshot()
                    assertEquals(2L, warmup.completedJobs)
                    assertEquals(0L, warmup.failedJobs)
                    assertEquals(0L, warmup.cancelledJobs)
                    assertEquals(0L, warmup.unsupportedJobs)
                    assertEquals(2L, warmup.cacheMisses)
                    assertEquals(0, warmup.activeJobs)
                    assertTrue(warmup.actualBytes > 0L)
                    assertTrue(warmup.cacheEntries >= 2)
                    assertTrue(warmup.cacheBytes > 0L)
                } finally {
                    scenario.onActivity {
                        sequence?.dispose()
                        controller?.dispose()
                        sequence = null
                        controller = null
                    }
                }
            }
        } finally {
            fixtureRoot.deleteRecursively()
        }
    }

    private fun awaitPlayback(
        controller: VesperPlayerController,
        sequence: VesperPlaybackSequence,
        expectedItemId: String,
        expectedLabel: String,
    ) {
        val reached = awaitCondition(20) {
            controller.refresh()
            val state = controller.uiState.value
            sequence.snapshot.value.activeItemId == expectedItemId &&
                state.sourceLabel == expectedLabel &&
                !state.isBuffering &&
                state.lastError == null &&
                (state.timeline.durationMs ?: 0L) > 0L
        }
        val state = controller.uiState.value
        assertTrue(
            "playback did not converge for $expectedItemId: active=${sequence.snapshot.value.activeItemId}, " +
                "label=${state.sourceLabel}, state=${state.playbackState}, buffering=${state.isBuffering}, " +
                "duration=${state.timeline.durationMs}, error=${state.lastError}",
            reached,
        )
        assertNull(state.lastError)
    }

    private fun awaitWarmup(
        sequence: VesperPlaybackSequence,
        expectedCompleted: Long,
    ) {
        val reached = awaitCondition(20) {
            val snapshot = sequence.warmupSnapshot()
            snapshot.completedJobs >= expectedCompleted && snapshot.activeJobs == 0
        }
        val snapshot = sequence.warmupSnapshot()
        assertTrue(
            "warmup did not converge: completed=${snapshot.completedJobs}, failed=${snapshot.failedJobs}, " +
                "cancelled=${snapshot.cancelledJobs}, unsupported=${snapshot.unsupportedJobs}, " +
                "active=${snapshot.activeJobs}, bytes=${snapshot.actualBytes}",
            reached,
        )
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

    private fun item(
        itemId: String,
        label: String,
        file: File,
    ): VesperPlaybackSequenceItem {
        val revision = 1L
        return VesperPlaybackSequenceItem(
            itemId = itemId,
            contentIdentity =
                VesperPlaybackSequenceContentIdentity(
                    providerNamespace = "device.fixture",
                    value = itemId,
                ),
            source =
                VesperPlayerSource(
                    uri = Uri.fromFile(file).toString(),
                    label = label,
                    kind = VesperPlayerSourceKind.Local,
                    protocol = VesperPlayerSourceProtocol.Progressive,
                ),
            cacheIdentity =
                VesperPlaybackSequenceCacheIdentity(
                    providerNamespace = "device.fixture",
                    contentIdentity = itemId,
                    renditionIdentity = "h264-aac-128x96",
                    resourceIdentity = "progressive-file",
                    accessPartition = "instrumentation",
                    sourceRevision = revision,
                ),
            sourceRevision = revision,
            preloadProfile =
                VesperPlaybackSequencePreloadProfile(
                    expectedDiskBytes = 64L * 1024L,
                    warmupWindowMs = 10_000L,
                ),
        )
    }

    private fun copyFixture(
        context: Context,
        destination: File,
    ): File {
        context.assets.open("tiny-h264-aac-mediacodec.m4v").use { input ->
            destination.outputStream().use(input::copyTo)
        }
        return destination
    }
}
