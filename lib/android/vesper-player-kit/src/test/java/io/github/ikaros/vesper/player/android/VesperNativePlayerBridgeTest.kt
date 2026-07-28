package io.github.ikaros.vesper.player.android

import android.view.Surface
import androidx.media3.common.C
import androidx.media3.common.ColorInfo
import androidx.media3.common.Format
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativePlayerBridgeTest {
    @Test
    fun dashSubtitleStateStaysLoadingUntilMedia3ReportsTracks() {
        val bindings = FakeBindings().apply { trackCatalogReady = false }
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )

        bridge.refreshFromNative()

        assertEquals(VesperSubtitleStatus.Loading, bridge.subtitleState.value.status)

        bindings.trackCatalogReady = true
        bridge.refreshFromNative()

        assertEquals(VesperSubtitleStatus.Unavailable, bridge.subtitleState.value.status)
    }

    @Test
    fun progressiveSubtitleStateStaysLoadingUntilMedia3ReportsTracks() {
        val trackId = "external-en"
        val bindings =
            FakeBindings().apply {
                trackCatalogReady = false
                advertisedSubtitleTrackCount = 1
            }
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.remote("https://example.com/video.mp4", "Video"),
            )

        bridge.refreshFromNative()

        val loading = bridge.subtitleState.value
        assertEquals(VesperSubtitleCatalogState.Loading, loading.catalogState)
        assertEquals(1, loading.advertisedTrackCount)
        assertEquals(0, loading.selectableTrackCount)
        assertNull(loading.catalogError)

        bindings.trackCatalogReady = true
        bindings.trackCatalog = subtitleCatalog(trackId)
        bridge.refreshFromNative()

        val ready = bridge.subtitleState.value
        assertEquals(VesperSubtitleCatalogState.Ready, ready.catalogState)
        assertEquals(1, ready.advertisedTrackCount)
        assertEquals(1, ready.selectableTrackCount)
        assertNull(ready.catalogError)
    }

    @Test
    fun validSubtitleCommandClearsPreviousSelectionFailure() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "subtitle:dash:sub-en",
                                    kind = VesperMediaTrackKind.Subtitle,
                                ),
                            ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )
        bridge.refreshFromNative()
        bridge._subtitleState.value =
            VesperSubtitleState.failed(
                advertisedTrackCount = 1,
                code = "subtitle_track_not_found",
                phase = VesperSubtitleErrorPhase.Selection,
                message = "stale selection",
            )

        runBlocking { bridge.setSubtitleTrackSelection(VesperTrackSelection.disabled()) }

        assertEquals(VesperSubtitleStatus.Ready, bridge.subtitleState.value.status)
        assertNull(bridge.subtitleState.value.error)
    }

    @Test
    fun subtitleSelectionPublishesConfirmedAndEffectiveStateAfterNativeCallback() {
        val trackId = "subtitle:dash:sub-en"
        val catalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = trackId,
                            kind = VesperMediaTrackKind.Subtitle,
                        ),
                    ),
            )
        val bindings = FakeBindings(trackCatalog = catalog)
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )

        runBlocking {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(trackId))
        }

        assertEquals(VesperTrackSelection.track(trackId), bridge.requestedSubtitleSelection.value)
        assertEquals(VesperTrackSelection.track(trackId), bridge.confirmedSubtitleSelection.value)
        assertEquals(trackId, bridge.effectiveSubtitleTrackId.value)
        assertEquals("selection confirmation must not rewrite the catalog", catalog, bridge.trackCatalog.value)
    }

    @Test
    fun pausedSubtitleSelectionConfirmsAppliedOverrideBeforeRendererActivation() {
        val trackId = "subtitle:dash:sub-en"
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = trackId,
                                    kind = VesperMediaTrackKind.Subtitle,
                                ),
                            ),
                    ),
            ).apply {
                confirmAppliedSubtitleSelectionWithoutRenderer = true
            }
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )

        runBlocking {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(trackId))
        }

        assertEquals(VesperTrackSelection.track(trackId), bridge.confirmedSubtitleSelection.value)
        assertNull(bridge.effectiveSubtitleTrackId.value)

        bindings.trackSelection =
            bindings.trackSelection.copy(subtitle = VesperTrackSelection.track(trackId))
        bridge.refreshFromNative()

        assertEquals(trackId, bridge.effectiveSubtitleTrackId.value)
    }

    @Test
    fun subtitleSelectionFailureRetainsConfirmedStateAndCatalog() {
        val previousId = "subtitle:dash:sub-en"
        val requestedId = "subtitle:dash:sub-zh"
        val catalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = previousId,
                            kind = VesperMediaTrackKind.Subtitle,
                        ),
                        VesperMediaTrack(
                            id = requestedId,
                            kind = VesperMediaTrackKind.Subtitle,
                        ),
                    ),
            )
        val bindings = FakeBindings(trackCatalog = catalog)
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )

        runBlocking {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(previousId))
        }
        bindings.subtitleSelectionFailure =
            NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = requestedId,
                code = "subtitle_track_not_found",
                phase = "selection",
                message = "requested subtitle is unavailable",
            )

        val error =
            org.junit.Assert.assertThrows(VesperPlayerUnsupportedOperation::class.java) {
                runBlocking {
                    bridge.setSubtitleTrackSelection(VesperTrackSelection.track(requestedId))
                }
            }

        assertEquals("subtitle_track_not_found", error.details["code"])
        assertEquals(VesperTrackSelection.track(previousId), bridge.confirmedSubtitleSelection.value)
        assertEquals(previousId, bridge.effectiveSubtitleTrackId.value)
        assertEquals(catalog, bridge.trackCatalog.value)
    }

    @Test
    fun unknownSubtitleRequestFailsInsideTransactionAndRetainsConfirmation() {
        val previousId = "subtitle:dash:sub-en"
        val missingId = "subtitle:dash:missing"
        val catalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = previousId,
                            kind = VesperMediaTrackKind.Subtitle,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = FakeBindings(trackCatalog = catalog),
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )
        runBlocking {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(previousId))
        }

        val error =
            org.junit.Assert.assertThrows(VesperPlayerUnsupportedOperation::class.java) {
                runBlocking {
                    bridge.setSubtitleTrackSelection(VesperTrackSelection.track(missingId))
                }
            }

        assertEquals("subtitle_track_not_found", error.details["code"])
        assertEquals(VesperTrackSelection.track(missingId), bridge.requestedSubtitleSelection.value)
        assertEquals(VesperTrackSelection.track(previousId), bridge.confirmedSubtitleSelection.value)
        assertEquals(previousId, bridge.effectiveSubtitleTrackId.value)
        assertEquals(VesperSubtitleSelectionState.Failed, bridge.subtitleState.value.selectionState)
        assertEquals(missingId, bridge.subtitleState.value.selectionError?.trackId)
        assertEquals(catalog, bridge.trackCatalog.value)
    }

    @Test
    fun subtitleSelectionWaitsForNativeConfirmationCallback() = runBlocking {
        val trackId = "subtitle:dash:sub-en"
        val bindings =
            FakeBindings(trackCatalog = subtitleCatalog(trackId)).apply {
                deferSubtitleSelectionConfirmation = true
            }
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        val request = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(trackId))
        }
        kotlinx.coroutines.yield()

        assertFalse(request.isCompleted)
        assertEquals(VesperTrackSelection.track(trackId), bridge.requestedSubtitleSelection.value)
        assertEquals(VesperTrackSelection.disabled(), bridge.confirmedSubtitleSelection.value)
        assertEquals(VesperSubtitleSelectionState.Applying, bridge.subtitleState.value.selectionState)

        bindings.confirmDeferredSubtitleSelection()
        request.await()

        assertEquals(VesperTrackSelection.track(trackId), bridge.confirmedSubtitleSelection.value)
        assertEquals(trackId, bridge.effectiveSubtitleTrackId.value)
        assertEquals(VesperSubtitleSelectionState.Confirmed, bridge.subtitleState.value.selectionState)
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun subtitleSelectionTimeoutUsesBoundedVirtualClockAndRetainsConfirmedState() = runTest {
        val confirmedId = "subtitle:dash:sub-en"
        val timedOutId = "subtitle:dash:sub-zh"
        val catalog = subtitleCatalog(confirmedId, timedOutId)
        val bindings = FakeBindings(trackCatalog = catalog)
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge._requestedSubtitleSelection.value = VesperTrackSelection.track(confirmedId)
        bridge._confirmedSubtitleSelection.value = VesperTrackSelection.track(confirmedId)
        bridge._effectiveSubtitleTrackId.value = confirmedId
        bridge._trackSelection.value =
            VesperTrackSelectionSnapshot(
                subtitle = VesperTrackSelection.track(confirmedId),
                confirmedSubtitle = VesperTrackSelection.track(confirmedId),
                effectiveSubtitleTrackId = confirmedId,
            )
        bindings.deferSubtitleSelectionConfirmation = true
        val request = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(timedOutId))
        }
        runCurrent()

        assertFalse(request.isCompleted)
        assertEquals(VesperTrackSelection.track(timedOutId), bridge.requestedSubtitleSelection.value)
        assertEquals(VesperTrackSelection.track(confirmedId), bridge.confirmedSubtitleSelection.value)
        assertEquals(confirmedId, bridge.effectiveSubtitleTrackId.value)
        assertEquals(VesperSubtitleSelectionState.Applying, bridge.subtitleState.value.selectionState)

        advanceTimeBy(3_001)
        runCurrent()
        val error =
            try {
                request.await()
                throw AssertionError("Expected the bounded subtitle selection timeout.")
            } catch (error: VesperPlayerUnsupportedOperation) {
                error
            }

        assertEquals("subtitle_selection_timeout", error.details["code"])
        assertEquals(VesperTrackSelection.track(confirmedId), bridge.confirmedSubtitleSelection.value)
        assertEquals(confirmedId, bridge.effectiveSubtitleTrackId.value)
        assertEquals(catalog, bridge.trackCatalog.value)
        assertEquals(VesperSubtitleSelectionState.Failed, bridge.subtitleState.value.selectionState)
        assertEquals("subtitle_selection_timeout", bridge.subtitleState.value.selectionError?.code)
        assertEquals(timedOutId, bridge.subtitleState.value.selectionError?.trackId)

        // A callback arriving after the transaction deadline belongs to the
        // expired command and must not rewrite the last confirmed selection.
        bindings.confirmDeferredSubtitleSelection()
        assertEquals(VesperTrackSelection.track(confirmedId), bridge.confirmedSubtitleSelection.value)
        assertEquals(confirmedId, bridge.effectiveSubtitleTrackId.value)
        assertEquals(VesperSubtitleSelectionState.Failed, bridge.subtitleState.value.selectionState)
        assertEquals("subtitle_selection_timeout", bridge.subtitleState.value.selectionError?.code)
    }

    @Test
    fun lateNativeCallbackFromSupersededCommandCannotRewriteConfirmedEffectiveState() = runBlocking {
        val firstId = "subtitle:dash:sub-en"
        val secondId = "subtitle:dash:sub-zh"
        val bindings =
            FakeBindings(trackCatalog = subtitleCatalog(firstId, secondId)).apply {
                deferSubtitleSelectionConfirmation = true
            }
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        val first = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(firstId))
        }
        kotlinx.coroutines.yield()
        first.cancel()
        first.join()

        val second = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(secondId))
        }
        kotlinx.coroutines.yield()
        bindings.confirmDeferredSubtitleSelection()
        second.await()
        assertEquals(secondId, bridge.effectiveSubtitleTrackId.value)

        // A delayed callback from the cancelled command can arrive on the
        // same Media3 item and callback generation. It must not overwrite the
        // coordinator's last confirmed/effective selection.
        bindings.trackSelection =
            bindings.trackSelection.copy(
                subtitle = VesperTrackSelection.track(firstId),
            )
        bindings.trackSelectionChangeGenerationValue += 1L
        bridge.refreshFromNative()

        assertEquals(VesperTrackSelection.track(secondId), bridge.confirmedSubtitleSelection.value)
        assertEquals(secondId, bridge.effectiveSubtitleTrackId.value)
    }

    @Test
    fun sourceEpochInvalidatesPendingSelectionAndIgnoresOldConfirmation() = runBlocking {
        val trackId = "subtitle:dash:sub-en"
        val bindings =
            FakeBindings(trackCatalog = subtitleCatalog(trackId)).apply {
                deferSubtitleSelectionConfirmation = true
            }
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        val request = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(trackId))
        }
        kotlinx.coroutines.yield()
        bridge.advanceSubtitleSourceEpoch()

        val error =
            try {
                request.await()
                throw AssertionError("Expected the source epoch change to cancel the selection.")
            } catch (error: VesperPlayerUnsupportedOperation) {
                error
            }
        assertEquals("subtitle_source_changed", error.details["code"])
        assertNotNull(error.details["commandId"])
        assertNotNull(error.details["sourceEpoch"])

        bindings.confirmDeferredSubtitleSelection()
        assertEquals(VesperTrackSelection.disabled(), bridge.confirmedSubtitleSelection.value)
        assertNull(bridge.effectiveSubtitleTrackId.value)
        assertEquals(VesperSubtitleSelectionState.Idle, bridge.subtitleState.value.selectionState)
    }

    @Test
    fun newerSubtitleSelectionSupersedesTheSinglePendingCommand() = runBlocking {
        val firstId = "subtitle:dash:sub-en"
        val secondId = "subtitle:dash:sub-zh"
        val bindings =
            FakeBindings(trackCatalog = subtitleCatalog(firstId, secondId)).apply {
                deferSubtitleSelectionConfirmation = true
            }
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        val first = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(firstId))
        }
        kotlinx.coroutines.yield()
        val second = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(secondId))
        }
        kotlinx.coroutines.yield()

        val firstError =
            try {
                first.await()
                throw AssertionError("Expected the first selection to be superseded.")
            } catch (error: VesperPlayerUnsupportedOperation) {
                error
            }
        assertEquals("subtitle_selection_superseded", firstError.details["code"])
        assertEquals(VesperTrackSelection.track(secondId), bridge.requestedSubtitleSelection.value)
        assertFalse(second.isCompleted)

        bindings.confirmDeferredSubtitleSelection()
        second.await()

        assertEquals(VesperTrackSelection.track(secondId), bridge.confirmedSubtitleSelection.value)
        assertEquals(secondId, bridge.effectiveSubtitleTrackId.value)
    }

    @Test
    fun delayedFailureFromSupersededCommandCannotFailNewPendingSelection() = runBlocking {
        val firstId = "subtitle:dash:sub-en"
        val secondId = "subtitle:dash:sub-zh"
        val bindings =
            FakeBindings(trackCatalog = subtitleCatalog(firstId, secondId)).apply {
                deferSubtitleSelectionConfirmation = true
            }
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        val first = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(firstId))
        }
        kotlinx.coroutines.yield()
        val firstCommandGeneration = bindings.subtitleSelectionCommandGeneration
        val second = async(kotlinx.coroutines.SupervisorJob()) {
            bridge.setSubtitleTrackSelection(VesperTrackSelection.track(secondId))
        }
        kotlinx.coroutines.yield()

        bindings.emitSubtitleSelectionFailure(
            NativeTrackSelectionFailure(
                kind = NativeTrackKind.Subtitle,
                trackId = null,
                code = "subtitle_selection_mismatch",
                phase = "selection",
                message = "delayed failure from the superseded command",
            ),
            commandGeneration = firstCommandGeneration,
        )

        assertFalse(second.isCompleted)
        bindings.confirmDeferredSubtitleSelection()
        second.await()
        val firstError =
            try {
                first.await()
                throw AssertionError("Expected the first selection to be superseded.")
            } catch (error: VesperPlayerUnsupportedOperation) {
                error
            }
        assertEquals("subtitle_selection_superseded", firstError.details["code"])
        assertEquals(VesperTrackSelection.track(secondId), bridge.confirmedSubtitleSelection.value)
    }

    @Test
    fun dashIdentityFailurePreservesAdvertisedTrackCount() {
        val bindings =
            FakeBindings().apply {
                subtitleCatalogFailure =
                    NativeTrackSelectionFailure(
                        kind = NativeTrackKind.Subtitle,
                        trackId = "sub-en",
                        code = "subtitle_track_identity_ambiguous",
                        phase = "identity",
                        message = "duplicate representation id",
                        advertisedTrackCount = 2,
                    )
            }
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.dash("https://example.com/video.mpd", "DASH"),
            )

        bridge.refreshFromNative()

        assertEquals(VesperSubtitleStatus.Failed, bridge.subtitleState.value.status)
        assertEquals(2, bridge.subtitleState.value.advertisedTrackCount)
    }

    @Test
    fun subtitleManifestRefreshCanReduceAdvertisedTrackCount() {
        val bindings = FakeBindings(trackCatalog = subtitleCatalog("subtitle:a", "subtitle:b"))
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refreshFromNative()
        assertEquals(2, bridge.subtitleState.value.advertisedTrackCount)

        bindings.trackCatalog = subtitleCatalog("subtitle:b")
        bridge.refreshFromNative()

        assertEquals(1, bridge.subtitleState.value.advertisedTrackCount)
        assertEquals(1, bridge.subtitleState.value.selectableTrackCount)
        assertEquals(VesperSubtitleCatalogState.Ready, bridge.subtitleState.value.catalogState)
    }

    @Test
    fun advertisedButUnsupportedSubtitleCatalogFailsDiscovery() {
        val bindings =
            FakeBindings().apply {
                advertisedSubtitleTrackCount = 1
            }
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refreshFromNative()

        val state = bridge.subtitleState.value
        assertEquals(VesperSubtitleCatalogState.Failed, state.catalogState)
        assertEquals(1, state.advertisedTrackCount)
        assertEquals(0, state.selectableTrackCount)
        assertEquals("subtitle_platform_track_unavailable", state.catalogError?.code)
        assertEquals(VesperSubtitleErrorPhase.Discovery, state.catalogError?.phase)
    }

    @Test
    @OptIn(ExperimentalCoroutinesApi::class)
    fun resilienceRestorePublishesRequestedSelectionOnlyWhenCoordinatorStarts() = runTest {
        Dispatchers.setMain(UnconfinedTestDispatcher(testScheduler))
        val trackId = "subtitle:restored"
        val source = VesperPlayerSource.remote("https://example.com/video.mp4", "video")
        val bindings =
            FakeBindings(
                systemPlaybackActive = true,
                trackCatalog = subtitleCatalog(trackId),
            )
        try {
            lateinit var bridge: VesperNativePlayerBridge
            var requestedAtSeek: VesperTrackSelection? = null
            var confirmedAtSeek: VesperTrackSelection? = null
            var effectiveAtSeek: String? = null
            var selectionStateAtSeek: VesperSubtitleSelectionState? = null
            bindings.onSeekTo = {
                requestedAtSeek = bridge.requestedSubtitleSelection.value
                confirmedAtSeek = bridge.confirmedSubtitleSelection.value
                effectiveAtSeek = bridge.effectiveSubtitleTrackId.value
                selectionStateAtSeek = bridge.subtitleState.value.selectionState
            }
            bridge =
                VesperNativePlayerBridge(
                    bindings = bindings,
                    initialSource = source,
                )

            bridge.restorePlaybackState(
                source = source,
                preservedState =
                    PreservedPlaybackState(
                        positionMs = 500,
                        restorePosition = true,
                        seekToLiveEdge = false,
                        playbackRate = 1f,
                        playbackState = PlaybackStateUi.Paused,
                        shouldResumePlayback = false,
                        videoSelection = VesperTrackSelection.auto(),
                        audioSelection = VesperTrackSelection.auto(),
                        subtitleSelection = VesperTrackSelection.track(trackId),
                        effectiveSubtitleTrackId = trackId,
                        abrPolicy = VesperAbrPolicy.auto(),
                    ),
            )
            advanceUntilIdle()

            assertEquals(VesperTrackSelection.disabled(), requestedAtSeek)
            assertEquals(VesperTrackSelection.track(trackId), confirmedAtSeek)
            assertNull(effectiveAtSeek)
            assertEquals(VesperSubtitleSelectionState.Idle, selectionStateAtSeek)
            assertEquals(VesperTrackSelection.track(trackId), bridge.requestedSubtitleSelection.value)
            assertEquals(VesperTrackSelection.track(trackId), bridge.confirmedSubtitleSelection.value)
            assertEquals(trackId, bridge.effectiveSubtitleTrackId.value)
            assertEquals(VesperSubtitleSelectionState.Confirmed, bridge.subtitleState.value.selectionState)
        } finally {
            Dispatchers.resetMain()
        }
    }

    @Test
    fun benchmarkRecorderDefaultsDisabled() {
        val bridge = VesperNativePlayerBridge(bindings = FakeBindings())

        runBlocking { bridge.initializeAsync() }
        bridge.play()

        assertTrue(bridge.drainBenchmarkEvents().isEmpty())
        assertEquals(0L, bridge.benchmarkSummary().acceptedEvents)
    }

    @Test
    fun benchmarkRecorderDrainsRawEventsAndKeepsSummary() {
        val bridge =
            VesperNativePlayerBridge(
                bindings = FakeBindings(),
                benchmarkRecorder =
                    VesperBenchmarkRecorder(
                        VesperBenchmarkConfiguration(enabled = true),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()

        val events = bridge.drainBenchmarkEvents()
        val eventNames = events.map { it.eventName }.toSet()
        assertTrue(eventNames.contains("initialize_start"))
        assertTrue(eventNames.contains("initialize_without_source"))
        assertTrue(eventNames.contains("play_command"))
        assertTrue(bridge.drainBenchmarkEvents().isEmpty())
        assertEquals(events.size.toLong(), bridge.benchmarkSummary().acceptedEvents)
    }

    @Test
    fun playBeforeInitializeDefersAutoplayWithoutPublishingPlaying() {
        val bindings = FakeBindings(systemPlaybackActive = false)
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live"),
            )

        bridge.play()

        assertEquals(0, bindings.playCount)
        assertTrue(bridge.pendingAutoPlay)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)

        runBlocking { bridge.initializeAsync() }

        assertEquals(1, bindings.playCount)
        assertFalse(bridge.pendingAutoPlay)
        assertEquals(PlaybackStateUi.Playing, bridge.uiState.value.playbackState)
    }

    @Test
    fun refreshDrainsNativeRuntimeWarningsOnce() {
        val bindings = FakeBindings()
        val bridge = VesperNativePlayerBridge(bindings = bindings)
        bindings.events +=
            NativeBridgeEvent.Warning(
                VesperRuntimeWarning(
                    domain = "capability",
                    payload =
                        mapOf(
                            "reason" to "hdrNativeFrameUnsupported",
                            "recommendedPlaybackPath" to "systemPlayer",
                            "hdrKind" to "dolbyVision",
                        ),
                ),
            )

        bridge.refresh()

        val warnings = bridge.drainRuntimeWarnings()
        assertEquals(1, warnings.size)
        assertEquals("capability", warnings.single().domain)
        assertEquals("hdrNativeFrameUnsupported", warnings.single().payload["reason"])
        assertEquals("systemPlayer", warnings.single().payload["recommendedPlaybackPath"])
        assertEquals("dolbyVision", warnings.single().payload["hdrKind"])
        assertTrue(bridge.drainRuntimeWarnings().isEmpty())
    }

    @Test
    fun terminalNativeErrorStopsBufferingAndStoresLastError() {
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = true,
                        isInterrupted = true,
                        timeline = testVodTimeline(positionMs = 1_200L),
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)
        bindings.events +=
            NativeBridgeEvent.Error(
                message = "Widevine license failed",
                codeOrdinal = VesperPlayerErrorCode.BackendFailure.jniOrdinal,
                categoryOrdinal = VesperPlayerErrorCategory.Network.jniOrdinal,
                retriable = true,
                details =
                    mapOf(
                        "reason" to "drmLicenseAcquisitionFailed",
                        "keySystem" to "widevine",
                        "licenseUriHost" to "license.example.com",
                        "attemptsExhausted" to true,
                        "maxAttempts" to 3,
                    ),
            )

        bridge.refresh()

        val uiState = bridge.uiState.value
        assertEquals(PlaybackStateUi.Paused, uiState.playbackState)
        assertFalse(uiState.isBuffering)
        assertFalse(uiState.isInterrupted)
        assertEquals(1, bindings.pauseCount)
        assertEquals("Widevine license failed", uiState.lastError?.message)
        assertEquals(VesperPlayerErrorCode.BackendFailure, uiState.lastError?.code)
        assertEquals(VesperPlayerErrorCategory.Network, uiState.lastError?.category)
        assertEquals(true, uiState.lastError?.retriable)
        assertEquals("widevine", uiState.lastError?.details?.get("keySystem"))
        assertEquals(true, uiState.lastError?.details?.get("attemptsExhausted"))
        assertEquals(3, uiState.lastError?.details?.get("maxAttempts"))
    }

    @Test
    fun retryScheduledDoesNotStoreTerminalLastError() {
        val bindings = FakeBindings()
        val bridge = VesperNativePlayerBridge(bindings = bindings)
        bindings.events += NativeBridgeEvent.RetryScheduled(attempt = 1, delayMs = 1_000L)

        bridge.refresh()

        assertNull(bridge.uiState.value.lastError)
        assertFalse(bridge.uiState.value.isBuffering)
        assertEquals(0, bindings.pauseCount)
    }

    @Test
    fun nonWidevineDrmSourceFailsBeforeNativeInitialization() {
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource =
                    VesperPlayerSource.hls(
                        uri = "https://example.com/drm.m3u8",
                        label = "DRM",
                        drmConfiguration =
                            VesperPlayerDrmConfiguration(
                                keySystem = "fairPlay",
                                licenseUri = "https://license.example.com/fairplay",
                            ),
                    ),
            )

        val error =
            runCatching { runBlocking { bridge.initializeAsync() } }
                .exceptionOrNull() as? VesperPlayerUnsupportedOperation

        assertNotNull(error)
        assertNull(bindings.lastInitializedSource)
        assertEquals("drmUnsupportedKeySystem", error?.details?.get("reason"))
        assertEquals("direct", error?.details?.get("route"))
        assertEquals("fairPlay", error?.details?.get("keySystem"))
        assertFalse(bridge.uiState.value.isBuffering)
        assertEquals(PlaybackStateUi.Paused, bridge.uiState.value.playbackState)
        assertEquals(
            "drmUnsupportedKeySystem",
            bridge.uiState.value.lastError?.details?.get("reason"),
        )
        assertEquals(VesperPlayerErrorCode.Unsupported, bridge.uiState.value.lastError?.code)
        assertEquals(VesperPlayerErrorCategory.Capability, bridge.uiState.value.lastError?.category)
    }

    @Test
    fun widevineDirectDrmSourceInitializesThroughMedia3Route() {
        val bindings = FakeBindings()
        val source =
            VesperPlayerSource.hls(
                uri = "https://example.com/drm.m3u8",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = source,
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(source, bindings.lastInitializedSource)
        assertFalse(bridge.uiState.value.isBuffering)
    }

    @Test
    fun disabledPluginPathsDoNotBlockWidevineDirectDrmRoute() {
        val bindings = FakeBindings()
        val source =
            VesperPlayerSource.dash(
                uri = "https://example.com/drm.mpd",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = source,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.Disabled,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.Disabled,
                        decoderPluginLibraryPaths = listOf("/tmp/libdecoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 3,
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(source, bindings.lastInitializedSource)
        assertTrue(bridge.pluginDiagnostics.isEmpty())
    }

    @Test
    fun widevineRemoteDrmBypassesSourceNormalizerRouteGuardWhenSystemPlaybackHandlesSource() {
        val bindings = FakeBindings()
        val source =
            VesperPlayerSource.dash(
                uri = "https://example.com/drm.mpd",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = source,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreferNormalized,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(source, bindings.lastInitializedSource)
        assertEquals(true, bindings.lastSystemPlaybackUsesSourceNormalizerResource)
        assertEquals(true, bindings.lastSystemPlaybackVideoEnabled)
    }

    @Test
    fun widevineRemoteDrmIsAllowedWithDiagnosticsOnlySourceNormalizer() {
        val bindings = FakeBindings()
        val source =
            VesperPlayerSource.dash(
                uri = "https://example.com/drm.mpd",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = source,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.DiagnosticsOnly,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(source, bindings.lastInitializedSource)
        assertEquals(true, bindings.lastSystemPlaybackUsesSourceNormalizerResource)
        assertEquals(true, bindings.lastSystemPlaybackVideoEnabled)
    }

    @Test
    fun widevineDrmAllowsNativeFrameFallbackToSystemPlayback() {
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val source =
            VesperPlayerSource.dash(
                uri = "https://example.com/drm.mpd",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = source,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(source, bindings.lastInitializedSource)
        assertEquals(true, bindings.lastSystemPlaybackVideoEnabled)
    }

    @Test
    fun widevineDrmRejectsRequiredNativeFrameRoute() {
        val bindings = FakeBindings()
        val source =
            VesperPlayerSource.dash(
                uri = "https://example.com/drm.mpd",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = source,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        val error =
            runCatching { runBlocking { bridge.initializeAsync() } }
                .exceptionOrNull() as? VesperPlayerUnsupportedOperation

        assertNotNull(error)
        assertNull(bindings.lastInitializedSource)
        assertEquals("drmUnsupportedRoute", error?.details?.get("reason"))
        assertEquals("nativeFrame", error?.details?.get("route"))
        assertEquals("widevine", error?.details?.get("keySystem"))
    }

    @Test
    fun widevineMediaItemKeepsLicenseHeadersSeparateFromMediaHeaders() {
        val source =
            VesperPlayerSource.hls(
                uri = "https://example.com/drm.m3u8",
                label = "Widevine",
                headers = mapOf("User-Agent" to "media-client"),
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = "https://license.example.com/widevine",
                        licenseHeaders = mapOf("Authorization" to "Bearer drm-token"),
                        multiSession = true,
                    ),
            )

        val drmConfiguration = checkNotNull(buildWidevineDrmConfiguration(source))

        assertEquals(C.WIDEVINE_UUID, drmConfiguration.scheme)
        assertEquals(
            mapOf("Authorization" to "Bearer drm-token"),
            drmConfiguration.licenseRequestHeaders,
        )
        assertEquals(true, drmConfiguration.multiSession)
        assertFalse(drmConfiguration.licenseRequestHeaders.containsKey("User-Agent"))
    }

    @Test
    fun widevineMediaItemRejectsBlankLicenseUri() {
        val source =
            VesperPlayerSource.hls(
                uri = "https://example.com/drm.m3u8",
                label = "Widevine",
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = " ",
                    ),
            )

        val error =
            runCatching { buildWidevineDrmConfiguration(source) }
                .exceptionOrNull() as? VesperPlayerUnsupportedOperation

        assertNotNull(error)
        assertEquals("drmLicenseUriMissing", error?.details?.get("reason"))
        assertEquals("direct", error?.details?.get("route"))
    }

    @Test
    fun firstFrameWatchdogRouteOnlyEnablesSystemVideoPlayback() {
        val enabled = FirstFrameWatchdogRoute.systemPlayback(videoEnabled = true)
        val disabled = FirstFrameWatchdogRoute.systemPlayback(videoEnabled = false)

        assertTrue(enabled.enabled)
        assertEquals("systemPlayer", enabled.payloadValue)
        assertFalse(disabled.enabled)
        assertEquals("systemPlayer", disabled.payloadValue)
    }

    @Test
    fun defaultRetryPolicyMapsToFiniteMedia3RetryCount() {
        val policy = VesperRetryPolicy().toNativePayload()

        assertEquals(3, media3MinimumRetryCount(policy))
        assertEquals(3, policy.resolvedMaxAttempts())
    }

    @Test
    fun pictureInPictureReadinessAllowsInitializedSystemVideoRoute() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:0",
                                    kind = VesperMediaTrackKind.Video,
                                    width = 1920,
                                    height = 1080,
                                ),
                            ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource =
                    VesperPlayerSource.remote(
                        uri = "https://example.com/video.mp4",
                        label = "Video",
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        val readiness = bridge.pictureInPictureReadiness()
        assertTrue(readiness.isAvailable)
        assertNull(readiness.error)
        assertEquals(1, readiness.diagnostics["videoTrackCount"])
    }

    @Test
    fun pictureInPictureReadinessRejectsActiveNativeFrameRoute() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:0",
                                    kind = VesperMediaTrackKind.Video,
                                ),
                            ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource =
                    VesperPlayerSource.remote(
                        uri = "https://example.com/video.mp4",
                        label = "Video",
                    ),
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreferNormalized,
                        pluginLibraryPaths = listOf("/tmp/libsource.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libdecoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = ManualNativeFramePipelinePumpScheduler(),
            )

        runBlocking { bridge.initializeAsync() }

        val readiness = bridge.pictureInPictureReadiness()
        assertFalse(readiness.isAvailable)
        assertEquals(
            VesperPictureInPictureErrorCode.PictureInPictureNativeFrameRouteCannotHandOff,
            readiness.error?.code,
        )
        assertEquals(true, readiness.diagnostics["nativeFramePipelineActive"])
    }

    @Test
    fun runtimeHdrEvidenceIncludesFormatColorMetadataAndStaticInfo() {
        val hdrStaticInfo =
            ByteArray(25).apply {
                this[21] = 0x03.toByte()
                this[22] = 0xE8.toByte()
                this[23] = 0x01.toByte()
                this[24] = 0x90.toByte()
            }
        val evidence =
            Format.Builder()
                .setCodecs("hvc1.2.4.L153.B0")
                .setSampleMimeType("video/hevc")
                .setWidth(3840)
                .setHeight(2160)
                .setFrameRate(59.94f)
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .setHdrStaticInfo(hdrStaticInfo)
                        .setLumaBitdepth(10)
                        .setChromaBitdepth(10)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        assertNotNull(evidence)
        val diagnostics = checkNotNull(evidence).diagnostics
        assertEquals("hdr10", evidence.hdrKind)
        assertEquals("media3FormatColorInfo", diagnostics["runtimeFormatHdrMetadataProbe"])
        assertEquals("hvc1.2.4.L153.B0", diagnostics["runtimeFormatCodecs"])
        assertEquals("video/hevc", diagnostics["runtimeFormatSampleMimeType"])
        assertEquals("3840", diagnostics["runtimeFormatWidth"])
        assertEquals("2160", diagnostics["runtimeFormatHeight"])
        assertEquals("bt2020", diagnostics["runtimeFormatColorSpace"])
        assertEquals("limited", diagnostics["runtimeFormatColorRange"])
        assertEquals("st2084", diagnostics["runtimeFormatColorTransfer"])
        assertEquals("10", diagnostics["runtimeFormatLumaBitDepth"])
        assertEquals("10", diagnostics["runtimeFormatChromaBitDepth"])
        assertEquals("true", diagnostics["runtimeFormatHdrStaticInfoPresent"])
        assertEquals("25", diagnostics["runtimeFormatHdrStaticInfoByteLength"])
        assertEquals("1000", diagnostics["runtimeFormatMaxContentLightLevelNits"])
        assertEquals("400", diagnostics["runtimeFormatMaxFrameAverageLightLevelNits"])

        val metadata = evidence.metadata
        assertEquals(10, metadata?.lumaBitDepth)
        assertEquals(10, metadata?.chromaBitDepth)
        assertEquals(true, metadata?.hdrStaticInfoPresent)
        assertEquals(25, metadata?.hdrStaticInfoByteLength)
        assertEquals(1000, metadata?.maxContentLightLevelNits)
        assertEquals(400, metadata?.maxFrameAverageLightLevelNits)
    }

    @Test
    fun runtimeHdrEvidenceRecognizesHlgAndDolbyVisionWithoutStaticInfo() {
        val hlgEvidence =
            Format.Builder()
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_HLG)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        assertEquals("hlg", checkNotNull(hlgEvidence).hdrKind)
        assertEquals("hlg", hlgEvidence.diagnostics["runtimeFormatColorTransfer"])
        assertFalse(hlgEvidence.diagnostics.containsKey("runtimeFormatHdrStaticInfoPresent"))

        val dolbyVisionEvidence =
            Format.Builder()
                .setCodecs("dvhe.08.07")
                .build()
                .androidRuntimeHdrEvidence()

        assertEquals("dolbyVision", checkNotNull(dolbyVisionEvidence).hdrKind)
        assertEquals("dvhe.08.07", dolbyVisionEvidence.diagnostics["runtimeFormatCodecs"])
    }

    @Test
    fun runtimeDolbyVisionEvidencePayloadIncludesTypedMetadata() {
        val evidence =
            Format.Builder()
                .setCodecs("dvhe.08.07")
                .setSampleMimeType("video/dolby-vision")
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        val warningPayload = checkNotNull(evidence).capabilityWarningPayload()
        val metadata = warningPayload["hdrMetadata"] as? Map<*, *>

        assertEquals("hdrNativeFrameUnsupported", warningPayload["reason"])
        assertEquals("systemPlayer", warningPayload["recommendedPlaybackPath"])
        assertEquals("dolbyVision", warningPayload["hdrKind"])
        assertEquals("media3FormatColorInfo", evidence.metadata?.probe)
        assertEquals("dolbyVision", metadata?.get("hdrKind"))
        assertEquals("compatibleBaseLayer", metadata?.get("dolbyVisionMode"))
        assertEquals("dvhe.08.07", metadata?.get("dolbyVisionCodec"))
        assertEquals(8, metadata?.get("dolbyVisionProfile"))
        assertEquals(7, metadata?.get("dolbyVisionLevel"))
        assertEquals("profile8Hdr10BaseLayer", metadata?.get("dolbyVisionCompatibility"))
        assertEquals("profile8SingleLayerCompatible", metadata?.get("dolbyVisionProfileFamily"))
        assertEquals("hdr10BaseLayer", metadata?.get("dolbyVisionBaseLayer"))
        assertEquals("hdr10BaseLayerSystemPlayer", metadata?.get("dolbyVisionFallbackTarget"))
        assertEquals("runtimeFormatColorTransfer", metadata?.get("dolbyVisionBaseLayerEvidence"))
        assertEquals("st2084", metadata?.get("dolbyVisionBaseLayerTransferFunction"))
        assertEquals("runtimeFormatColorTransfer", warningPayload["dolbyVisionBaseLayerEvidence"])
        assertEquals("st2084", warningPayload["dolbyVisionBaseLayerTransferFunction"])
        assertEquals("profile8Hdr10BaseLayer", warningPayload["dolbyVisionCompatibility"])
    }

    @Test
    fun runtimeHdrFailurePayloadIncludesTypedEvidenceAndErrorCode() {
        val evidence =
            Format.Builder()
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_HLG)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        val payload =
            checkNotNull(evidence).failureHintPayload(
                "ERROR_CODE_DECODING_FAILED",
                NativePlaybackError(
                    codeOrdinal = DECODE_FAILURE_ORDINAL,
                    categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                    retriable = false,
                    likelyCapabilityIssue = true,
                    capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                    capabilityFailureAxis = AndroidCapabilityFailureAxis.DisplaySurface,
                    causeEvidence =
                        AndroidPlaybackFailureCauseEvidence(
                            causeClass = "android.media.MediaCodec.CodecException",
                            causeMessage = "codec init failed",
                            rootCauseClass = "java.lang.IllegalStateException",
                            rootCauseMessage = "surface rejected",
                        ),
                ),
            )
        val metadata = payload["hdrMetadata"] as? Map<*, *>

        assertEquals(true, payload["likelyHdrCapabilityIssue"])
        assertEquals("sessionProbe", payload["confidence"])
        assertEquals("ERROR_CODE_DECODING_FAILED", payload["errorCode"])
        assertEquals("decodeFailed", payload["capabilityFailureCause"])
        assertEquals("displaySurface", payload["capabilityFailureAxis"])
        assertEquals("android.media.MediaCodec.CodecException", payload["playbackFailureCauseClass"])
        assertEquals("codec init failed", payload["playbackFailureCauseMessage"])
        assertEquals("java.lang.IllegalStateException", payload["playbackFailureRootCauseClass"])
        assertEquals("surface rejected", payload["playbackFailureRootCauseMessage"])
        assertEquals("hlg", payload["hdrKind"])
        assertEquals("hlg", metadata?.get("hdrKind"))
        assertEquals("hlg", metadata?.get("transferFunction"))
        assertEquals("bt2020", metadata?.get("colorSpace"))
        assertEquals("hlg", payload["runtimeFormatColorTransfer"])
    }

    @Test
    fun runtimeHdrFailurePayloadIncludesRendererRuntimeConvergenceDiagnostics() {
        val evidence =
            Format.Builder()
                .setSampleMimeType("video/dolby-vision")
                .setCodecs("dvh1.08.06")
                .setWidth(3840)
                .setHeight(2160)
                .setFrameRate(59.94f)
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        val payload =
            checkNotNull(evidence).failureHintPayload(
                "ERROR_CODE_DECODING_FAILED",
                NativePlaybackError(
                    codeOrdinal = DECODE_FAILURE_ORDINAL,
                    categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                    retriable = false,
                    likelyCapabilityIssue = true,
                    capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                    capabilityFailureAxis = AndroidCapabilityFailureAxis.Renderer,
                    causeEvidence =
                        AndroidPlaybackFailureCauseEvidence(
                            causeClass = "androidx.media3.exoplayer.video.MediaCodecVideoRenderer",
                            causeMessage = "renderer failed",
                            rootCauseClass = null,
                            rootCauseMessage = null,
                            rendererName = "MediaCodecVideoRenderer",
                            rendererIndex = 0,
                            rendererFormatSupport = "handled",
                            rendererFormatSampleMimeType = "video/dolby-vision",
                            rendererFormatCodecs = "dvh1.08.06",
                            rendererFormatWidth = 3840,
                            rendererFormatHeight = 2160,
                            rendererFormatFrameRate = 59.94f,
                        ),
                ),
            )

        assertEquals("renderer", payload["capabilityFailureAxis"])
        assertEquals("MediaCodecVideoRenderer", payload["playbackFailureRendererName"])
        assertEquals("handled", payload["playbackFailureRendererFormatSupport"])
        assertEquals("true", payload["playbackFailureRendererFormatSupported"])
        assertEquals("true", payload["playbackFailureRendererFormatMimeMatchesRuntime"])
        assertEquals("true", payload["playbackFailureRendererFormatCodecsMatchRuntime"])
        assertEquals("true", payload["playbackFailureRendererFormatSizeMatchesRuntime"])
        assertEquals("true", payload["playbackFailureRendererFormatFrameRateMatchesRuntime"])
    }

    @Test
    fun runtimeHdrFailurePayloadIncludesSessionProbeRuntimeConvergenceDiagnostics() {
        val evidence =
            Format.Builder()
                .setSampleMimeType("video/dolby-vision")
                .setCodecs("dvh1.08.06")
                .setWidth(3840)
                .setHeight(2160)
                .setFrameRate(59.94f)
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()
        val sessionProbe =
            AndroidRuntimeSessionProbeSnapshot(
                VesperPlaybackCapabilityProbeResult(
                    status = VesperPlaybackCapabilityProbeStatus.FallbackRequired,
                    codecFamily = VesperPlaybackCodecFamily.Hevc,
                    systemPlaybackSupported = true,
                    hardwareDecodeSupported = true,
                    sdkManagedNativeFrameSupported = false,
                    recommendedPlaybackPath = VesperRecommendedPlaybackPath.SystemPlayer,
                    outputFormat = VesperPlaybackCapabilityOutputFormat.SurfaceOpaque,
                    hdrKind = VesperPlaybackCapabilityHdrKind.DolbyVision,
                    dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
                    confidence = VesperPlaybackCapabilityConfidence.SessionProbe,
                    missingCapabilities = listOf("hdrProgrammableProcessingNotSupported"),
                    diagnostics =
                        mapOf(
                            "codecFormatSupported" to "true",
                            "codecFormatSampleMimeType" to "video/dolby-vision",
                            "codecFormatCodecs" to "dvh1.08.06",
                            "codecFormatWidth" to "3840",
                            "codecFormatHeight" to "2160",
                            "codecFormatFrameRate" to "59.94",
                            "displayHdrSupported" to "true",
                            "displayFrameRateSupported" to "true",
                        ),
                )
            )

        val payload =
            checkNotNull(evidence).failureHintPayload(
                "ERROR_CODE_DECODING_FAILED",
                NativePlaybackError(
                    codeOrdinal = DECODE_FAILURE_ORDINAL,
                    categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                    retriable = false,
                    likelyCapabilityIssue = true,
                    capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                    capabilityFailureAxis = AndroidCapabilityFailureAxis.Renderer,
                ),
                sessionProbe,
            )

        assertEquals("fallbackRequired", payload["runtimeSessionProbeStatus"])
        assertEquals("systemPlayer", payload["runtimeSessionProbeRecommendedPlaybackPath"])
        assertEquals("sessionProbe", payload["runtimeSessionProbeConfidence"])
        assertEquals("dolbyVision", payload["runtimeSessionProbeHdrKind"])
        assertEquals("compatibleBaseLayer", payload["runtimeSessionProbeDolbyVisionMode"])
        assertEquals(
            "hdrProgrammableProcessingNotSupported",
            payload["runtimeSessionProbeMissingCapabilities"],
        )
        assertEquals("true", payload["runtimeSessionProbeCodecFormatSupported"])
        assertEquals("video/dolby-vision", payload["runtimeSessionProbeCodecFormatSampleMimeType"])
        assertEquals("dvh1.08.06", payload["runtimeSessionProbeCodecFormatCodecs"])
        assertEquals("3840", payload["runtimeSessionProbeCodecFormatWidth"])
        assertEquals("2160", payload["runtimeSessionProbeCodecFormatHeight"])
        assertEquals("59.94", payload["runtimeSessionProbeCodecFormatFrameRate"])
        assertEquals("true", payload["runtimeSessionProbeDisplayHdrSupported"])
        assertEquals("true", payload["runtimeSessionProbeDisplayFrameRateSupported"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatMimeMatchesRuntime"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatCodecsMatchRuntime"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatSizeMatchesRuntime"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatFrameRateMatchesRuntime"])
    }

    @Test
    fun runtimeHdrEvidenceIgnoresSdrColorTransfer() {
        val evidence =
            Format.Builder()
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT709)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_SDR)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        assertNull(evidence)
    }

    @Test
    fun surfaceHostAspectFitSizeDoesNotCropWideVideo() {
        val size =
            calculateAspectFitSize(
                containerWidth = 400,
                containerHeight = 300,
                videoWidth = 1920,
                videoHeight = 1080,
            )

        assertEquals(AspectFitSize(width = 400, height = 225), size)
    }

    @Test
    fun surfaceHostAspectFitSizeDoesNotCropPortraitVideo() {
        val size =
            calculateAspectFitSize(
                containerWidth = 400,
                containerHeight = 300,
                videoWidth = 1080,
                videoHeight = 1920,
            )

        assertEquals(AspectFitSize(width = 168, height = 300), size)
    }

    @Test
    fun surfaceHostAspectFitScaleKeepsTextureViewInsideContainer() {
        val wideScale =
            calculateAspectFitScale(
                containerWidth = 400f,
                containerHeight = 300f,
                videoWidth = 1920,
                videoHeight = 1080,
            )
        val portraitScale =
            calculateAspectFitScale(
                containerWidth = 400f,
                containerHeight = 300f,
                videoWidth = 1080,
                videoHeight = 1920,
            )

        assertEquals(1.0f, wideScale?.scaleX)
        assertEquals(0.75f, wideScale?.scaleY)
        assertEquals(0.421875f, portraitScale?.scaleX)
        assertEquals(1.0f, portraitScale?.scaleY)
    }

    @Test
    fun surfaceHostAspectFitRejectsInvalidDimensions() {
        assertNull(
            calculateAspectFitSize(
                containerWidth = 0,
                containerHeight = 300,
                videoWidth = 1920,
                videoHeight = 1080,
            )
        )
        assertNull(
            calculateAspectFitScale(
                containerWidth = 400f,
                containerHeight = 300f,
                videoWidth = 1920,
                videoHeight = 0,
            )
        )
    }

    @Test
    fun refreshMirrorsEffectiveVideoTrackIdFromBindings() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:720p",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                ),
                trackSelection = VesperTrackSelectionSnapshot(abrPolicy = VesperAbrPolicy.auto()),
                effectiveVideoTrackId = "video:720p",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)
        assertEquals(
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            ),
            bridge.videoVariantObservation.value,
        )

        bindings.effectiveVideoTrackId = null
        bindings.videoVariantObservation = null
        bridge.refresh()
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun selectSourceClearsStaleEffectiveVideoTrackIdUntilBindingsPublishNewState() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:old",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
                    ),
                effectiveVideoTrackId = "video:old",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)
        assertEquals(1_500_000L, bridge.videoVariantObservation.value?.bitRate)

        bindings.onInitialize = {
            bindings.trackCatalog = VesperTrackCatalog.Empty
            bindings.trackSelection = VesperTrackSelectionSnapshot()
            bindings.effectiveVideoTrackId = null
            bindings.videoVariantObservation = null
        }
        bindings.onPrepareSourceNormalizerForPlayback = {
            // Source selection and the subsequent prepare phase both fence
            // callbacks; the important contract is that at least one fence
            // occurs before the new player item is installed.
            assertTrue(bindings.invalidateSystemPlaybackCallbacksCount >= 1)
        }

        runBlocking { bridge.selectSourceAsync(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next")) }
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)

        bindings.trackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:new",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        bindings.trackSelection = VesperTrackSelectionSnapshot(abrPolicy = VesperAbrPolicy.auto())
        bindings.effectiveVideoTrackId = "video:new"
        bindings.videoVariantObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )

        bridge.refresh()
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(1920, bridge.videoVariantObservation.value?.width)
    }

    @Test
    fun constructorDoesNotProbeMobilePluginsForInitialSource() {
        val initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live")
        val bindings =
            FakeBindings(
                mobilePluginDiagnostics =
                    listOf(
                        mapOf(
                            "pluginKind" to "source_normalizer",
                            "status" to "sourceNormalizerSupported",
                        )
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                    ),
            )

        assertNull(bindings.lastProbeSource)
        assertTrue(bridge.pluginDiagnostics.isEmpty())
    }

    @Test
    fun mobilePluginProbeExposesDiagnosticsWithoutReplacingPlaybackSource() {
        val initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live")
        val diagnostics =
            listOf(
                mapOf(
                    "pluginKind" to "source_normalizer",
                    "status" to "sourceNormalizerSupported",
                    "participation" to "available",
                )
            )
        val bindings =
            FakeBindings(
                mobilePluginDiagnostics = diagnostics,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                        runtimeProfile = "default",
                    ),
                frameProcessorConfiguration =
                    VesperFrameProcessorConfiguration(
                        mode = VesperFrameProcessorMode.DiagnosticsOnly,
                        pluginLibraryPaths = listOf("/tmp/libvesper_frame_processor_diagnostic.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(initialSource, bindings.lastProbeSource)
        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(diagnostics, bridge.pluginDiagnostics)
        assertEquals(
            VesperSourceNormalizerMode.PreflightOnly,
            bindings.lastSourceNormalizerConfiguration?.mode,
        )
        assertEquals(
            VesperFrameProcessorMode.DiagnosticsOnly,
            bindings.lastFrameProcessorConfiguration?.mode,
        )
    }

    @Test
    fun syncInitializeReturnsBeforeBackgroundSourcePreparationCompletes() {
        val initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live")
        val prepareStarted = CountDownLatch(1)
        val releasePrepare = CountDownLatch(1)
        val bindings =
            FakeBindings().apply {
                onPrepareSourceNormalizerForPlayback = {
                    prepareStarted.countDown()
                    assertTrue(releasePrepare.await(1, TimeUnit.SECONDS))
                }
            }
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
            )

        bridge.initialize()

        assertTrue(prepareStarted.await(1, TimeUnit.SECONDS))
        assertNull(bindings.lastInitializedSource)

        releasePrepare.countDown()
        repeat(20) {
            if (bindings.lastInitializedSource == initialSource) {
                return
            }
            Thread.sleep(25)
        }
        assertEquals(initialSource, bindings.lastInitializedSource)
    }

    @Test
    fun initializeAsyncPublishesFailureWhenBackgroundSourcePreparationFails() {
        val initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live")
        val bindings =
            FakeBindings().apply {
                onPrepareSourceNormalizerForPlayback = {
                    throw IllegalStateException("normalized resource unavailable")
                }
            }
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                    ),
            )

        val result = runCatching { runBlocking { bridge.initializeAsync() } }

        assertTrue(result.isFailure)
        assertEquals(1, bindings.prepareSourceNormalizerForPlaybackCount)
        assertNull(bindings.lastInitializedSource)
        assertEquals(1, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Paused, bridge.uiState.value.playbackState)
        assertFalse(bridge.uiState.value.isBuffering)
        assertEquals(initialSource.label, bridge.uiState.value.sourceLabel)
        assertEquals(
            "normalized resource unavailable",
            bridge.uiState.value.lastError?.message,
        )
    }

    @Test
    fun nativeFramePipelineConfigurationAddsDiagnosticsWithoutReplacingPlaybackSource() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libdecoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 2,
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "fallback" &&
                    it["route"] == "systemPlayer" &&
                    it["fallbackTargetRoute"] == "systemPlayer" &&
                    it["fallbackReason"].toString().contains("SourceNormalizer packet-stream")
            }
        )
    }

    @Test
    fun nativeFramePipelineDiagnosticsUseSdkManagedRouteForRunnableAndroidContract() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = FakeBindings(),
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 2,
                    ),
            )

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }

        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("sourceNormalizerPacket", diagnostic["sourceInput"])
        assertEquals("MediaCodec", diagnostic["decoderAdapter"])
        assertEquals("SurfaceView", diagnostic["presenterProfile"])
        assertEquals("media_codec_surface_texture", diagnostic["pipelineProfile"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun preferNativeFramePipelineOpensJniSessionAfterSystemStartup() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 2,
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(initialSource, bindings.lastNativeFramePipelineSource)
        assertEquals(1, bindings.openNativeFramePipelineCount)
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        assertEquals(0, bindings.closeNativeFramePipelineCount)
        assertEquals(NativeVideoSurfaceKind.SurfaceView, bindings.lastNativeFramePipelineSurfaceKind)
        assertEquals(
            VesperSourceNormalizerMode.PreflightOnly,
            bindings.lastNativeFramePipelineSourceNormalizerConfiguration?.mode,
        )
        assertEquals(
            VesperNativeFramePipelineMode.PreferNativeFrame,
            bindings.lastNativeFramePipelineConfiguration?.mode,
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertEquals("pending", diagnostic["lastAdvanceStatus"])
    }

    @Test
    fun preferNativeFramePipelineClosesStaleOpenWhenSourceEpochChanges() =
        runBlocking {
            val initialSource =
                VesperPlayerSource.remote(
                    uri = "https://example.com/video.mp4",
                    label = "MP4",
                    protocol = VesperPlayerSourceProtocol.Progressive,
                )
            val nextSource =
                VesperPlayerSource.remote(
                    uri = "https://example.com/next.mp4",
                    label = "Next MP4",
                    protocol = VesperPlayerSourceProtocol.Progressive,
                )
            val scheduler = QueuedNativeFramePipelinePumpScheduler()
            val bindings = FakeBindings()
            val bridge =
                VesperNativePlayerBridge(
                    bindings = bindings,
                    initialSource = initialSource,
                    sourceNormalizerConfiguration =
                        VesperSourceNormalizerConfiguration(
                            mode = VesperSourceNormalizerMode.PreflightOnly,
                            pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                        ),
                    nativeFramePipelineConfiguration =
                        VesperNativeFramePipelineConfiguration(
                            mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                            decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                            frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                            maxInFlightFrames = 2,
                        ),
                    nativeFramePipelinePumpScheduler = scheduler,
            )

            bridge.initializeAsync()
            assertTrue(scheduler.hasPendingActions())
            scheduler.runNext()
            scheduler.runNext()
            assertTrue(
                "native-frame open should still be pending before the epoch changes",
                bindings.openNativeFramePipelineCount == 0,
            )
            assertTrue(scheduler.hasPendingActions())

            bridge.sourceLoadEpoch.incrementAndGet()
            bridge.currentSource = nextSource
            scheduler.runNext()

            assertEquals(1, bindings.openNativeFramePipelineCount)
            assertEquals(0, bindings.advanceNativeFramePipelineCount)
            assertEquals(1, bindings.closeNativeFramePipelineCount)
            assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())
            val diagnostic =
                bridge.pluginDiagnostics.first {
                    it["pluginKind"] == "native_frame_pipeline"
                }
            assertFalse(diagnostic["lifecycle"] == "open")
            assertNull(diagnostic["lastAdvanceStatus"])
        }

    @Test
    fun preferNativeFramePipelineSkipsSystemSourceNormalizerResourcePlayback() {
        val initialSource =
            VesperPlayerSource.local(
                uri = "file:///tmp/video.mp4",
                label = "Local MP4",
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(false, bindings.lastSystemPlaybackUsesSourceNormalizerResource)
        assertEquals(false, bindings.lastSystemPlaybackVideoEnabled)
        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(initialSource, bindings.lastNativeFramePipelineSource)
        assertEquals(
            VesperSourceNormalizerMode.RequireNormalized,
            bindings.lastNativeFramePipelineSourceNormalizerConfiguration?.mode,
        )
    }

    @Test
    fun diagnosticsOnlyKeepsSystemSourceNormalizerResourcePlaybackEnabled() {
        val initialSource =
            VesperPlayerSource.local(
                uri = "file:///tmp/video.mp4",
                label = "Local MP4",
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.DiagnosticsOnly,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(true, bindings.lastSystemPlaybackUsesSourceNormalizerResource)
        assertEquals(true, bindings.lastSystemPlaybackVideoEnabled)
        assertNull(bindings.lastNativeFramePipelineSource)
    }

    @Test
    fun sourceNormalizerResourcePlaybackSkipsHostHandledNetworkSources() {
        val preferNormalized =
            VesperSourceNormalizerConfiguration(
                mode = VesperSourceNormalizerMode.PreferNormalized,
                pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
            )
        val requireNormalized =
            VesperSourceNormalizerConfiguration(
                mode = VesperSourceNormalizerMode.RequireNormalized,
                pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
            )

        assertFalse(
            preferNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.hls("https://example.com/live.m3u8", "Live"),
            )
        )
        assertFalse(
            requireNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.dash("https://example.com/manifest.mpd", "Dash"),
            )
        )
        assertFalse(
            preferNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.remote(
                    uri = "https://example.com/video.mp4",
                    label = "Remote MP4",
                    protocol = VesperPlayerSourceProtocol.Progressive,
                ),
            )
        )
        assertTrue(
            preferNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.local("file:///tmp/video.mp4", "Local MP4"),
            )
        )
    }

    @Test
    fun sourceNormalizerBypassDiagnosticsDecodeHdrResourceReason() {
        val diagnostics =
            listOf(
                mapOf(
                    "path" to "/tmp/libsource_normalizer.so",
                    "pluginKind" to "source_normalizer",
                    "status" to "sourceNormalizerUnsupported",
                    "participation" to "bypassed",
                    "message" to
                        "HdrResourceMetadataNotPreserved: source normalizer fMP4 resource route cannot currently guarantee HDR/Dolby Vision metadata preservation for system playback",
                )
            )

        assertEquals(1, diagnostics.size)
        assertEquals("sourceNormalizerUnsupported", diagnostics.first()["status"])
        assertEquals("bypassed", diagnostics.first()["participation"])
        assertEquals("sourceNormalizerResourceBypassedForHdr", sourceNormalizerBypassReason(diagnostics))
    }

    @Test
    fun sourceNormalizerResourceOpenObjectIsNotParsedAsBypassDiagnostics() {
        val diagnostics =
            parseSourceNormalizerBypassDiagnostics(
                """
                {
                  "handle": 42,
                  "outputRoute": "fmp4LocalStream",
                  "primaryResourcePath": "/tmp/normalized.mp4"
                }
                """.trimIndent(),
            )

        assertNull(diagnostics)
    }

    @Test
    fun preparedSourceNormalizerResourceIsPassedToMainApplyWithoutJsonReparse() {
        val prepared =
            NativeSourceNormalizerResourcePreparedOpenOutcome(
                resource =
                    NativeSourceNormalizerResource(
                        handle = 42L,
                        outputRoute = "fmp4LocalStream",
                        loopbackToken = "prepared-token",
                        playbackSource =
                            VesperPlayerSource.remote(
                                uri = "http://127.0.0.1:54321/normalized/prepared-token/primary",
                                label = "Local MP4",
                                protocol = VesperPlayerSourceProtocol.Progressive,
                            ),
                        diagnostics =
                            listOf(
                                mapOf(
                                    "pluginKind" to "source_normalizer",
                                    "participation" to "participated",
                                )
                            ),
                    ),
            )
        val bindings = FakeBindings(sourceNormalizerPrepareOutcome = prepared)
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.local("file:///tmp/original.mp4", "Local MP4"),
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertTrue(bindings.lastPreparedSourceNormalizer === prepared)
        assertEquals(1, bindings.prepareSourceNormalizerForPlaybackCount)
        assertEquals(
            "http://127.0.0.1:54321/normalized/prepared-token/primary",
            bindings.lastPreparedSourceNormalizer?.resource?.playbackSource?.uri,
        )
    }

    @Test
    fun staleSourceNormalizerPrepareFailureIsIgnoredAfterEpochChanges() =
        runBlocking {
            val prepareEntered = CountDownLatch(1)
            val releasePrepare = CountDownLatch(1)
            val bindings =
                FakeBindings().apply {
                    onPrepareSourceNormalizerForPlayback = {
                        prepareEntered.countDown()
                        assertTrue(
                            "test prepare hook should be released",
                            releasePrepare.await(5, TimeUnit.SECONDS),
                        )
                        throw IllegalStateException("stale source normalizer failure")
                    }
                }
            val bridge =
                VesperNativePlayerBridge(
                    bindings = bindings,
                    initialSource = VesperPlayerSource.local("file:///tmp/old.mp4", "Old"),
                    sourceNormalizerConfiguration =
                        VesperSourceNormalizerConfiguration(
                            mode = VesperSourceNormalizerMode.RequireNormalized,
                            pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                        ),
                )

            val result = async(Dispatchers.Default) { runCatching { bridge.initializeAsync() } }
            assertTrue(
                "source normalizer prepare should run in the background",
                prepareEntered.await(5, TimeUnit.SECONDS),
            )

            bridge.sourceLoadEpoch.incrementAndGet()
            bridge.currentSource = VesperPlayerSource.local("file:///tmp/new.mp4", "New")
            releasePrepare.countDown()

            assertNull(result.await().exceptionOrNull())
            assertNull(bridge.uiState.value.lastError)
            assertNull(bindings.lastInitializedSource)
        }

    @Test
    fun canceledInitializeDisposesPreparedSourceNormalizerAfterBackgroundPrepareCompletes() =
        runBlocking {
            val prepareEntered = CountDownLatch(1)
            val releasePrepare = CountDownLatch(1)
            val prepared =
                NativeSourceNormalizerResourcePreparedOpenOutcome(
                    resource =
                        NativeSourceNormalizerResource(
                            handle = 42L,
                            playbackSource = VesperPlayerSource.local("file:///tmp/normalized.mp4", "Normalized"),
                            outputRoute = "fmp4LocalStream",
                            loopbackToken = null,
                            diagnostics = emptyList(),
                        ),
                )
            val bindings =
                FakeBindings(sourceNormalizerPrepareOutcome = prepared).apply {
                    onPrepareSourceNormalizerForPlayback = {
                        prepareEntered.countDown()
                        assertTrue(
                            "test prepare hook should be released",
                            releasePrepare.await(5, TimeUnit.SECONDS),
                        )
                    }
                }
            val bridge =
                VesperNativePlayerBridge(
                    bindings = bindings,
                    initialSource = VesperPlayerSource.local("file:///tmp/original.mp4", "Original"),
                    sourceNormalizerConfiguration =
                        VesperSourceNormalizerConfiguration(
                            mode = VesperSourceNormalizerMode.RequireNormalized,
                            pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                        ),
                )

            val initializeJob = async(Dispatchers.Default) { bridge.initializeAsync() }
            assertTrue(
                "source normalizer prepare should run in the background",
                prepareEntered.await(5, TimeUnit.SECONDS),
            )

            initializeJob.cancel(CancellationException("test cancellation"))
            releasePrepare.countDown()

            val result = runCatching { initializeJob.await() }
            assertTrue(result.exceptionOrNull() is CancellationException)
            assertEquals(1, bindings.prepareSourceNormalizerForPlaybackCount)
            assertEquals(1, bindings.disposePreparedSourceNormalizerResourceCount)
            assertNull(bindings.lastInitializedSource)
        }

    @Test
    fun nativeFramePipelineDiagnosticsReportPresenterSurfaceState() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatus =
                    mapOf(
                        "status" to "pending",
                        "presenterReady" to false,
                        "presenterConfigured" to false,
                        "presenterState" to "waitingForPresenter",
                        "surfaceAttached" to true,
                        "surfaceProfile" to "SurfaceView",
                        "message" to "presenter surface attached",
                    )
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals(false, diagnostic["presenterReady"])
        assertEquals(false, diagnostic["presenterConfigured"])
        assertEquals("waitingForPresenter", diagnostic["presenterState"])
        assertEquals(true, diagnostic["surfaceAttached"])
        assertEquals("SurfaceView", diagnostic["surfaceProfile"])
    }

    @Test
    fun nativeFramePipelineDiagnosticsUseLatestPresenterSurfaceState() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatus =
                    mapOf(
                        "status" to "pending",
                        "presenterReady" to true,
                        "presenterConfigured" to true,
                        "presenterState" to "ready",
                        "surfaceAttached" to true,
                        "surfaceProfile" to "SurfaceView",
                        "message" to "presenter surface attached",
                    )
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bindings.setCurrentNativeFramePipelineStatusForTest(
            bindings.nativeFramePipelineStatusForTest(
                "status" to "pending",
                "presenterReady" to false,
                "presenterConfigured" to false,
                "presenterState" to "waitingForSurface",
                "surfaceAttached" to false,
                "message" to "presenter surface detached",
            )
        )
        bridge.refresh()

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals(false, diagnostic["presenterReady"])
        assertEquals(false, diagnostic["presenterConfigured"])
        assertEquals("waitingForSurface", diagnostic["presenterState"])
        assertEquals(false, diagnostic["surfaceAttached"])
        assertNull(diagnostic["surfaceProfile"])
    }

    @Test
    fun nativeFramePipelineRawFrameAdvanceIsReleasedWhenPresenterDoesNotAccept() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatus =
                    mapOf(
                        "status" to "frame",
                        "handle" to 77L,
                        "nativeHandle" to 1234L,
                        "message" to "decoded frame",
                        "requiresHostRelease" to false,
                        "counters" to
                            mapOf(
                                "processedFrames" to 1L,
                                "presentedFrames" to 0L,
                                "releasedFrames" to 0L,
                                "deadlineMisses" to 0L,
                                "backpressureCount" to 0L,
                                "lateDropped" to 0L,
                            ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        assertEquals(listOf(77L to false), bindings.releasedNativeFramePipelineFrames)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("released", diagnostic["lastAdvanceStatus"])
        assertEquals(1L, diagnostic["processedFrames"])
        assertEquals(0L, diagnostic["presentedFrames"])
    }

    @Test
    fun preferNativeFramePipelinePumpAdvancesWhilePlayingAndStopsAtEndOfStream() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        ),
                        mapOf(
                            "status" to "presented",
                            "message" to "presented frame",
                            "counters" to
                                mapOf(
                                    "processedFrames" to 1L,
                                    "presentedFrames" to 1L,
                                    "deadlineMisses" to 0L,
                                    "backpressureCount" to 0L,
                                    "lateDropped" to 0L,
                                ),
                        ),
                        mapOf(
                            "status" to "pending",
                            "message" to "decoder needs more input",
                        ),
                        mapOf(
                            "status" to "endOfStream",
                            "message" to "end of stream",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())

        bridge.play()
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()
        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()
        assertEquals(3, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()
        assertEquals(4, bindings.advanceNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("endOfStream", diagnostic["lastAdvanceStatus"])
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelinePlayFromFinishedSeeksNativeFramePipelineBeforeRestartingPump() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "endOfStream",
                            "message" to "end of stream",
                        ),
                        mapOf(
                            "status" to "pending",
                            "message" to "waiting after replay seek",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        assertEquals("endOfStream", bridge.pluginDiagnostics.first {
            it["pluginKind"] == "native_frame_pipeline"
        }["lastAdvanceStatus"])
        bindings.events.add(NativeBridgeEvent.Ended())
        bridge.refresh()

        bridge.play()

        assertTrue(bindings.seekToPositions.isEmpty())
        assertEquals(listOf(0L), bindings.seekNativeFramePipelinePositions)
        assertEquals(0, bindings.flushNativeFramePipelineCount)
        assertEquals(1, bindings.playCount)
        assertTrue(scheduler.hasPendingActions())
        assertEquals(PlaybackStateUi.Playing, bridge.uiState.value.playbackState)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertEquals("seeked", diagnostic["lastAdvanceStatus"])
        assertEquals(true, diagnostic["pumpRunning"])
    }

    @Test
    fun nativeFrameRuntimeCommandQueueCoalescesSeekStormAndPrioritizesClose() {
        val queue = BoundedNativeFramePipelineRuntimeCommandQueue(capacity = 3)

        repeat(100) { index ->
            assertTrue(
                queue.enqueue(
                    NativeFramePipelineRuntimeCommand(
                        operation = "seek",
                        coalescingKey = "seek",
                        action = {},
                    )
                )
            )
            assertTrue("seek command $index should be coalesced", queue.size <= 1)
        }

        assertTrue(
            queue.enqueue(
                NativeFramePipelineRuntimeCommand(
                    operation = "status",
                    coalescingKey = "status",
                    action = {},
                )
            )
        )
        assertTrue(
            queue.enqueue(
                NativeFramePipelineRuntimeCommand(
                    operation = "flush",
                    coalescingKey = "flush",
                    action = {},
                )
            )
        )
        assertEquals(3, queue.size)

        assertTrue(
            queue.enqueue(
                NativeFramePipelineRuntimeCommand(
                    operation = "close",
                    runsDuringClose = true,
                    replacesPendingCommands = true,
                    action = {},
                )
            )
        )

        assertEquals("close", queue.removeFirstOrNull()?.operation)
        assertNull(queue.removeFirstOrNull())
    }

    @Test
    fun preferNativeFramePipelineCoalescesPendingSeekStormRuntimeCommands() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = CoalescingQueuedNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        )
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        scheduler.runUntilIdle()
        assertEquals(1, bindings.openNativeFramePipelineCount)

        repeat(100) { index ->
            assertTrue(bridge.seekBindingsTo(index.toLong()))
        }

        assertTrue(
            "seek storm should stay bounded before runtime commands drain",
            scheduler.pendingActionCount <= 3,
        )
        scheduler.runUntilIdle()

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(99L), bindings.seekNativeFramePipelinePositions)
    }

    @Test
    fun preferNativeFramePipelineRuntimeAdvanceFailureFallsBackToSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        )
                    ),
                nativeFramePipelineAdvanceError =
                    IllegalStateException("simulated native-frame runtime failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        val closeCountBeforeRuntimeFailure = bindings.closeNativeFramePipelineCount
        bindings.events += NativeBridgeEvent.PlaybackStateChanged(PlaybackStateUi.Playing)

        bridge.refresh()
        assertTrue(scheduler.hasPendingActions())
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertEquals(closeCountBeforeRuntimeFailure + 1, bindings.closeNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("fallback", diagnostic["participation"])
        assertEquals("systemPlayer", diagnostic["route"])
        assertEquals("fallback", diagnostic["lifecycle"])
        assertEquals("systemPlayer", diagnostic["fallbackTargetRoute"])
        assertEquals("simulated native-frame runtime failure", diagnostic["fallbackReason"])
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun requireNativeFramePipelineRuntimeAdvanceFailureKeepsBridgeRecoverable() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        )
                    ),
                nativeFramePipelineAdvanceError =
                    IllegalStateException("simulated required native-frame runtime failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        val closeCountBeforeRuntimeFailure = bindings.closeNativeFramePipelineCount
        bindings.events += NativeBridgeEvent.PlaybackStateChanged(PlaybackStateUi.Playing)

        bridge.refresh()
        assertTrue(scheduler.hasPendingActions())
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertEquals(closeCountBeforeRuntimeFailure + 1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertEquals(1, bindings.clearSystemPlaybackCount)
        assertFalse(scheduler.hasPendingActions())
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame runtime failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame runtime failure",
            diagnostic["fallbackReason"],
        )
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelinePumpSchedulesHostTimedReleaseToSurface() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        ),
                        mapOf(
                            "status" to "frame",
                            "handle" to 88L,
                            "presentationTimeUs" to 1_050_000L,
                            "requiresHostRelease" to true,
                            "message" to "host-timed release",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        scheduler.runNext()

        assertEquals(listOf(88L to true), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineKeepsPumpRunningWhenSystemSnapshotReportsReady() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Ready,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 0L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        ),
                        mapOf(
                            "status" to "pending",
                            "message" to "system snapshot still reports ready",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()

        assertTrue(scheduler.hasPendingActions())
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals(true, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelineReleasesPendingFrameWhenPumpEpochChangesBeforeRelease() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        lateinit var bridge: VesperNativePlayerBridge
        var schedulerRunCount = 0
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 90L,
                            "presentationTimeUs" to 1_050_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val scheduler =
            ManualNativeFramePipelinePumpScheduler(
                beforeRun = {
                    schedulerRunCount += 1
                    if (
                        schedulerRunCount == 2 &&
                            bindings.releasedNativeFramePipelineFrames.isEmpty()
                    ) {
                        bridge.pause()
                    }
                },
            )
        bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()

        assertEquals(listOf(90L to false), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun nativeFramePipelineEpochChangeDuringAdvanceReleasesReturnedFrameImmediately() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 303L,
                            "presentationTimeUs" to 1_050_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bindings.onAdvanceNativeFramePipeline = bridge::stopNativeFramePipelinePump
        bridge.play()
        scheduler.runNext()

        assertEquals(listOf(303L to false), bindings.releasedNativeFramePipelineFrames)
        assertNull(bridge.pendingTimedNativeFrame)
        assertFalse(scheduler.hasPendingActions())
    }

    @Test
    fun nativeFramePipelineTerminalErrorReleasesPendingFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 304L,
                            "presentationTimeUs" to 1_050_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertNotNull(bridge.pendingTimedNativeFrame)
        bindings.events +=
            NativeBridgeEvent.Error(
                message = "terminal playback failure",
                codeOrdinal = VesperPlayerErrorCode.BackendFailure.jniOrdinal,
                categoryOrdinal = VesperPlayerErrorCategory.Playback.jniOrdinal,
                retriable = false,
            )

        bridge.refresh()

        assertEquals(listOf(304L to false), bindings.releasedNativeFramePipelineFrames)
        assertNull(bridge.pendingTimedNativeFrame)
        assertFalse(scheduler.hasPendingActions())
    }

    @Test
    fun preferNativeFramePipelineHostTimedReleaseFailureFallsBackWithoutAdvancingAgain() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 91L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
                nativeFramePipelineReleaseError =
                    IllegalStateException("simulated native-frame release failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        val advanceCountBeforeReleaseFailure = bindings.advanceNativeFramePipelineCount
        val closeCountBeforeReleaseFailure = bindings.closeNativeFramePipelineCount

        scheduler.runNext()

        assertEquals(advanceCountBeforeReleaseFailure, bindings.advanceNativeFramePipelineCount)
        assertEquals(closeCountBeforeReleaseFailure + 1, bindings.closeNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("fallback", diagnostic["participation"])
        assertEquals("systemPlayer", diagnostic["route"])
        assertEquals("fallback", diagnostic["lifecycle"])
        assertEquals("systemPlayer", diagnostic["fallbackTargetRoute"])
        assertEquals("simulated native-frame release failure", diagnostic["fallbackReason"])
    }

    @Test
    fun requireNativeFramePipelineHostTimedReleaseFailureDisposesSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 91L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
                nativeFramePipelineReleaseError =
                    IllegalStateException("simulated required native-frame release failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        val closeCountBeforeReleaseFailure = bindings.closeNativeFramePipelineCount

        scheduler.runNext()

        assertEquals(closeCountBeforeReleaseFailure + 1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertFalse(scheduler.hasPendingActions())
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame release failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame release failure",
            diagnostic["fallbackReason"],
        )
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelineSeekFlushesAndSeeksOpenSession() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(1_000L), bindings.seekNativeFramePipelinePositions)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("open", diagnostic["lifecycle"])
        assertEquals(1L, diagnostic["processedFrames"])
        assertEquals(1L, diagnostic["presentedFrames"])
    }

    @Test
    fun requireNativeFramePipelineSeekFailureKeepsBridgeRecoverable() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated required native-frame seek failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(1_000L), bindings.seekNativeFramePipelinePositions)
        assertEquals(1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertEquals(0L, bridge.uiState.value.timeline.positionMs)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame seek failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame seek failure",
            diagnostic["fallbackReason"],
        )
    }

    @Test
    fun requireNativeFramePipelineFlushFailureDisposesSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineFlushError =
                    IllegalStateException("simulated required native-frame flush failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertTrue(bindings.seekNativeFramePipelinePositions.isEmpty())
        assertEquals(1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertEquals(0L, bridge.uiState.value.timeline.positionMs)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame flush failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame flush failure",
            diagnostic["fallbackReason"],
        )
    }

    @Test
    fun requireNativeFramePipelineCanRecoverAfterRuntimeFailureOnReinitialize() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated required native-frame seek failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.seekBy(1_000L)
        assertEquals(0, bindings.disposeCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed"
            }
        )

        bindings.nativeFramePipelineSeekError = null
        runBlocking { bridge.initializeAsync() }

        assertEquals(2, bindings.openNativeFramePipelineCount)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun requireNativeFramePipelineFailureDoesNotBlockNextSourceInitialization() {
        val hlsSource =
            VesperPlayerSource.hls(
                uri = "https://example.com/master.m3u8",
                label = "HLS",
            )
        val localSource =
            VesperPlayerSource.local(
                uri = "file:///tmp/local.mp4",
                label = "Local MP4",
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = hlsSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        assertEquals(0, bindings.disposeCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["lifecycle"] == "failed"
            }
        )

        bindings.nativeFramePipelineOpenError = null
        runBlocking { bridge.selectSourceAsync(localSource) }

        assertEquals(localSource, bindings.lastInitializedSource)
        assertEquals(localSource, bindings.lastNativeFramePipelineSource)
        assertEquals(2, bindings.openNativeFramePipelineCount)
        assertEquals(1, bindings.playCount)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("open", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun preferNativeFramePipelinePauseStopsPumpAndSeekRestartsWhenPlaying() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        assertTrue(scheduler.hasPendingActions())

        bridge.pause()
        assertFalse(scheduler.hasPendingActions())
        val advanceCountAfterPause = bindings.advanceNativeFramePipelineCount
        scheduler.runNext()
        assertEquals(advanceCountAfterPause, bindings.advanceNativeFramePipelineCount)

        bridge.play()
        assertTrue(scheduler.hasPendingActions())
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(1_000L), bindings.seekNativeFramePipelinePositions)
        assertTrue(scheduler.hasPendingActions())
    }

    @Test
    fun nativeFramePipelineSchedulerCloseClearsPendingWorkAndRejectsNewSchedules() {
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        var runCount = 0

        scheduler.schedule(0L) { runCount += 1 }
        assertTrue(scheduler.hasPendingActions())

        scheduler.close()
        scheduler.runNext()
        scheduler.schedule(0L) { runCount += 1 }

        assertEquals(0, runCount)
        assertFalse(scheduler.hasPendingActions())
        assertTrue(scheduler.closeCount > 0)
    }

    @Test
    fun preferNativeFramePipelineBackgroundPumpIdleTickDoesNotCrashOnNullMainThreadResult() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ThreadedNativeFramePipelinePumpScheduler(expectedRuns = 4)
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "background warmup",
                        )
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()

        assertTrue(scheduler.awaitRun())
        assertTrue(waitUntil { bindings.advanceNativeFramePipelineCount >= 2 })
        assertNull(scheduler.lastError)

        bridge.dispose()
        scheduler.close()
    }

    @Test
    fun preferNativeFramePipelinePauseDropsPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 89L,
                            "presentationTimeUs" to 1_100_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        bridge.pause()
        scheduler.runNext()

        assertEquals(listOf(89L to false), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun requireNativeFramePipelinePauseReleaseFailureKeepsHardFailureState() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 95L,
                            "presentationTimeUs" to 1_100_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        bindings.nativeFramePipelineReleaseError =
            IllegalStateException("simulated release failure")

        bridge.pause()

        assertEquals(0, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated release failure"))
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed" &&
                    it["fallbackTargetRoute"] == null &&
                    it["fallbackReason"] == "simulated release failure"
            }
        )
    }

    @Test
    fun requireNativeFramePipelineHardFailureIgnoresLaterPlaybackCommands() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated native-frame hard failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.seekBy(1_000L)
        val playCountAfterFailure = bindings.playCount
        val stopCountAfterFailure = bindings.stopCount
        val seekToPositionsAfterFailure = bindings.seekToPositions.toList()
        val playbackRatesAfterFailure = bindings.playbackRates.toList()

        bridge.play()
        bridge.stop()
        bridge.seekBy(2_000L)
        bridge.setPlaybackRate(2.0f)

        assertEquals(playCountAfterFailure, bindings.playCount)
        assertEquals(stopCountAfterFailure, bindings.stopCount)
        assertEquals(seekToPositionsAfterFailure, bindings.seekToPositions)
        assertEquals(playbackRatesAfterFailure, bindings.playbackRates)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertEquals(0L, bridge.uiState.value.timeline.positionMs)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame hard failure"))
    }

    @Test
    fun requireNativeFramePipelineHardFailureIgnoresLaterConfigurationCommands() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated native-frame hard failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.seekBy(1_000L)
        val initializedSourceAfterFailure = bindings.lastInitializedSource

        bridge.setVideoTrackSelection(VesperTrackSelection.track("video:720p"))
        bridge.setAudioTrackSelection(VesperTrackSelection.track("audio:main"))
        val subtitleError =
            org.junit.Assert.assertThrows(VesperPlayerUnsupportedOperation::class.java) {
                runBlocking { bridge.setSubtitleTrackSelection(VesperTrackSelection.disabled()) }
            }
        assertEquals("subtitle_platform_track_unavailable", subtitleError.details["code"])
        bridge.setAbrPolicy(VesperAbrPolicy.fixedTrack("video:720p"))
        bridge.configureSystemPlayback(
            VesperSystemPlaybackConfiguration(
                metadata =
                    VesperSystemPlaybackMetadata(
                        title = "Ignored",
                        contentUri = initialSource.uri,
                    )
            )
        )
        bridge.updateSystemPlaybackMetadata(VesperSystemPlaybackMetadata(title = "Ignored"))
        bridge.clearSystemPlayback()
        bridge.setResiliencePolicy(VesperPlaybackResiliencePolicy.resilient())

        assertEquals(0, bindings.videoTrackSelectionCount)
        assertEquals(0, bindings.audioTrackSelectionCount)
        assertEquals(0, bindings.subtitleTrackSelectionCount)
        assertEquals(0, bindings.abrPolicyCount)
        assertEquals(0, bindings.configureSystemPlaybackCount)
        assertEquals(0, bindings.updateSystemPlaybackMetadataCount)
        assertEquals(1, bindings.clearSystemPlaybackCount)
        assertEquals(initializedSourceAfterFailure, bindings.lastInitializedSource)
        assertEquals(0, bindings.disposeCount)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame hard failure"))
    }

    @Test
    fun requireNativeFramePipelineHardFailureIgnoresLaterRefreshAndNativeUpdates() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated native-frame hard failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        val updateListener = checkNotNull(bindings.currentUpdateListener())
        bridge.seekBy(1_000L)
        val expectedUiState = bridge.uiState.value
        val refreshCountAfterFailure = bindings.refreshSnapshotCount

        bindings.snapshot =
            NativeBridgeSnapshot(
                playbackState = PlaybackStateUi.Playing,
                playbackRate = 2.0f,
                isBuffering = true,
                isInterrupted = true,
                timeline =
                    TimelineUiState(
                        kind = TimelineKind.Vod,
                        isSeekable = true,
                        seekableRange = SeekableRangeUi(0L, 10_000L),
                        liveEdgeMs = null,
                        positionMs = 5_000L,
                        durationMs = 10_000L,
                    ),
            )
        bindings.events.add(
            NativeBridgeEvent.PlaybackStateChanged(PlaybackStateUi.Playing)
        )
        bindings.events.add(
            NativeBridgeEvent.SeekCompleted(positionMs = 5_000L)
        )
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale playback error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        bridge.refresh()
        updateListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(refreshCountAfterFailure + 1, bindings.refreshSnapshotCount)
        assertEquals(0, bindings.disposeCount)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame hard failure"))
    }

    @Test
    fun preferNativeFramePipelineRefreshDoesNotReschedulePendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 90L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        bridge.refresh()
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        scheduler.runNext()
        assertEquals(listOf(90L to true), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineSelectSourceReleasesPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 93L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertTrue(scheduler.hasPendingActions())
        val closeCountBeforeSelectSource = bindings.closeNativeFramePipelineCount
        val cancelCountBeforeSelectSource = scheduler.cancelCount

        runBlocking {
            bridge.selectSourceAsync(
                VesperPlayerSource.remote(
                    uri = "https://example.com/next.mp4",
                    label = "Next",
                    protocol = VesperPlayerSourceProtocol.Progressive,
                )
            )
        }

        assertTrue(scheduler.cancelCount > cancelCountBeforeSelectSource)
        assertEquals(listOf(93L to false), bindings.releasedNativeFramePipelineFrames)
        assertTrue(bindings.closeNativeFramePipelineCount > closeCountBeforeSelectSource)
    }

    @Test
    fun preferNativeFramePipelineInitializeReleasesPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 94L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertTrue(scheduler.hasPendingActions())
        val closeCountBeforeInitialize = bindings.closeNativeFramePipelineCount
        val cancelCountBeforeInitialize = scheduler.cancelCount

        runBlocking { bridge.initializeAsync() }

        assertTrue(scheduler.cancelCount > cancelCountBeforeInitialize)
        assertEquals(listOf(94L to false), bindings.releasedNativeFramePipelineFrames)
        assertTrue(bindings.closeNativeFramePipelineCount > closeCountBeforeInitialize)
    }

    @Test
    fun preferNativeFramePipelineRateChangeReschedulesPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 92L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        scheduler.runNext()
        assertEquals(80L, scheduler.lastDelayMs)

        bindings.snapshot =
            NativeBridgeSnapshot(
                playbackState = PlaybackStateUi.Playing,
                playbackRate = 2.0f,
                isBuffering = false,
                isInterrupted = false,
                timeline =
                    TimelineUiState(
                        kind = TimelineKind.Vod,
                        isSeekable = true,
                        seekableRange = SeekableRangeUi(0L, 10_000L),
                        liveEdgeMs = null,
                        positionMs = 1_000L,
                        durationMs = 10_000L,
                    ),
            )
        bridge.setPlaybackRate(2.0f)

        assertEquals(40L, scheduler.lastDelayMs)
        scheduler.runNext()
        assertEquals(listOf(92L to true), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineStopFlushesOpenSession() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.stop()

        assertEquals(1, bindings.flushNativeFramePipelineCount)
    }

    @Test
    fun disposeClosesOpenNativeFramePipelineSession() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.dispose()

        assertEquals(1, bindings.closeNativeFramePipelineCount)
        assertEquals(1, bindings.disposeCount)
    }

    @Test
    fun textureViewNativeFramePipelineStillFallsBackToSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                surfaceKind = NativeVideoSurfaceKind.TextureView,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(0, bindings.openNativeFramePipelineCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "fallback" &&
                    it["route"] == "systemPlayer" &&
                    it["fallbackReason"].toString().contains("TextureView")
            }
        )
    }

    @Test
    fun preferNativeFramePipelineOpenFailureFallsBackToSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(1, bindings.openNativeFramePipelineCount)
        assertEquals(0, bindings.advanceNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "fallback" &&
                    it["route"] == "systemPlayer" &&
                    it["fallbackReason"] == "simulated native-frame open failure"
            }
        )
    }

    @Test
    fun preferNativeFramePipelineFallbackKeepsSystemPlayerCommandsActive() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.play()
        bridge.seekBy(2_000L)
        bridge.setPlaybackRate(1.5f)
        bridge.setVideoTrackSelection(VesperTrackSelection.track("video:720p"))
        bridge.setAudioTrackSelection(VesperTrackSelection.track("audio:main"))
        runBlocking { bridge.setSubtitleTrackSelection(VesperTrackSelection.disabled()) }
        bridge.setAbrPolicy(VesperAbrPolicy.fixedTrack("video:720p"))
        bridge.configureSystemPlayback(
            VesperSystemPlaybackConfiguration(
                metadata =
                    VesperSystemPlaybackMetadata(
                        title = "Fallback",
                        contentUri = initialSource.uri,
                    )
            )
        )
        bridge.updateSystemPlaybackMetadata(VesperSystemPlaybackMetadata(title = "Fallback"))
        bridge.clearSystemPlayback()

        assertEquals(1, bindings.playCount)
        assertEquals(listOf(2_000L), bindings.seekToPositions)
        assertEquals(listOf(1.5f), bindings.playbackRates)
        assertEquals(1, bindings.videoTrackSelectionCount)
        assertEquals(1, bindings.audioTrackSelectionCount)
        assertEquals(1, bindings.subtitleTrackSelectionCount)
        assertEquals(1, bindings.abrPolicyCount)
        assertEquals(1, bindings.configureSystemPlaybackCount)
        assertEquals(1, bindings.updateSystemPlaybackMetadataCount)
        assertEquals(1, bindings.clearSystemPlaybackCount)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("fallback", diagnostic["participation"])
        assertEquals("systemPlayer", diagnostic["route"])
        assertEquals("systemPlayer", diagnostic["fallbackTargetRoute"])
        assertEquals("simulated native-frame open failure", diagnostic["fallbackReason"])
    }

    @Test
    fun preferNativeFramePipelineSelectSourceClearsFallbackAndRetriesNativeFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val nextSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/next.mp4",
                label = "Next MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        runBlocking { bridge.initializeAsync() }
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["lifecycle"] == "fallback"
            }
        )

        bindings.nativeFramePipelineOpenError = null
        runBlocking { bridge.selectSourceAsync(nextSource) }

        assertEquals(2, bindings.openNativeFramePipelineCount)
        assertEquals(nextSource, bindings.lastNativeFramePipelineSource)
        assertEquals(1, bindings.playCount)
        assertTrue(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun requireNativeFramePipelineOpenFailureKeepsBridgeRecoverable() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(1, bindings.openNativeFramePipelineCount)
        assertEquals(0, bindings.advanceNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame open failure"))
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed" &&
                    it["fallbackTargetRoute"] == null &&
                    it["status"] == "unsupported" &&
                    it["fallbackReason"] == "simulated native-frame open failure"
            }
        )
    }

    @Test
    fun nativeFramePipelineDiagnosticsSurviveNativeStartupDiagnosticsReplacement() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeStartupDiagnostics =
                    listOf(
                        mapOf(
                            "pluginKind" to "source_normalizer",
                            "status" to "sourceNormalizerSupported",
                            "participation" to "participated",
                        )
                    )
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.DiagnosticsOnly,
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "source_normalizer" &&
                    it["participation"] == "participated"
            }
        )
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "available" &&
                    it["route"] == "systemPlayer"
            }
        )
    }

    @Test
    fun requireNativeFramePipelineFailsWithoutInitializingSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libdecoder.so"),
                    ),
            )

        runBlocking { bridge.initializeAsync() }

        assertNull(bindings.lastInitializedSource)
        assertTrue(bridge.uiState.value.subtitle.contains("SourceNormalizer packet-stream"))
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed" &&
                    it["fallbackTargetRoute"] == null &&
                    it["status"] == "unsupported"
            }
        )
    }

    @Test
    fun disposeClearsEffectiveVideoTrackIdImmediately() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:720p",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:720p"),
                    ),
                effectiveVideoTrackId = "video:720p",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)
        assertEquals(1280, bridge.videoVariantObservation.value?.width)

        bridge.dispose()
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)

        bridge.refresh()
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun disposeOnlyDelegatesOnce() {
        val bindings = FakeBindings()
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.dispose()
        bridge.dispose()

        assertEquals(1, bindings.disposeCount)
    }

    @Test
    fun selectSourceFailureClearsStaleTrackState() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:old",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
                    ),
                effectiveVideoTrackId = "video:old",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals(1, bridge.trackCatalog.value.tracks.size)
        assertEquals(
            VesperAbrPolicy.fixedTrack("video:old"),
            bridge.trackSelection.value.abrPolicy,
        )
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)

        bindings.onInitialize = { error("simulated initialize failure") }

        assertTrue(
            runCatching {
                runBlocking { bridge.selectSourceAsync(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next")) }
            }.isFailure
        )

        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerFromPreviousSourceIsIgnored() {
        val oldTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:old",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val oldTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
            )
        val oldObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val newTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:new",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        val newTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.auto(),
            )
        val newObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )
        val bindings =
            FakeBindings(
                trackCatalog = oldTrackCatalog,
                trackSelection = oldTrackSelection,
                effectiveVideoTrackId = "video:old",
                videoVariantObservation = oldObservation,
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())
        bindings.onInitialize = {
            bindings.trackCatalog = newTrackCatalog
            bindings.trackSelection = newTrackSelection
            bindings.effectiveVideoTrackId = "video:new"
            bindings.videoVariantObservation = newObservation
            bindings.events.clear()
        }

        runBlocking { bridge.selectSourceAsync(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next")) }

        val expectedUiState = bridge.uiState.value
        assertEquals(newTrackCatalog, bridge.trackCatalog.value)
        assertEquals(newTrackSelection, bridge.trackSelection.value)
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(newObservation, bridge.videoVariantObservation.value)

        bindings.trackCatalog = oldTrackCatalog
        bindings.trackSelection = oldTrackSelection
        bindings.effectiveVideoTrackId = "video:old"
        bindings.videoVariantObservation = oldObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale old error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(newTrackCatalog, bridge.trackCatalog.value)
        assertEquals(newTrackSelection, bridge.trackSelection.value)
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(newObservation, bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerAfterDisposeIsIgnored() {
        val staleTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:stale",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val staleTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:stale"),
            )
        val staleObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val bindings =
            FakeBindings(
                trackCatalog = staleTrackCatalog,
                trackSelection = staleTrackSelection,
                effectiveVideoTrackId = "video:stale",
                videoVariantObservation = staleObservation,
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())

        bridge.dispose()
        val expectedUiState = bridge.uiState.value

        bindings.trackCatalog = staleTrackCatalog
        bindings.trackSelection = staleTrackSelection
        bindings.effectiveVideoTrackId = "video:stale"
        bindings.videoVariantObservation = staleObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale disposed error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerAfterResilienceReinitIsIgnored() {
        val oldTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:old",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val oldTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
            )
        val oldObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val reinitTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:reinit",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        val reinitTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.auto(),
            )
        val reinitObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )
        val bindings =
            FakeBindings(
                trackCatalog = oldTrackCatalog,
                trackSelection = oldTrackSelection,
                effectiveVideoTrackId = "video:old",
                videoVariantObservation = oldObservation,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live"),
            )

        runBlocking { bridge.initializeAsync() }
        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())

        bindings.onInitialize = {
            bindings.trackCatalog = reinitTrackCatalog
            bindings.trackSelection = reinitTrackSelection
            bindings.effectiveVideoTrackId = "video:reinit"
            bindings.videoVariantObservation = reinitObservation
            bindings.events.clear()
        }

        bridge.setResiliencePolicy(VesperPlaybackResiliencePolicy.resilient())
        runBlocking { bridge.initializeAsync() }

        val expectedUiState = bridge.uiState.value
        assertEquals(reinitTrackCatalog, bridge.trackCatalog.value)
        assertEquals(reinitTrackSelection, bridge.trackSelection.value)
        assertEquals("video:reinit", bridge.effectiveVideoTrackId.value)
        assertEquals(reinitObservation, bridge.videoVariantObservation.value)

        bindings.trackCatalog = oldTrackCatalog
        bindings.trackSelection = oldTrackSelection
        bindings.effectiveVideoTrackId = "video:old"
        bindings.videoVariantObservation = oldObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale resilience error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(reinitTrackCatalog, bridge.trackCatalog.value)
        assertEquals(reinitTrackSelection, bridge.trackSelection.value)
        assertEquals("video:reinit", bridge.effectiveVideoTrackId.value)
        assertEquals(reinitObservation, bridge.videoVariantObservation.value)
    }

    @Test
    fun resolveVideoVariantObservationUsesRenderedFormat() {
        val observation =
            resolveVideoVariantObservation(
                Format.Builder()
                    .setPeakBitrate(1_500_000)
                    .setWidth(1280)
                    .setHeight(720)
                    .build(),
            )

        assertEquals(
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            ),
            observation,
        )
    }

    @Test
    fun resolveVideoVariantObservationReturnsNilWhenFormatLacksSignal() {
        assertNull(resolveVideoVariantObservation(Format.Builder().build()))
    }

    @Test
    fun resolveEffectiveVideoTrackIdUsesCurrentRenderedFormat() {
        val effectiveTrackId =
            resolveEffectiveVideoTrackId(
                videoTracks =
                    listOf(
                        VesperMediaTrack(
                            id = "group:video-480:0",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 900_000L,
                            width = 854,
                            height = 480,
                            frameRate = 30f,
                        ),
                        VesperMediaTrack(
                            id = "group:video-720:1",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 1_500_000L,
                            width = 1280,
                            height = 720,
                            frameRate = 30f,
                        ),
                    ),
                currentVideoFormat =
                    Format.Builder()
                        .setId("video-720")
                        .setCodecs("avc1.4d401f")
                        .setPeakBitrate(1_500_000)
                        .setWidth(1280)
                        .setHeight(720)
                        .setFrameRate(30f)
                        .build(),
            )

        assertEquals("group:video-720:1", effectiveTrackId)
    }

    @Test
    fun resolveEffectiveVideoTrackIdStaysNilWhenFormatIsTooAmbiguous() {
        val effectiveTrackId =
            resolveEffectiveVideoTrackId(
                videoTracks =
                    listOf(
                        VesperMediaTrack(
                            id = "group:video-480:0",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 900_000L,
                        ),
                        VesperMediaTrack(
                            id = "group:video-720:1",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 1_500_000L,
                        ),
                    ),
                currentVideoFormat =
                    Format.Builder()
                        .setCodecs("avc1.4d401f")
                        .build(),
            )

        assertNull(effectiveTrackId)
    }
}

