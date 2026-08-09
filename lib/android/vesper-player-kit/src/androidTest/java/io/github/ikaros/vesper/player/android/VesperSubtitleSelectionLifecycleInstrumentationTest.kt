package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.SystemClock
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.yield
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Device-time coverage for subtitle transaction bounds and source-epoch
 * cancellation. Media3 cue delivery is covered separately by the host and
 * Flutter device tests; these cases need a deferred confirmation boundary.
 */
@RunWith(AndroidJUnit4::class)
class VesperSubtitleSelectionLifecycleInstrumentationTest {
    private var bridge: VesperNativePlayerBridge? = null

    @After
    fun tearDown() {
        bridge?.dispose()
        bridge = null
    }

    @Test
    fun pendingSubtitleSelectionTimesOutAgainstTheDeviceClock() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val trackId = "device-timeout-subtitle"
        val bindings = DeviceDeferredSubtitleBindings(subtitleCatalog(trackId))
        val activeBridge = activeBridge(context, bindings, "Timeout source")
        bridge = activeBridge

        val startedAtMs = SystemClock.elapsedRealtime()
        val failure =
            runBlocking {
                try {
                    activeBridge.setSubtitleTrackSelection(VesperTrackSelection.track(trackId))
                    throw AssertionError("Expected the deferred subtitle selection to time out.")
                } catch (error: VesperPlayerUnsupportedOperation) {
                    error
                }
            }
        val elapsedMs = SystemClock.elapsedRealtime() - startedAtMs

        assertEquals("subtitle_selection_timeout", failure.details["code"])
        assertTransactionIdentity(failure)
        assertTrue("selection timeout fired too early after ${elapsedMs}ms", elapsedMs >= 2_500L)
        assertTrue("selection timeout was not bounded after ${elapsedMs}ms", elapsedMs < 8_000L)
        assertEquals(VesperTrackSelection.disabled(), activeBridge.confirmedSubtitleSelection.value)
        assertNull(activeBridge.effectiveSubtitleTrackId.value)
        assertEquals(
            VesperSubtitleSelectionState.Failed,
            activeBridge.subtitleState.value.selectionState,
        )
        assertEquals(
            "subtitle_selection_timeout",
            activeBridge.subtitleState.value.selectionError?.code,
        )

        bindings.confirmDeferredSelection()
        activeBridge.refreshFromNative()

        assertEquals(VesperTrackSelection.disabled(), activeBridge.confirmedSubtitleSelection.value)
        assertNull(activeBridge.effectiveSubtitleTrackId.value)
    }

    @Test
    fun sourceSwitchCancelsPendingSubtitleSelectionOnTheDevice() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val firstTrackId = "device-source-a"
        val secondTrackId = "device-source-b"
        val bindings = DeviceDeferredSubtitleBindings(subtitleCatalog(firstTrackId))
        val activeBridge = activeBridge(context, bindings, "First source")
        bridge = activeBridge

        val selection = async(SupervisorJob()) {
            activeBridge.setSubtitleTrackSelection(VesperTrackSelection.track(firstTrackId))
        }
        yield()
        assertTrue(
            "subtitle selection did not reach the deferred native boundary",
            bindings.firstSelectionIssued.await(1, TimeUnit.SECONDS),
        )

        bindings.replaceCatalog(subtitleCatalog(secondTrackId))
        val replacement = source("Second source")
        activeBridge.selectSourceAsync(replacement)

        val failure =
            try {
                selection.await()
                throw AssertionError("Expected source selection to cancel the subtitle request.")
            } catch (error: VesperPlayerUnsupportedOperation) {
                error
            }
        assertEquals("subtitle_source_changed", failure.details["code"])
        assertTransactionIdentity(failure)
        assertEquals(replacement, bindings.initializedSource)
        assertEquals(VesperTrackSelection.disabled(), activeBridge.confirmedSubtitleSelection.value)
        assertNull(activeBridge.effectiveSubtitleTrackId.value)
        assertEquals(
            VesperSubtitleSelectionState.Idle,
            activeBridge.subtitleState.value.selectionState,
        )
    }

    @Test
    fun newerSubtitleSelectionSupersedesPendingCommandOnTheDevice() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val firstTrackId = "device-supersede-a"
        val secondTrackId = "device-supersede-b"
        val bindings = DeviceDeferredSubtitleBindings(subtitleCatalog(firstTrackId, secondTrackId))
        val activeBridge = activeBridge(context, bindings, "Supersede source")
        bridge = activeBridge

        val firstSelection = async(SupervisorJob()) {
            activeBridge.setSubtitleTrackSelection(VesperTrackSelection.track(firstTrackId))
        }
        yield()
        assertTrue(
            "first subtitle selection did not reach the deferred native boundary",
            bindings.firstSelectionIssued.await(1, TimeUnit.SECONDS),
        )

        val secondSelection = async(SupervisorJob()) {
            activeBridge.setSubtitleTrackSelection(VesperTrackSelection.track(secondTrackId))
        }
        yield()
        assertTrue(
            "second subtitle selection did not reach the deferred native boundary",
            bindings.secondSelectionIssued.await(1, TimeUnit.SECONDS),
        )

        val firstFailure =
            try {
                firstSelection.await()
                throw AssertionError("Expected the newer subtitle command to supersede the first.")
            } catch (error: VesperPlayerUnsupportedOperation) {
                error
            }
        assertEquals("subtitle_selection_superseded", firstFailure.details["code"])
        assertTransactionIdentity(firstFailure)

        bindings.confirmDeferredSelection()
        activeBridge.refreshFromNative()
        secondSelection.await()

        assertEquals(
            VesperTrackSelection.track(secondTrackId),
            activeBridge.confirmedSubtitleSelection.value,
        )
        assertEquals(secondTrackId, activeBridge.effectiveSubtitleTrackId.value)
        assertEquals(
            VesperSubtitleSelectionState.Confirmed,
            activeBridge.subtitleState.value.selectionState,
        )
        assertNull(activeBridge.subtitleState.value.selectionError)
    }

    private fun activeBridge(
        context: Context,
        bindings: DeviceDeferredSubtitleBindings,
        label: String,
    ): VesperNativePlayerBridge =
        VesperNativePlayerBridge(
            bindings = bindings,
            initialSource = source(label),
            appContext = context,
        )

    private fun source(label: String): VesperPlayerSource =
        VesperPlayerSource.local(
            uri = "file:///subtitle-device-lifecycle-${label.replace(' ', '-')}.m4a",
            label = label,
        )

    private fun assertTransactionIdentity(failure: VesperPlayerUnsupportedOperation) {
        val commandId = failure.details["commandId"] as? Long
        assertTrue("subtitle failure did not preserve command identity", commandId != null && commandId > 0L)
        assertEquals(0L, failure.details["sourceEpoch"])
    }
}

