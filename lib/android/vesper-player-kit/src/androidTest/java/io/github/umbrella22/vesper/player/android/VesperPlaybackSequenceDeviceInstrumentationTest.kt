package io.github.umbrella22.vesper.player.android

import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.withTimeout
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
        val sourceSession = VesperSourceSession(context)
        val firstItem = item(sourceSession, "item-a", "device item A", firstFile)
        val secondItem = item(sourceSession, "item-b", "device item B", secondFile)

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
                        )
                    }
                    val activeController = requireNotNull(controller)
                    val activeSequence = requireNotNull(sequence)

                    runBlocking { activeSequence.activate(firstItem.itemId) }
                    awaitPlayback(activeController, activeSequence, firstItem.itemId, "device item A")
                    awaitWarmup(activeSequence, expectedCompleted = 2L)

                    runBlocking { activeSequence.next() }
                    awaitPlayback(activeController, activeSequence, secondItem.itemId, "device item B")

                    runBlocking { activeSequence.previous() }
                    awaitPlayback(activeController, activeSequence, firstItem.itemId, "device item A")

                    val warmup = activeSequence.warmupSnapshot()
                    assertEquals(2L, warmup.completedJobs)
                    assertEquals(0L, warmup.failedJobs)
                    assertEquals(0L, warmup.cancelledJobs)
                    assertEquals(0L, warmup.unsupportedJobs)
                    assertEquals(2L, warmup.cacheMisses)
                    assertEquals(0, warmup.activeJobs)
                    assertTrue(warmup.actualBytes > 0L)
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
            sourceSession.close()
            fixtureRoot.deleteRecursively()
        }
    }

    @Test
    fun sameHandleReorderPreservesResolvedRevisionTwoWithoutPlayback() {
        withMetadataSequence { scenario, controller, sequence, session ->
            val unresolved = unresolvedItem("item-a")
            val other = unresolvedItem("item-b")
            val handle = session.register(VesperPlayerSource(
                uri = "file:///unused-sequence-metadata-fixture.mp4",
                label = "must not play",
                kind = VesperPlayerSourceKind.Local,
                protocol = VesperPlayerSourceProtocol.Progressive,
            ))
            val originalLabel = controller.uiState.value.sourceLabel
            val originalEpoch = controller.playbackDiagnostics?.value?.playbackEpoch
            scenario.onActivity { sequence.replace(listOf(unresolved, other)) }
            runBlocking {
                val navigation = async(Dispatchers.Default) { sequence.activate(unresolved.itemId) }
                try {
                    val firstRequest = awaitSourceRequest(sequence, unresolved.itemId)
                    // Cancel the explicit playback continuation, leaving its native resolution request.
                    navigation.cancelAndJoin()
                    scenario.onActivity { sequence.submitResolvedSource(firstRequest, handle) }
                } finally {
                    navigation.cancelAndJoin()
                }
            }
            assertEquals(1L, sequence.snapshot.value.items.first { it.itemId == unresolved.itemId }.sourceRevision)
            scenario.onActivity { sequence.markSourceExpired(unresolved.itemId, 1L) }
            val secondRequest = awaitSourceRequest(sequence, unresolved.itemId)
            scenario.onActivity { sequence.submitResolvedSource(secondRequest, handle) }
            assertEquals(2L, sequence.snapshot.value.items.first { it.itemId == unresolved.itemId }.sourceRevision)
            scenario.onActivity { sequence.replace(listOf(other, unresolved.copy(source = handle))) }
            assertEquals(listOf("item-b", "item-a"), sequence.snapshot.value.items.map { it.itemId })
            assertEquals(2L, sequence.snapshot.value.items.first { it.itemId == unresolved.itemId }.sourceRevision)
            assertEquals(originalLabel, controller.uiState.value.sourceLabel)
            assertEquals(originalEpoch, controller.playbackDiagnostics?.value?.playbackEpoch)
        }
    }

    @Test
    fun replacementAndRemovalSettlePendingNavigationWithoutPlayback() {
        withMetadataSequence { scenario, controller, sequence, _ ->
            val originalLabel = controller.uiState.value.sourceLabel
            val originalEpoch = controller.playbackDiagnostics?.value?.playbackEpoch
            for (replace in listOf(true, false)) {
                val unresolved = unresolvedItem("item-a")
                scenario.onActivity { sequence.replace(listOf(unresolved)) }
                runBlocking {
                    val navigation = async(Dispatchers.Default) {
                        runCatching { sequence.activate(unresolved.itemId) }
                    }
                    try {
                        awaitSourceRequest(sequence, unresolved.itemId)
                        scenario.onActivity {
                            if (replace) sequence.replace(listOf(unresolvedItem("item-b")))
                            else assertTrue(sequence.remove(unresolved.itemId))
                        }
                        val failure = runCatching { withTimeout(2_000L) { navigation.await() } }
                        assertTrue("pending navigation must settle before timeout", navigation.isCompleted)
                        assertTrue("superseded navigation must fail", failure.isFailure || failure.getOrThrow().isFailure)
                        val error = failure.exceptionOrNull() ?: failure.getOrThrow().exceptionOrNull()
                        assertTrue("unexpected cancellation reason: $error",
                            error?.message in setOf("activation_superseded", "activation_item_removed"))
                    } finally {
                        navigation.cancelAndJoin()
                    }
                }
                assertEquals(originalLabel, controller.uiState.value.sourceLabel)
                assertEquals(originalEpoch, controller.playbackDiagnostics?.value?.playbackEpoch)
            }
        }
    }

    private fun unresolvedItem(id: String) = VesperPlaybackSequenceItem(
        itemId = id,
        contentIdentity = VesperPlaybackSequenceContentIdentity("device.metadata", id),
    )

    private fun awaitSourceRequest(
        sequence: VesperPlaybackSequence,
        itemId: String,
    ): VesperPlaybackSequenceSourceRequest {
        fun requestMap() = sequence.snapshot.value.pendingRequests.map { pending ->
            @Suppress("UNCHECKED_CAST")
            (pending["request"] as? Map<String, Any?>) ?: pending
        }.firstOrNull { it["type"] == "sourceResolutionRequired" && it["itemId"] == itemId }
        assertTrue("source resolution request did not arrive", awaitCondition(5) { requestMap() != null })
        return VesperPlaybackSequenceSourceRequest.fromWireMap(requireNotNull(requestMap()))
    }

    private fun withMetadataSequence(
        block: (ActivityScenario<VesperSurfaceLayoutTestActivity>, VesperPlayerController,
            VesperPlaybackSequence, VesperSourceSession) -> Unit,
    ) {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val session = VesperSourceSession(context, VesperSourceSessionConfiguration(maxMemoryBytes = 0))
        var controller: VesperPlayerController? = null
        var sequence: VesperPlaybackSequence? = null
        try {
            ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
                try {
                    scenario.onActivity { activity ->
                        controller = VesperPlayerControllerFactory.createDefault(context = activity.applicationContext)
                        sequence = VesperPlaybackSequence(VesperPlaybackSequenceConfiguration(
                            sequenceId = "android-device-metadata-sequence", forwardWindow = 0,
                        )).also { it.attach(requireNotNull(controller)) }
                    }
                    block(scenario, requireNotNull(controller), requireNotNull(sequence), session)
                } finally {
                    scenario.onActivity {
                        sequence?.dispose()
                        controller?.dispose()
                    }
                }
            }
        } finally {
            session.close()
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
        session: VesperSourceSession,
        itemId: String,
        label: String,
        file: File,
    ): VesperPlaybackSequenceItem {
        return VesperPlaybackSequenceItem(
            itemId = itemId,
            contentIdentity =
                VesperPlaybackSequenceContentIdentity(
                    providerNamespace = "device.fixture",
                    value = itemId,
                ),
            source =
                session.register(VesperPlayerSource(
                    uri = Uri.fromFile(file).toString(),
                    label = label,
                    kind = VesperPlayerSourceKind.Local,
                    protocol = VesperPlayerSourceProtocol.Progressive,
                )),
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