private fun testVodTimeline(positionMs: Long = 0L): TimelineUiState =
    TimelineUiState(
        kind = TimelineKind.Vod,
        isSeekable = true,
        seekableRange = SeekableRangeUi(0L, 10_000L),
        liveEdgeMs = null,
        positionMs = positionMs,
        durationMs = 10_000L,
    )

private fun subtitleCatalog(vararg ids: String): VesperTrackCatalog =
    VesperTrackCatalog(
        tracks =
            ids.map { id ->
                VesperMediaTrack(
                    id = id,
                    kind = VesperMediaTrackKind.Subtitle,
                )
            },
    )

private class FakeBindings(
    var systemPlaybackActive: Boolean = true,
    var snapshot: NativeBridgeSnapshot? = null,
    var trackCatalog: VesperTrackCatalog = VesperTrackCatalog.Empty,
    var trackSelection: VesperTrackSelectionSnapshot = VesperTrackSelectionSnapshot(),
    var effectiveVideoTrackId: String? = null,
    var videoVariantObservation: VesperVideoVariantObservation? = null,
    var mobilePluginDiagnostics: List<Map<String, Any?>> = emptyList(),
    var nativeStartupDiagnostics: List<Map<String, Any?>> = emptyList(),
    var sourceNormalizerPrepareOutcome: NativeSourceNormalizerResourcePreparedOpenOutcome =
        NativeSourceNormalizerResourcePreparedOpenOutcome(),
    var nativeFramePipelineOpenError: Throwable? = null,
    var nativeFramePipelineAdvanceError: Throwable? = null,
    var nativeFramePipelineReleaseError: Throwable? = null,
    var nativeFramePipelineFlushError: Throwable? = null,
    var nativeFramePipelineSeekError: Throwable? = null,
    var nativeFramePipelineAdvanceStatus: Map<String, Any?>? = null,
    var nativeFramePipelineAdvanceStatuses: MutableList<Map<String, Any?>> = mutableListOf(),
) : VesperNativeBindings {
    override val isSystemPlaybackActive: Boolean
        get() = systemPlaybackActive

    var onInitialize: (() -> Unit)? = null
    var onAdvanceNativeFramePipeline: (() -> Unit)? = null
    var onSeekTo: ((Long) -> Unit)? = null
    val events = mutableListOf<NativeBridgeEvent>()
    var disposeCount = 0
    var openNativeFramePipelineCount = 0
    var advanceNativeFramePipelineCount = 0
    var flushNativeFramePipelineCount = 0
    var closeNativeFramePipelineCount = 0
    var playCount = 0
    var pauseCount = 0
    var stopCount = 0
    var videoTrackSelectionCount = 0
    var audioTrackSelectionCount = 0
    var subtitleTrackSelectionCount = 0
    var trackSelectionChangeGenerationValue = 0L
    var sourceCallbackGenerationValue = 0L
    var subtitleSelectionCommandGenerationValue = 0L
    var subtitleSelectionFailure: NativeTrackSelectionFailure? = null
    var deferSubtitleSelectionConfirmation = false
    var confirmAppliedSubtitleSelectionWithoutRenderer = false
    var abrPolicyCount = 0
    var configureSystemPlaybackCount = 0
    var updateSystemPlaybackMetadataCount = 0
    var clearSystemPlaybackCount = 0
    var refreshSnapshotCount = 0
    var trackCatalogReady = true
    var advertisedSubtitleTrackCount: Int? = null
    var subtitleCatalogFailure: NativeTrackSelectionFailure? = null
    var prepareSourceNormalizerForPlaybackCount = 0
    var disposePreparedSourceNormalizerResourceCount = 0
    var invalidateSystemPlaybackCallbacksCount = 0
    var onPrepareSourceNormalizerForPlayback: (() -> Unit)? = null
    val releasedNativeFramePipelineFrames = mutableListOf<Pair<Long, Boolean>>()
    val seekNativeFramePipelinePositions = mutableListOf<Long>()
    val seekToPositions = mutableListOf<Long>()
    val playbackRates = mutableListOf<Float>()
    var lastProbeSource: VesperPlayerSource? = null
    var lastSourceNormalizerConfiguration: VesperSourceNormalizerConfiguration? = null
    var lastFrameProcessorConfiguration: VesperFrameProcessorConfiguration? = null
    var lastInitializedSource: VesperPlayerSource? = null
    var lastSystemPlaybackUsesSourceNormalizerResource: Boolean? = null
    var lastSystemPlaybackVideoEnabled: Boolean? = null
    var lastPreparedSourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome? = null
    var lastNativeFramePipelineSource: VesperPlayerSource? = null
    var lastNativeFramePipelineSourceNormalizerConfiguration:
        VesperSourceNormalizerConfiguration? = null
    var lastNativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration? = null
    var lastNativeFramePipelineSurfaceKind: NativeVideoSurfaceKind? = null
    private var currentNativeFramePipelineStatus: Map<String, Any?>? = null
    private var updateListener: (() -> Unit)? = null
    private var trackSelectionFailureListener:
        ((NativeTrackSelectionFailure) -> Unit)? = null
    private var deferredSubtitleSelection: VesperTrackSelection? = null
    private var appliedSubtitleSelection: VesperTrackSelection? = null

    override fun probeMobilePlugins(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
    ): List<Map<String, Any?>> {
        lastProbeSource = source
        lastSourceNormalizerConfiguration = sourceNormalizerConfiguration
        lastFrameProcessorConfiguration = frameProcessorConfiguration
        return mobilePluginDiagnostics
    }

    override fun prepareSourceNormalizerForPlayback(
        source: VesperPlayerSource,
        enabled: Boolean,
    ): NativeSourceNormalizerResourcePreparedOpenOutcome {
        prepareSourceNormalizerForPlaybackCount += 1
        onPrepareSourceNormalizerForPlayback?.invoke()
        return sourceNormalizerPrepareOutcome
    }

    override fun disposePreparedSourceNormalizerResource(
        prepared: NativeSourceNormalizerResourcePreparedOpenOutcome,
    ) {
        disposePreparedSourceNormalizerResourceCount += 1
    }

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
        preparedSourceNormalizer: NativeSourceNormalizerResourcePreparedOpenOutcome,
    ): NativeBridgeStartup {
        lastInitializedSource = source
        lastSystemPlaybackUsesSourceNormalizerResource = systemPlaybackUsesSourceNormalizerResource
        lastSystemPlaybackVideoEnabled = systemPlaybackVideoEnabled
        lastPreparedSourceNormalizer = preparedSourceNormalizer
        onInitialize?.invoke()
        return NativeBridgeStartup(subtitle = null, pluginDiagnostics = nativeStartupDiagnostics)
    }

    override fun openNativeFramePipeline(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?> {
        openNativeFramePipelineCount += 1
        nativeFramePipelineOpenError?.let { throw it }
        lastNativeFramePipelineSource = source
        lastNativeFramePipelineSourceNormalizerConfiguration = sourceNormalizerConfiguration
        lastNativeFramePipelineConfiguration = nativeFramePipelineConfiguration
        lastNativeFramePipelineSurfaceKind = surfaceKind
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "opened",
                message = "Android native-frame lifecycle opened for test session.",
            ) + mapOf(
                "handle" to 10L,
                "sourceUri" to source.uri,
                "sourceNormalizerMode" to sourceNormalizerConfiguration.mode.name,
            )
        )
    }

    override fun advanceNativeFramePipeline(): Map<String, Any?> {
        advanceNativeFramePipelineCount += 1
        onAdvanceNativeFramePipeline?.invoke()
        val queuedStatus =
            if (nativeFramePipelineAdvanceStatuses.isNotEmpty()) {
                nativeFramePipelineAdvanceStatuses.removeAt(0)
            } else {
                nativeFramePipelineAdvanceStatus
            }
        queuedStatus?.let { status ->
            return rememberNativeFramePipelineStatus(
                nativeFramePipelineStatus(
                    status = status["status"]?.toString() ?: "frame",
                    message = status["message"]?.toString() ?: "frame",
                ) + status
            )
        }
        nativeFramePipelineAdvanceError?.let { throw it }
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "pending",
                message = "Android MediaCodec native-frame decoder is waiting for input or output",
            )
        )
    }

    override fun releaseNativeFramePipelineFrame(
        frameHandle: Long,
        presented: Boolean,
    ): Map<String, Any?> {
        nativeFramePipelineReleaseError?.let { throw it }
        releasedNativeFramePipelineFrames += frameHandle to presented
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "released",
                message = "released",
                processedFrames = 1L,
                presentedFrames = if (presented) 1L else 0L,
            )
        )
    }

    override fun attachNativeFramePipelineSurface(
        surface: Surface,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?> =
        rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "surfaceAttached",
                message = "presenter surface attached",
            ) + mapOf(
                "presenterReady" to true,
                "presenterConfigured" to true,
                "presenterState" to "ready",
                "surfaceAttached" to true,
                "surfaceProfile" to surfaceKind.name,
            )
        )

    override fun detachNativeFramePipelineSurface(): Map<String, Any?> =
        rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "surfaceDetached",
                message = "presenter surface detached",
            ) + mapOf(
                "presenterReady" to false,
                "presenterConfigured" to false,
                "presenterState" to "waitingForSurface",
                "surfaceAttached" to false,
            )
        )

    override fun flushNativeFramePipeline(): Map<String, Any?> {
        flushNativeFramePipelineCount += 1
        nativeFramePipelineFlushError?.let { throw it }
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "flushed",
                message = "flushed",
                processedFrames = 1L,
                presentedFrames = 1L,
            )
        )
    }

    override fun seekNativeFramePipeline(positionMs: Long): Map<String, Any?> {
        seekNativeFramePipelinePositions.add(positionMs)
        nativeFramePipelineSeekError?.let { throw it }
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "seeked",
                message = "seeked",
                processedFrames = 1L,
                presentedFrames = 1L,
            )
        )
    }

    override fun currentNativeFramePipelineStatus(): Map<String, Any?>? =
        currentNativeFramePipelineStatus

    fun setCurrentNativeFramePipelineStatusForTest(status: Map<String, Any?>) {
        currentNativeFramePipelineStatus = status
    }

    override fun closeNativeFramePipeline() {
        currentNativeFramePipelineStatus = null
        if (openNativeFramePipelineCount > closeNativeFramePipelineCount) {
            closeNativeFramePipelineCount += 1
        }
    }

    private fun rememberNativeFramePipelineStatus(
        status: Map<String, Any?>,
    ): Map<String, Any?> {
        currentNativeFramePipelineStatus = status
        return status
    }

    override fun dispose() {
        disposeCount += 1
    }

    override fun invalidateSystemPlaybackCallbacks() {
        invalidateSystemPlaybackCallbacksCount += 1
        sourceCallbackGenerationValue += 1L
        events.clear()
    }

    override fun refreshSnapshot() {
        refreshSnapshotCount += 1
    }

    override fun currentTrackCatalog(): VesperTrackCatalog = trackCatalog

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = trackSelection

    override fun currentAppliedSubtitleSelection(): VesperTrackSelection =
        appliedSubtitleSelection ?: trackSelection.subtitle

    override fun currentAdvertisedSubtitleTrackCount(): Int =
        advertisedSubtitleTrackCount ?: trackCatalog.subtitleTracks.size

    override val trackSelectionChangeGeneration: Long
        get() = trackSelectionChangeGenerationValue

    override val sourceCallbackGeneration: Long
        get() = sourceCallbackGenerationValue

    override val subtitleSelectionCommandGeneration: Long
        get() = subtitleSelectionCommandGenerationValue

    override fun isTrackCatalogReady(): Boolean = trackCatalogReady

    override fun currentSubtitleCatalogFailure(): NativeTrackSelectionFailure? =
        subtitleCatalogFailure

    override fun currentEffectiveVideoTrackId(): String? = effectiveVideoTrackId

    override fun currentVideoVariantObservation(): VesperVideoVariantObservation? =
        videoVariantObservation

    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = null

    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) {
        updateListener = listener
    }

    override fun setOnTrackSelectionFailureListener(
        listener: ((NativeTrackSelectionFailure) -> Unit)?,
    ) {
        trackSelectionFailureListener = listener
    }

    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) = Unit

    override fun detachSurface() = Unit

    override fun pollSnapshot(): NativeBridgeSnapshot? = snapshot

    override fun drainEvents(): List<NativeBridgeEvent> = events.toList().also { events.clear() }

    override fun play() {
        playCount += 1
    }

    override fun pause() {
        pauseCount += 1
    }

    override fun stop() {
        stopCount += 1
    }

    override fun seekTo(positionMs: Long) {
        seekToPositions += positionMs
        onSeekTo?.invoke(positionMs)
    }

    override fun setPlaybackRate(rate: Float) {
        playbackRates += rate
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        videoTrackSelectionCount += 1
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        audioTrackSelectionCount += 1
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        subtitleTrackSelectionCount += 1
        subtitleSelectionCommandGenerationValue += 1L
        subtitleSelectionFailure?.let { failure ->
            emitSubtitleSelectionFailure(failure, subtitleSelectionCommandGenerationValue)
            return
        }
        if (deferSubtitleSelectionConfirmation) {
            deferredSubtitleSelection = selection
            return
        }
        if (confirmAppliedSubtitleSelectionWithoutRenderer) {
            appliedSubtitleSelection = selection
            trackSelectionChangeGenerationValue += 1L
            updateListener?.invoke()
            return
        }
        confirmSubtitleSelection(selection)
    }

    fun confirmDeferredSubtitleSelection() {
        val selection = deferredSubtitleSelection ?: return
        deferredSubtitleSelection = null
        confirmSubtitleSelection(selection)
    }

    fun emitSubtitleSelectionFailure(
        failure: NativeTrackSelectionFailure,
        commandGeneration: Long,
        sourceCallbackGeneration: Long = sourceCallbackGenerationValue,
    ) {
        trackSelectionFailureListener?.invoke(
            failure.copy(
                sourceCallbackGeneration = sourceCallbackGeneration,
                commandGeneration = commandGeneration,
            )
        )
    }

    private fun confirmSubtitleSelection(selection: VesperTrackSelection) {
        appliedSubtitleSelection = selection
        trackSelection = trackSelection.copy(subtitle = selection)
        trackSelectionChangeGenerationValue += 1L
        updateListener?.invoke()
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        abrPolicyCount += 1
    }

    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) {
        configureSystemPlaybackCount += 1
    }

    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) {
        updateSystemPlaybackMetadataCount += 1
    }

    override fun clearSystemPlayback() {
        clearSystemPlaybackCount += 1
    }

    fun currentUpdateListener(): (() -> Unit)? = updateListener