private class DeviceDeferredSubtitleBindings(
    initialCatalog: VesperTrackCatalog,
) : VesperNativeBindings by MissingVesperNativeBindings() {
    private var catalog = initialCatalog
    private var currentSelection = VesperTrackSelectionSnapshot()
    private var currentAppliedSelection = VesperTrackSelection.disabled()
    private var deferredSelection: VesperTrackSelection? = null
    private var sourceGeneration = 0L
    private var selectionGeneration = 0L

    private var selectionIssueCount = 0
    val firstSelectionIssued = CountDownLatch(1)
    val secondSelectionIssued = CountDownLatch(1)
    var initializedSource: VesperPlayerSource? = null
        private set

    override val isSystemPlaybackActive: Boolean
        get() = true

    override val sourceCallbackGeneration: Long
        get() = sourceGeneration

    override val trackSelectionChangeGeneration: Long
        get() = selectionGeneration

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
        preparedSourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
    ): NativeBridgeStartup {
        initializedSource = source
        return NativeBridgeStartup()
    }

    override fun invalidateSystemPlaybackCallbacks() {
        sourceGeneration += 1L
    }

    override fun currentTrackCatalog(): VesperTrackCatalog = catalog

    override fun isSubtitleTrackSelectable(trackId: String): Boolean =
        catalog.subtitleTracks.any { it.id == trackId }

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = currentSelection

    override fun currentAppliedSubtitleSelection(): VesperTrackSelection = currentAppliedSelection

    override fun currentAdvertisedSubtitleTrackCount(): Int = catalog.subtitleTracks.size

    override fun isTrackCatalogReady(): Boolean = true

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        deferredSelection = selection
        selectionIssueCount += 1
        if (selectionIssueCount == 1) {
            firstSelectionIssued.countDown()
        } else if (selectionIssueCount == 2) {
            secondSelectionIssued.countDown()
        }
    }

    fun replaceCatalog(replacement: VesperTrackCatalog) {
        catalog = replacement
        currentSelection = VesperTrackSelectionSnapshot()
        currentAppliedSelection = VesperTrackSelection.disabled()
        selectionGeneration += 1L
    }

    fun confirmDeferredSelection() {
        val selection = deferredSelection ?: return
        currentAppliedSelection = selection
        currentSelection = VesperTrackSelectionSnapshot(subtitle = selection)
        selectionGeneration += 1L
    }
}

private fun subtitleCatalog(vararg trackIds: String): VesperTrackCatalog =
    VesperTrackCatalog(
        tracks = trackIds.map { trackId ->
            VesperMediaTrack(id = trackId, kind = VesperMediaTrackKind.Subtitle)
        },
    )