private fun nativeFramePipelineStatus(
        status: String,
        message: String,
        processedFrames: Long = 0L,
        presentedFrames: Long = 0L,
    ): Map<String, Any?> =
        mapOf(
            "status" to status,
            "route" to "sdkManagedNativeFrame",
            "participation" to "selected",
            "sourceInput" to "sourceNormalizerPacket",
            "decoderAdapter" to "MediaCodec",
            "presenterProfile" to "SurfaceView",
            "presenterReady" to false,
            "presenterConfigured" to false,
            "presenterState" to "waitingForSurface",
            "surfaceAttached" to false,
            "pipelineProfile" to "media_codec_surface_texture",
            "message" to message,
            "counters" to
                mapOf(
                    "processedFrames" to processedFrames,
                    "presentedFrames" to presentedFrames,
                    "deadlineMisses" to 0L,
                    "backpressureCount" to 0L,
                    "lateDropped" to 0L,
                ),
        )

    fun nativeFramePipelineStatusForTest(
        vararg overrides: Pair<String, Any?>,
    ): Map<String, Any?> =
        nativeFramePipelineStatus(
            status = overrides.firstOrNull { it.first == "status" }?.second?.toString()
                ?: "pending",
            message = overrides.firstOrNull { it.first == "message" }?.second?.toString()
                ?: "test status",
        ) + overrides.toMap()
}

private fun waitUntil(timeoutMs: Long = 5_000L, predicate: () -> Boolean): Boolean {
    val deadlineNs = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMs)
    while (System.nanoTime() < deadlineNs) {
        if (predicate()) {
            return true
        }
        Thread.sleep(10L)
    }
    return predicate()
}

private class ManualNativeFramePipelinePumpScheduler(
    private val beforeRun: (() -> Unit)? = null,
) : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = true
    private var scheduledAction: (() -> Unit)? = null
    var cancelCount = 0
        private set
    var closeCount = 0
        private set
    var lastDelayMs: Long? = null
        private set
    private var closed = false

    override fun schedule(delayMs: Long, action: () -> Unit) {
        if (closed) {
            return
        }
        lastDelayMs = delayMs
        scheduledAction = action
    }

    override fun execute(action: () -> Unit) {
        if (closed) {
            return
        }
        action()
    }

    override fun cancel() {
        cancelCount += 1
        scheduledAction = null
    }

    override fun close() {
        closeCount += 1
        closed = true
        cancel()
    }

    fun hasPendingActions(): Boolean = scheduledAction != null

    fun runNext() {
        scheduledAction.also { scheduledAction = null }?.let { action ->
            beforeRun?.invoke()
            action()
        }
    }
}

private class QueuedNativeFramePipelinePumpScheduler : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = true
    private val actions = ArrayDeque<() -> Unit>()

    override fun schedule(delayMs: Long, action: () -> Unit) {
        actions.addLast(action)
    }

    override fun execute(action: () -> Unit) {
        actions.addLast(action)
    }

    override fun cancel() {
        actions.clear()
    }

    fun hasPendingActions(): Boolean = actions.isNotEmpty()

    fun runNext() {
        actions.removeFirstOrNull()?.invoke()
    }
}

private class CoalescingQueuedNativeFramePipelinePumpScheduler : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = true
    private val scheduledActions = ArrayDeque<() -> Unit>()
    private val runtimeCommands = BoundedNativeFramePipelineRuntimeCommandQueue()

    val pendingActionCount: Int
        get() = scheduledActions.size + runtimeCommands.size

    override fun schedule(delayMs: Long, action: () -> Unit) {
        scheduledActions.addLast(action)
    }

    override fun execute(action: () -> Unit) {
        executeCommand(
            NativeFramePipelineRuntimeCommand(
                operation = "generic",
                action = action,
            )
        )
    }

    override fun executeCommand(command: NativeFramePipelineRuntimeCommand) {
        assertTrue("test runtime command should be accepted", runtimeCommands.enqueue(command))
    }

    override fun cancel() {
        scheduledActions.clear()
    }

    fun hasPendingActions(): Boolean = pendingActionCount > 0

    fun runNext() {
        runtimeCommands.removeFirstOrNull()?.let { command ->
            command.action()
            return
        }
        scheduledActions.removeFirstOrNull()?.invoke()
    }

    fun runUntilIdle() {
        var guard = 256
        while (hasPendingActions() && guard > 0) {
            guard -= 1
            runNext()
        }
        assertFalse("test scheduler did not drain", hasPendingActions())
    }
}

private class ThreadedNativeFramePipelinePumpScheduler(
    expectedRuns: Int = 2,
) : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = true
    @Volatile
    var lastError: Throwable? = null
        private set

    private var latch = CountDownLatch(expectedRuns)
    private var closed = false

    @Synchronized
    override fun schedule(delayMs: Long, action: () -> Unit) {
        if (closed) {
            return
        }
        val currentLatch = latch
        Thread {
            try {
                action()
            } catch (error: Throwable) {
                lastError = error
            } finally {
                currentLatch.countDown()
            }
        }.start()
    }

    @Synchronized
    override fun cancel() = Unit

    @Synchronized
    override fun close() {
        closed = true
    }

    fun awaitRun(): Boolean = latch.await(5, TimeUnit.SECONDS)
}
