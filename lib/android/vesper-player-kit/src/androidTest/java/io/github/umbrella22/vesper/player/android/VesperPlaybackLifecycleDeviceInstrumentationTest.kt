package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.PixelCopy
import android.view.SurfaceView
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.absoluteValue
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Physical-device regression coverage for local Media3 playback and surface lifecycle handling. */
@RunWith(AndroidJUnit4::class)
class VesperPlaybackLifecycleDeviceInstrumentationTest {
    @Test
    fun localPlaybackInitializesDecoderRendersFirstFrameAndAdvancesPosition() {
        withPlaybackSession("local-playback") { _, controller, _, events, fixtureDurationMs ->
            val proof = awaitPlaybackProof(controller, events)
            val sustainedRange =
                if (fixtureDurationMs >= MINIMUM_SUSTAINED_FIXTURE_DURATION_MS) {
                    observeSustainedPlayback(controller, events)
                } else {
                    null
                }
            val state = sample(controller, events)

            assertNull(state.lastError)
            Log.i(
                EVIDENCE_TAG,
                "test=local-playback decoderNames=${decoderNames(events)} " +
                    "firstFrame=${events.hasEvent(FIRST_FRAME_EVENT)} " +
                    "positions=${proof.minimumPositionMs},${proof.maximumPositionMs}," +
                    "${sustainedRange?.first},${sustainedRange?.second}," +
                    "${state.timeline.positionMs} droppedVideoFrames=${droppedVideoFrameEvidence(events)}",
            )
        }
    }

    @Test
    fun optInNetworkPlaybackValidatesDecoderTimelineAndControls() {
        val fixture = resolveNetworkFixture()

        withSourceSession(
            testId = "network-${fixture.protocol.name.lowercase()}",
            source = fixture.source,
            sourceDurationMs = -1L,
        ) { scenario, controller, _, events, _ ->
            val proof =
                awaitPlaybackProof(
                    controller = controller,
                    events = events,
                    timeoutSeconds = NETWORK_PLAYBACK_TIMEOUT_SECONDS,
                    requireTimelineAdvance =
                        fixture.expectedTimelineKind == null ||
                            fixture.expectedTimelineKind == TimelineKind.Vod,
                )
            val startedState = sample(controller, events)
            if (fixture.expectedTimelineKind != null) {
                assertTrue(
                    "expected timeline ${fixture.expectedTimelineKind}: " +
                        describe(startedState, events),
                    startedState.timeline.kind == fixture.expectedTimelineKind,
                )
            }

            val controlEvidence =
                if (startedState.timeline.kind == TimelineKind.LiveDvr) {
                    validateLiveDvrNetworkControls(scenario, controller, events)
                } else {
                    validateVodNetworkControls(scenario, controller, events)
                }
            val finalState = sample(controller, events)
            assertNull(finalState.lastError)
            Log.i(
                EVIDENCE_TAG,
                "test=network-${fixture.protocol.name.lowercase()} protocol=${fixture.protocol} " +
                    "expectedTimeline=${fixture.expectedTimelineKind} " +
                    "actualTimeline=${finalState.timeline.kind} " +
                    "decoderNames=${decoderNames(events)} firstFrame=${events.hasEvent(FIRST_FRAME_EVENT)} " +
                    "positions=${proof.minimumPositionMs},${proof.maximumPositionMs} " +
                    "controls=$controlEvidence droppedVideoFrames=${droppedVideoFrameEvidence(events)}",
            )
        }
    }

    @Test
    fun optInNetworkHostPauseDetachPolicySurvivesStoppedAndResumedActivityStates() {
        val fixture = resolveNetworkFixture()

        withSourceSession(
            testId = "network-host-stop-resume",
            source = fixture.source,
            sourceDurationMs = -1L,
        ) { scenario, controller, surfaceHost, events, _ ->
            val proof =
                awaitPlaybackProof(
                    controller = controller,
                    events = events,
                    timeoutSeconds = NETWORK_PLAYBACK_TIMEOUT_SECONDS,
                    requireTimelineAdvance =
                        fixture.expectedTimelineKind == null ||
                            fixture.expectedTimelineKind == TimelineKind.Vod,
                )
            val startedState = sample(controller, events)
            if (fixture.expectedTimelineKind != null) {
                assertTrue(
                    "expected timeline ${fixture.expectedTimelineKind}: " +
                        describe(startedState, events),
                    startedState.timeline.kind == fixture.expectedTimelineKind,
                )
            }

            val lifecycle = validateHostPauseDetachPolicy(scenario, controller, surfaceHost, events)
            Log.i(
                EVIDENCE_TAG,
                "test=network-host-stop-resume protocol=${fixture.protocol} " +
                    "expectedTimeline=${fixture.expectedTimelineKind} " +
                    "actualTimeline=${lifecycle.finalState.timeline.kind} " +
                    "policy=explicit-pause-detach decoderNames=${decoderNames(events)} " +
                    "firstFrame=${events.hasEvent(FIRST_FRAME_EVENT)} " +
                    "pixelCopy=${lifecycle.copiedFrame.result} " +
                    "surface=${lifecycle.copiedFrame.width}x${lifecycle.copiedFrame.height} " +
                    "positions=${proof.minimumPositionMs},${lifecycle.stoppedPositionRange.first}," +
                    "${lifecycle.stoppedPositionRange.second}," +
                    "${lifecycle.positionAfterCopiedFrameMs}," +
                    "${lifecycle.finalState.timeline.positionMs}",
            )
        }
    }

    @Test
    fun optInNetworkPlaybackRecoversAfterControlledOutage() {
        val arguments = InstrumentationRegistry.getArguments()
        val outageControlUrl = arguments.getString(NETWORK_OUTAGE_CONTROL_URL_ARGUMENT)
        assumeTrue(
            "requires -e $NETWORK_OUTAGE_CONTROL_URL_ARGUMENT <http-url>",
            !outageControlUrl.isNullOrBlank(),
        )
        val outageSeconds =
            arguments.getString(NETWORK_OUTAGE_SECONDS_ARGUMENT)?.toLongOrNull()
                ?: DEFAULT_NETWORK_OUTAGE_SECONDS
        require(outageSeconds in 1L..MAX_NETWORK_OUTAGE_SECONDS) {
            "$NETWORK_OUTAGE_SECONDS_ARGUMENT must be between 1 and " +
                "$MAX_NETWORK_OUTAGE_SECONDS: $outageSeconds"
        }
        val fixture = resolveNetworkFixture()

        withSourceSession(
            testId = "network-controlled-outage",
            source = fixture.source,
            sourceDurationMs = -1L,
        ) { _, controller, _, events, _ ->
            awaitPlaybackProof(
                controller = controller,
                events = events,
                timeoutSeconds = NETWORK_PLAYBACK_TIMEOUT_SECONDS,
                requireTimelineAdvance =
                    fixture.expectedTimelineKind == null ||
                        fixture.expectedTimelineKind == TimelineKind.Vod,
            )
            val startedState = sample(controller, events)
            if (fixture.expectedTimelineKind != null) {
                assertTrue(
                    "expected timeline ${fixture.expectedTimelineKind}: " +
                        describe(startedState, events),
                    startedState.timeline.kind == fixture.expectedTimelineKind,
                )
            }

            val outageEventStart = events.size
            val outageStartedAtNanos = System.nanoTime()
            val outageResponse = triggerControlledOutage(outageControlUrl!!, outageSeconds)
            var outageState = sample(controller, events)
            val retryObserved =
                awaitCondition(NETWORK_OUTAGE_SIGNAL_TIMEOUT_SECONDS) {
                    outageState = sample(controller, events)
                    val outageEvents = events.drop(outageEventStart)
                    outageState.lastError == null &&
                        outageEvents.hasEvent(LOAD_ERROR_EVENT) &&
                        outageEvents.hasEvent(RETRY_SCHEDULED_EVENT)
                }
            assertTrue(
                "controlled outage did not schedule a retry: ${describe(outageState, events)}",
                retryObserved,
            )

            val settleDeadlineNanos =
                outageStartedAtNanos +
                    TimeUnit.SECONDS.toNanos(outageSeconds) +
                    TimeUnit.MILLISECONDS.toNanos(NETWORK_RECOVERY_SETTLE_DELAY_MS)
            while (System.nanoTime() < settleDeadlineNanos && outageState.lastError == null) {
                Thread.sleep(POLL_INTERVAL_MS)
                outageState = sample(controller, events)
            }
            assertNull(outageState.lastError)

            val recoveryStartPositionMs = outageState.timeline.positionMs
            assertPlayerCondition(
                description = "playback to remain active after the controlled outage",
                controller = controller,
                events = events,
                timeoutSeconds = NETWORK_RECOVERY_TIMEOUT_SECONDS,
            ) { state ->
                state.playbackState == PlaybackStateUi.Playing &&
                    !state.isBuffering &&
                    state.timeline.positionMs >=
                    recoveryStartPositionMs + MINIMUM_NETWORK_RECOVERY_ADVANCE_MS
            }
            observeActivePlayback(
                controller = controller,
                events = events,
                durationSeconds = NETWORK_POST_RECOVERY_OBSERVATION_SECONDS,
            )
            val finalState = sample(controller, events)
            val outageEvents =
                events.drop(outageEventStart).filter {
                    it.eventName == LOAD_ERROR_EVENT || it.eventName == RETRY_SCHEDULED_EVENT
                }
            assertNull(finalState.lastError)
            Log.i(
                EVIDENCE_TAG,
                "test=network-controlled-outage protocol=${fixture.protocol} " +
                    "timeline=${finalState.timeline.kind} outageSeconds=$outageSeconds " +
                    "controlResponse=$outageResponse recoveryPositions=" +
                    "$recoveryStartPositionMs,${finalState.timeline.positionMs} " +
                    "outageEvents=${outageEvents.map { it.eventName to it.attributes }}",
            )
        }
    }

    @Test
    fun optInNetworkPlaybackSurvivesExtendedLifecycle() {
        val arguments = InstrumentationRegistry.getArguments()
        val observationSeconds =
            arguments.getString(NETWORK_LONG_RUN_SECONDS_ARGUMENT)?.toLongOrNull()
        assumeTrue(
            "requires -e $NETWORK_LONG_RUN_SECONDS_ARGUMENT <$MINIMUM_NETWORK_LONG_RUN_SECONDS.." +
                "$MAXIMUM_NETWORK_LONG_RUN_SECONDS>",
            observationSeconds != null,
        )
        val resolvedObservationSeconds = requireNotNull(observationSeconds)
        require(
            resolvedObservationSeconds in
                MINIMUM_NETWORK_LONG_RUN_SECONDS..MAXIMUM_NETWORK_LONG_RUN_SECONDS
        ) {
            "$NETWORK_LONG_RUN_SECONDS_ARGUMENT must be between " +
                "$MINIMUM_NETWORK_LONG_RUN_SECONDS and $MAXIMUM_NETWORK_LONG_RUN_SECONDS: " +
                "$resolvedObservationSeconds"
        }
        val fixture = resolveNetworkFixture()
        val evidenceContext = InstrumentationRegistry.getInstrumentation().context
        VesperAndroidDeviceEvidenceWriter(
            context = evidenceContext,
            testId = NETWORK_LONG_RUN_EVIDENCE_ID,
        ).use { evidence ->
            Log.i(EVIDENCE_TAG, "test=network-extended-lifecycle evidence=${evidence.file.absolutePath}")
            evidence.record(
                phase = "pre_player",
                details =
                    mapOf(
                        "observationSeconds" to resolvedObservationSeconds,
                        "protocol" to fixture.protocol.name,
                    ),
            )
            try {
                withSourceSession(
                    testId = "network-extended-lifecycle",
                    source = fixture.source,
                    sourceDurationMs = -1L,
                    keepScreenOnDuringPlayback = true,
                ) { scenario, controller, surfaceHost, events, _ ->
                    awaitPlaybackProof(
                        controller = controller,
                        events = events,
                        timeoutSeconds = NETWORK_PLAYBACK_TIMEOUT_SECONDS,
                        requireTimelineAdvance =
                            fixture.expectedTimelineKind == null ||
                                fixture.expectedTimelineKind == TimelineKind.Vod,
                    )
                    val startedState = sample(controller, events)
                    if (fixture.expectedTimelineKind != null) {
                        assertTrue(
                            "expected timeline ${fixture.expectedTimelineKind}: " +
                                describe(startedState, events),
                            startedState.timeline.kind == fixture.expectedTimelineKind,
                        )
                    }
                    evidence.record(
                        phase = "playback_ready",
                        state = startedState,
                        events = events,
                        details = mapOf("scenarioState" to scenario.state.name),
                    )

                    var observedSeconds = 0L
                    var lifecycleCycles = 0
                    while (observedSeconds < resolvedObservationSeconds) {
                        val chunkSeconds =
                            minOf(
                                NETWORK_LONG_RUN_CHUNK_SECONDS,
                                resolvedObservationSeconds - observedSeconds,
                            )
                        evidence.record(
                            phase = "active_chunk_start",
                            state = sample(controller, events),
                            events = events,
                            details =
                                longRunDetails(
                                    observedSeconds = observedSeconds,
                                    lifecycleCycles = lifecycleCycles,
                                    scenarioState = scenario.state.name,
                                ),
                        )
                        observeActivePlayback(controller, events, chunkSeconds)
                        observedSeconds += chunkSeconds
                        evidence.record(
                            phase = "active_chunk_end",
                            state = sample(controller, events),
                            events = events,
                            details =
                                longRunDetails(
                                    observedSeconds = observedSeconds,
                                    lifecycleCycles = lifecycleCycles,
                                    scenarioState = scenario.state.name,
                                ),
                        )
                        if (observedSeconds < resolvedObservationSeconds) {
                            val lifecycle =
                                validateHostPauseDetachPolicy(
                                    scenario = scenario,
                                    controller = controller,
                                    surfaceHost = surfaceHost,
                                    events = events,
                                    evidence = evidence,
                                    observedSeconds = observedSeconds,
                                    lifecycleCycle = lifecycleCycles + 1,
                                )
                            lifecycleCycles += 1
                            Log.i(
                                EVIDENCE_TAG,
                                "test=network-extended-lifecycle progressSeconds=$observedSeconds " +
                                    "lifecycleCycles=$lifecycleCycles " +
                                    "timeline=${lifecycle.finalState.timeline.kind} " +
                                    "positionMs=${lifecycle.finalState.timeline.positionMs} " +
                                    "pixelCopy=${lifecycle.copiedFrame.result} " +
                                    "frameHash=${lifecycle.copiedFrame.sampleHash}",
                            )
                        }
                    }

                    val finalState = sample(controller, events)
                    assertNull(finalState.lastError)
                    assertTrue(
                        "extended playback did not finish active: ${describe(finalState, events)}",
                        finalState.playbackState == PlaybackStateUi.Playing && !finalState.isBuffering,
                    )
                    val benchmarkSummary = controller.benchmarkSummary()
                    evidence.record(
                        phase = "playback_final",
                        state = finalState,
                        events = events,
                        details =
                            longRunDetails(
                                observedSeconds = observedSeconds,
                                lifecycleCycles = lifecycleCycles,
                                scenarioState = scenario.state.name,
                            ) +
                                mapOf(
                                    "benchmarkAcceptedEvents" to benchmarkSummary.acceptedEvents,
                                    "benchmarkDroppedEvents" to benchmarkSummary.droppedEvents,
                                ),
                    )
                    Log.i(
                        EVIDENCE_TAG,
                        "test=network-extended-lifecycle protocol=${fixture.protocol} " +
                            "timeline=${finalState.timeline.kind} observedSeconds=$observedSeconds " +
                            "lifecycleCycles=$lifecycleCycles decoderNames=${decoderNames(events)} " +
                            "firstFrame=${events.hasEvent(FIRST_FRAME_EVENT)} " +
                            "finalPositionMs=${finalState.timeline.positionMs} " +
                            "benchmarkAcceptedEvents=${benchmarkSummary.acceptedEvents} " +
                            "benchmarkDroppedEvents=${benchmarkSummary.droppedEvents} " +
                            "droppedVideoFrames=${droppedVideoFrameEvidence(events)}",
                    )
                }
                evidence.record(phase = "post_dispose", captureDetailedMemory = false)
                Runtime.getRuntime().gc()
                System.runFinalization()
                Thread.sleep(POST_DISPOSE_GC_SETTLE_MS)
                evidence.record(phase = "post_dispose_gc", captureDetailedMemory = false)
            } catch (error: Throwable) {
                evidence.record(
                    phase = "failed",
                    details =
                        mapOf(
                            "errorType" to error::class.java.name,
                            "errorMessage" to error.message,
                        ),
                    captureDetailedMemory = false,
                )
                throw error
            }
        }
    }

    @Test
    fun pauseResumeAndSeekPreserveRealPlaybackProgress() {
        withPlaybackSession("pause-resume-seek", requiresLongFixture = true) {
                scenario, controller, _, events, _ ->
            val proof = awaitPlaybackProof(controller, events)

            scenario.onActivity { controller.pause() }
            assertPlayerCondition(
                description = "player to enter paused state",
                controller = controller,
                events = events,
            ) { state ->
                state.playbackState == PlaybackStateUi.Paused && !state.isBuffering
            }
            val pausedRange = assertPositionStableWhilePaused(controller, events)

            scenario.onActivity { controller.play() }
            assertPlayerCondition(
                description = "position to advance after resume",
                controller = controller,
                events = events,
            ) { state ->
                state.playbackState == PlaybackStateUi.Playing &&
                    state.timeline.positionMs >= pausedRange.second + MINIMUM_RESUME_ADVANCE_MS
            }
            val resumedPositionMs = sample(controller, events).timeline.positionMs

            scenario.onActivity { controller.pause() }
            assertPlayerCondition(
                description = "player to pause before seek",
                controller = controller,
                events = events,
            ) { state -> state.playbackState == PlaybackStateUi.Paused }
            val beforeSeek = sample(controller, events)
            val durationMs = requireNotNull(beforeSeek.timeline.durationMs)
            assertTrue(
                "fixture timeline must be seekable: ${describe(beforeSeek, events)}",
                beforeSeek.timeline.isSeekable,
            )
            val seekTargetMs = durationMs * SEEK_PERCENT / 100L

            scenario.onActivity { controller.seekToRatio(SEEK_RATIO) }
            assertPlayerCondition(
                description = "seek position to converge on $seekTargetMs ms",
                controller = controller,
                events = events,
            ) { state ->
                state.playbackState == PlaybackStateUi.Paused &&
                    (state.timeline.positionMs - seekTargetMs).absoluteValue <= SEEK_TOLERANCE_MS
            }
            val seekPositionMs = sample(controller, events).timeline.positionMs
            assertTrue(
                "seek did not move far enough: before=${beforeSeek.timeline.positionMs}, " +
                    "after=$seekPositionMs, target=$seekTargetMs",
                (seekPositionMs - beforeSeek.timeline.positionMs).absoluteValue >= MINIMUM_SEEK_DISTANCE_MS,
            )

            scenario.onActivity { controller.play() }
            assertPlayerCondition(
                description = "position to advance after seek",
                controller = controller,
                events = events,
            ) { state ->
                state.playbackState == PlaybackStateUi.Playing &&
                    state.timeline.positionMs >= seekPositionMs + MINIMUM_POST_SEEK_ADVANCE_MS
            }
            val finalState = sample(controller, events)
            assertNull(finalState.lastError)
            Log.i(
                EVIDENCE_TAG,
                "test=pause-resume-seek decoderNames=${decoderNames(events)} " +
                    "firstFrame=${events.hasEvent(FIRST_FRAME_EVENT)} " +
                    "positions=${proof.minimumPositionMs},${pausedRange.first},${pausedRange.second}," +
                    "$resumedPositionMs,$seekPositionMs,${finalState.timeline.positionMs} " +
                    "droppedVideoFrames=${droppedVideoFrameEvidence(events)}",
            )
        }
    }

    @Test
    fun explicitSurfaceDetachAndActivityRecreationKeepPlaybackAdvancing() {
        withPlaybackSession("surface-recreation", requiresLongFixture = true) {
                scenario, controller, surfaceHost, events, _ ->
            val proof = awaitPlaybackProof(controller, events)
            val firstFrameCountBeforeDetach = events.count { it.eventName == FIRST_FRAME_EVENT }

            scenario.onActivity { controller.detachSurfaceHost(surfaceHost.value) }
            scenario.recreate()
            scenario.onActivity { activity ->
                surfaceHost.value = activity.replaceSurfaceHost()
                controller.attachSurfaceHost(surfaceHost.value)
            }

            val copiedFrame = awaitCopiedSurfaceFrame(scenario, surfaceHost, controller, events)

            val positionAfterAttachMs = sample(controller, events).timeline.positionMs
            assertPlayerCondition(
                description = "position to advance after Activity and SurfaceView recreation",
                controller = controller,
                events = events,
            ) { state ->
                state.playbackState == PlaybackStateUi.Playing &&
                    state.timeline.positionMs >= positionAfterAttachMs + MINIMUM_SURFACE_REATTACH_ADVANCE_MS
            }
            val finalState = sample(controller, events)
            val firstFrameCountAfterAttach = events.count { it.eventName == FIRST_FRAME_EVENT }
            assertNull(finalState.lastError)
            Log.i(
                EVIDENCE_TAG,
                "test=surface-recreation decoderNames=${decoderNames(events)} " +
                    "firstFrameCounts=$firstFrameCountBeforeDetach,$firstFrameCountAfterAttach " +
                    "pixelCopy=${copiedFrame.result} surface=${copiedFrame.width}x${copiedFrame.height} " +
                    "centerPixel=${copiedFrame.centerPixel} " +
                    "positions=${proof.minimumPositionMs},$positionAfterAttachMs," +
                    "${finalState.timeline.positionMs} " +
                    "droppedVideoFrames=${droppedVideoFrameEvidence(events)}",
            )
        }
    }

    /**
     * Verifies a host-owned pause and surface-detach policy while stopped. This does not claim that
     * the SDK automatically manages background playback or guarantees lock-screen audio behavior.
     */
    @Test
    fun hostExplicitPauseDetachPolicySurvivesStoppedAndResumedActivityStates() {
        withPlaybackSession("host-stop-resume", requiresLongFixture = true) {
                scenario, controller, surfaceHost, events, _ ->
            val proof = awaitPlaybackProof(controller, events)
            val lifecycle = validateHostPauseDetachPolicy(scenario, controller, surfaceHost, events)
            Log.i(
                EVIDENCE_TAG,
                "test=host-stop-resume policy=explicit-pause-detach decoderNames=${decoderNames(events)} " +
                    "firstFrame=${events.hasEvent(FIRST_FRAME_EVENT)} " +
                    "pixelCopy=${lifecycle.copiedFrame.result} " +
                    "surface=${lifecycle.copiedFrame.width}x${lifecycle.copiedFrame.height} " +
                    "positions=${proof.minimumPositionMs},${lifecycle.stoppedPositionRange.first}," +
                    "${lifecycle.stoppedPositionRange.second}," +
                    "${lifecycle.positionAfterCopiedFrameMs}," +
                    "${lifecycle.finalState.timeline.positionMs} " +
                    "droppedVideoFrames=${droppedVideoFrameEvidence(events)}",
            )
        }
    }

    private fun withPlaybackSession(
        testId: String,
        requiresLongFixture: Boolean = false,
        block: (
            ActivityScenario<VesperSurfaceLayoutTestActivity>,
            VesperPlayerController,
            SurfaceHostReference,
            MutableList<VesperBenchmarkEvent>,
            Long,
        ) -> Unit,
    ) {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val fixture = resolveFixture(context, testId)
        try {
            val fixtureDurationMs = fixtureDurationMs(fixture.file)
            Log.i(
                EVIDENCE_TAG,
                "test=$testId fixtureDurationMs=$fixtureDurationMs " +
                    "fixtureSource=${if (fixture.deleteAfterTest) "asset" else "runner"}",
            )
            if (requiresLongFixture) {
                assumeTrue(
                    "requires -e $FIXTURE_FILE_ARGUMENT <absolute-readable-file> with duration >= " +
                        "$MINIMUM_LONG_FIXTURE_DURATION_MS ms; resolved=${fixture.file.absolutePath}, " +
                        "duration=$fixtureDurationMs ms",
                    fixtureDurationMs >= MINIMUM_LONG_FIXTURE_DURATION_MS,
                )
            }

            val source =
                VesperPlayerSource(
                    uri = Uri.fromFile(fixture.file).toString(),
                    label = "device lifecycle fixture",
                    kind = VesperPlayerSourceKind.Local,
                    protocol = VesperPlayerSourceProtocol.Progressive,
                )
            withSourceSession(
                testId = testId,
                source = source,
                sourceDurationMs = fixtureDurationMs,
                block = block,
            )
        } finally {
            if (fixture.deleteAfterTest) {
                check(fixture.file.delete() || !fixture.file.exists()) {
                    "failed to delete copied playback fixture ${fixture.file.absolutePath}"
                }
            }
        }
    }

    private fun withSourceSession(
        testId: String,
        source: VesperPlayerSource,
        sourceDurationMs: Long,
        keepScreenOnDuringPlayback: Boolean = false,
        block: (
            ActivityScenario<VesperSurfaceLayoutTestActivity>,
            VesperPlayerController,
            SurfaceHostReference,
            MutableList<VesperBenchmarkEvent>,
            Long,
        ) -> Unit,
    ) {
        val events = mutableListOf<VesperBenchmarkEvent>()
        var controller: VesperPlayerController? = null

        ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
            try {
                lateinit var surfaceHost: SurfaceHostReference
                scenario.onActivity { activity ->
                    surfaceHost = SurfaceHostReference(activity.replaceSurfaceHost())
                    controller =
                        VesperPlayerControllerFactory.createDefault(
                            context = activity.applicationContext,
                            initialSource = source,
                            resiliencePolicy = VesperPlaybackResiliencePolicy.streaming(),
                            decoderBackend = VesperDecoderBackend.SystemOnly,
                            surfaceKind = VesperVideoSurfaceKind.SurfaceView,
                            keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
                            benchmarkConfiguration =
                                VesperBenchmarkConfiguration(
                                    enabled = true,
                                    maxBufferedEvents = MAX_BUFFERED_BENCHMARK_EVENTS,
                                ),
                        ).also { player ->
                            player.attachSurfaceHost(surfaceHost.value)
                            player.initialize()
                            player.play()
                        }
                }
                block(
                    scenario,
                    requireNotNull(controller),
                    surfaceHost,
                    events,
                    sourceDurationMs,
                )
            } finally {
                scenario.onActivity { activity ->
                    controller?.dispose()
                    controller = null
                    activity.finish()
                }
                Log.i(EVIDENCE_TAG, "test=$testId dispose=completed")
            }
        }
    }

    private fun awaitPlaybackProof(
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
        timeoutSeconds: Long = PLAYBACK_TIMEOUT_SECONDS,
        requireTimelineAdvance: Boolean = true,
    ): PlaybackProof {
        var minimumPositionMs = Long.MAX_VALUE
        var maximumPositionMs = Long.MIN_VALUE
        assertPlayerCondition(
            description = "decoder initialization, first video frame, and timeline progress",
            controller = controller,
            events = events,
            timeoutSeconds = timeoutSeconds,
        ) { state ->
            minimumPositionMs = minOf(minimumPositionMs, state.timeline.positionMs)
            maximumPositionMs = maxOf(maximumPositionMs, state.timeline.positionMs)
            events.hasEvent(VIDEO_DECODER_INITIALIZED_EVENT) &&
                events.hasEvent(FIRST_FRAME_EVENT) &&
                state.playbackState == PlaybackStateUi.Playing &&
                !state.isBuffering &&
                (!requireTimelineAdvance ||
                    maximumPositionMs - minimumPositionMs >= MINIMUM_INITIAL_ADVANCE_MS)
        }
        val state = sample(controller, events)
        assertNull(state.lastError)
        assertTrue(
            "missing $VIDEO_DECODER_INITIALIZED_EVENT: ${describe(state, events)}",
            events.hasEvent(VIDEO_DECODER_INITIALIZED_EVENT),
        )
        assertTrue(
            "missing $FIRST_FRAME_EVENT: ${describe(state, events)}",
            events.hasEvent(FIRST_FRAME_EVENT),
        )
        return PlaybackProof(minimumPositionMs, maximumPositionMs)
    }

    private fun validateVodNetworkControls(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
    ): String {
        val sustainedRange = observeSustainedPlayback(controller, events)
        scenario.onActivity { controller.pause() }
        assertPlayerCondition(
            description = "network VOD to pause",
            controller = controller,
            events = events,
        ) { state -> state.playbackState == PlaybackStateUi.Paused && !state.isBuffering }
        val pausedRange = assertPositionStableWhilePaused(controller, events)
        val pausedState = sample(controller, events)

        var seekPositionMs: Long? = null
        if (pausedState.timeline.isSeekable &&
            (pausedState.timeline.durationMs ?: 0L) >= MINIMUM_NETWORK_SEEK_DURATION_MS
        ) {
            val durationMs = requireNotNull(pausedState.timeline.durationMs)
            val seekTargetMs = (durationMs * NETWORK_SEEK_RATIO).toLong()
            scenario.onActivity { controller.seekToRatio(NETWORK_SEEK_RATIO) }
            assertPlayerCondition(
                description = "network VOD seek to converge on $seekTargetMs ms",
                controller = controller,
                events = events,
            ) { state ->
                state.playbackState == PlaybackStateUi.Paused &&
                    (state.timeline.positionMs - seekTargetMs).absoluteValue <= NETWORK_SEEK_TOLERANCE_MS
            }
            seekPositionMs = sample(controller, events).timeline.positionMs
        }

        val positionBeforeResumeMs = sample(controller, events).timeline.positionMs
        scenario.onActivity { controller.play() }
        assertPlayerCondition(
            description = "network VOD position to advance after resume",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Playing &&
                state.timeline.positionMs >= positionBeforeResumeMs + MINIMUM_POST_SEEK_ADVANCE_MS
        }
        return "vod:sustained=${sustainedRange.first}-${sustainedRange.second}," +
            "paused=${pausedRange.first}-${pausedRange.second}," +
            "pausedSeekableStart=${pausedRange.startSeekableRangeStartMs}-" +
            "${pausedRange.endSeekableRangeStartMs}," +
            "pausedClampObserved=${pausedRange.clampObserved},seek=$seekPositionMs"
    }

    private fun validateLiveDvrNetworkControls(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
    ): String {
        observeActivePlayback(
            controller = controller,
            events = events,
            durationSeconds = NETWORK_LIVE_OBSERVATION_SECONDS,
        )
        scenario.onActivity { controller.pause() }
        assertPlayerCondition(
            description = "Live-DVR playback to pause",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Paused &&
                state.timeline.kind == TimelineKind.LiveDvr &&
                !state.isBuffering
        }
        val pausedRange = assertPositionStableWhilePaused(controller, events)
        val timelineBeforeRewind = sample(controller, events).timeline
        val seekableRange = requireNotNull(timelineBeforeRewind.seekableRange)
        val seekableWidthMs = seekableRange.endMs - seekableRange.startMs
        assertTrue(
            "Live-DVR window is too small to validate rewind: ${describe(sample(controller, events), events)}",
            seekableWidthMs >= MINIMUM_LIVE_DVR_WINDOW_MS,
        )

        scenario.onActivity { controller.seekToRatio(LIVE_DVR_REWIND_RATIO) }
        assertPlayerCondition(
            description = "Live-DVR rewind away from the live edge",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Paused &&
                state.timeline.kind == TimelineKind.LiveDvr &&
                (state.timeline.liveOffsetMs ?: 0L) >= MINIMUM_LIVE_DVR_OFFSET_MS
        }
        val rewindState = sample(controller, events)

        scenario.onActivity { controller.seekToLiveEdge() }
        assertPlayerCondition(
            description = "Live-DVR seek to the live edge",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Paused &&
                state.timeline.kind == TimelineKind.LiveDvr &&
                state.timeline.isAtLiveEdge(LIVE_EDGE_TOLERANCE_MS)
        }
        val liveEdgeState = sample(controller, events)

        scenario.onActivity { controller.play() }
        assertPlayerCondition(
            description = "Live-DVR playback to resume at the live edge",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Playing &&
                state.timeline.kind == TimelineKind.LiveDvr &&
                state.timeline.isAtLiveEdge(LIVE_EDGE_TOLERANCE_MS)
        }
        return "live-dvr:paused=${pausedRange.first}-${pausedRange.second}," +
            "pausedSeekableStart=${pausedRange.startSeekableRangeStartMs}-" +
            "${pausedRange.endSeekableRangeStartMs}," +
            "pausedClampObserved=${pausedRange.clampObserved}," +
            "rewindOffset=${rewindState.timeline.liveOffsetMs}," +
            "liveEdge=${liveEdgeState.timeline.positionMs}"
    }

    private fun validateHostPauseDetachPolicy(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
        controller: VesperPlayerController,
        surfaceHost: SurfaceHostReference,
        events: MutableList<VesperBenchmarkEvent>,
        evidence: VesperAndroidDeviceEvidenceWriter? = null,
        observedSeconds: Long? = null,
        lifecycleCycle: Int? = null,
    ): HostPauseDetachEvidence {
        scenario.onActivity {
            controller.pause()
            controller.detachSurfaceHost(surfaceHost.value)
        }
        evidence?.record(
            phase = "lifecycle_pause_detach",
            state = sample(controller, events),
            events = events,
            details =
                longRunDetails(
                    observedSeconds = observedSeconds,
                    lifecycleCycles = lifecycleCycle,
                    scenarioState = scenario.state.name,
                ),
        )
        stopScenarioWithCoverActivity(scenario)
        assertPlayerCondition(
            description = "host-paused player to remain paused while its Activity is stopped",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Paused
        }
        val stoppedPositionRange = assertPositionStableWhilePaused(controller, events)
        evidence?.record(
            phase = "lifecycle_stopped",
            state = sample(controller, events),
            events = events,
            details =
                longRunDetails(
                    observedSeconds = observedSeconds,
                    lifecycleCycles = lifecycleCycle,
                    scenarioState = scenario.state.name,
                ) +
                    mapOf(
                        "stoppedMinimumPositionMs" to stoppedPositionRange.first,
                        "stoppedMaximumPositionMs" to stoppedPositionRange.second,
                        "stoppedStartSeekableRangeStartMs" to
                            stoppedPositionRange.startSeekableRangeStartMs,
                        "stoppedEndSeekableRangeStartMs" to
                            stoppedPositionRange.endSeekableRangeStartMs,
                        "stoppedSeekableRangeStartAdvanced" to
                            stoppedPositionRange.seekableRangeStartAdvanced,
                        "stoppedClampObserved" to stoppedPositionRange.clampObserved,
                    ),
            captureDetailedMemory = false,
        )

        val resumeDriver = resumeStoppedScenarioByFinishingCoverActivity(scenario)
        scenario.onActivity {
            controller.attachSurfaceHost(surfaceHost.value)
            controller.play()
        }
        evidence?.record(
            phase = "lifecycle_resume_attach",
            state = sample(controller, events),
            events = events,
            details =
                longRunDetails(
                    observedSeconds = observedSeconds,
                    lifecycleCycles = lifecycleCycle,
                    scenarioState = scenario.state.name,
                ) + mapOf("resumeDriver" to resumeDriver),
        )
        val copiedFrame = awaitCopiedSurfaceFrame(scenario, surfaceHost, controller, events)
        val positionAfterCopiedFrameMs = sample(controller, events).timeline.positionMs
        assertPlayerCondition(
            description = "position to advance after the host resumes playback",
            controller = controller,
            events = events,
        ) { state ->
            state.playbackState == PlaybackStateUi.Playing &&
                state.timeline.positionMs >=
                positionAfterCopiedFrameMs + MINIMUM_SURFACE_REATTACH_ADVANCE_MS
        }
        val finalState = sample(controller, events)
        assertNull(finalState.lastError)
        evidence?.record(
            phase = "lifecycle_recovered",
            state = finalState,
            events = events,
            details =
                longRunDetails(
                    observedSeconds = observedSeconds,
                    lifecycleCycles = lifecycleCycle,
                    scenarioState = scenario.state.name,
                ) +
                    mapOf(
                        "pixelCopyResult" to copiedFrame.result,
                        "pixelCopyWidth" to copiedFrame.width,
                        "pixelCopyHeight" to copiedFrame.height,
                        "pixelCopyCenterPixel" to copiedFrame.centerPixel,
                        "pixelCopySampleHash" to copiedFrame.sampleHash,
                        "pixelCopyNonBlackSamples" to copiedFrame.nonBlackSampleCount,
                        "positionAfterCopiedFrameMs" to positionAfterCopiedFrameMs,
                    ),
        )
        return HostPauseDetachEvidence(
            stoppedPositionRange = stoppedPositionRange,
            copiedFrame = copiedFrame,
            positionAfterCopiedFrameMs = positionAfterCopiedFrameMs,
            finalState = finalState,
        )
    }

    private fun resumeStoppedScenarioByFinishingCoverActivity(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
    ): String {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var coverActivityClassName = ""
        instrumentation.runOnMainSync {
            val resumedActivities =
                ActivityLifecycleMonitorRegistry
                    .getInstance()
                    .getActivitiesInStage(Stage.RESUMED)
            check(resumedActivities.size == 1) {
                "expected exactly one ActivityScenario cover Activity while the player host was " +
                    "stopped, found=${resumedActivities.map { it.javaClass.name }}"
            }
            val coverActivity = resumedActivities.single()
            check(coverActivity is VesperLifecycleCoverTestActivity) {
                "expected Vesper lifecycle cover Activity, found=${coverActivity.javaClass.name}"
            }
            coverActivityClassName = coverActivity.javaClass.name
            coverActivity.finish()
        }

        val resumed =
            awaitCondition(PLAYBACK_TIMEOUT_SECONDS) {
                runCatching { scenario.state }.getOrNull() == Lifecycle.State.RESUMED
            }
        assertTrue(
            "player host did not resume after finishing cover Activity $coverActivityClassName; " +
                "scenarioState=${runCatching { scenario.state.name }.getOrNull()}",
            resumed,
        )
        return "finish-cover:$coverActivityClassName"
    }

    private fun stopScenarioWithCoverActivity(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
    ) {
        // ActivityScenario waits on app-local broadcasts that some MTK DuraSpeed builds suppress.
        scenario.onActivity { activity ->
            activity.startActivity(Intent(activity, VesperLifecycleCoverTestActivity::class.java))
        }
        val stopped =
            awaitCondition(PLAYBACK_TIMEOUT_SECONDS) {
                runCatching { scenario.state }.getOrNull() == Lifecycle.State.CREATED
            }
        assertTrue(
            "player host did not stop behind Vesper lifecycle cover Activity; " +
                "scenarioState=${runCatching { scenario.state.name }.getOrNull()}",
            stopped,
        )
    }

    private fun longRunDetails(
        observedSeconds: Long?,
        lifecycleCycles: Int?,
        scenarioState: String,
    ): Map<String, Any?> =
        mapOf(
            "observedSeconds" to observedSeconds,
            "lifecycleCycles" to lifecycleCycles,
            "scenarioState" to scenarioState,
        )

    private fun observeActivePlayback(
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
        durationSeconds: Long,
    ) {
        val startedAtNanos = System.nanoTime()
        val deadlineNanos = startedAtNanos + TimeUnit.SECONDS.toNanos(durationSeconds)
        var state = sample(controller, events)
        while (System.nanoTime() < deadlineNanos && state.lastError == null) {
            Thread.sleep(POLL_INTERVAL_MS)
            state = sample(controller, events)
        }
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAtNanos)
        assertTrue(
            "network playback did not remain active for ${durationSeconds}s: " +
                "elapsedMs=$elapsedMs, ${describe(state, events)}",
            elapsedMs >= TimeUnit.SECONDS.toMillis(durationSeconds) &&
                state.lastError == null &&
                state.playbackState == PlaybackStateUi.Playing,
        )
        assertNull(state.lastError)
    }

    private fun observeSustainedPlayback(
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
    ): Pair<Long, Long> {
        val startedAtNanos = System.nanoTime()
        val deadlineNanos = startedAtNanos + TimeUnit.SECONDS.toNanos(SUSTAINED_PLAYBACK_SECONDS)
        val startPositionMs = sample(controller, events).timeline.positionMs
        var state = controller.uiState.value
        var remainedActive = true
        while (System.nanoTime() < deadlineNanos) {
            state = sample(controller, events)
            if (state.lastError != null || state.playbackState == PlaybackStateUi.Finished) {
                remainedActive = false
                break
            }
            Thread.sleep(POLL_INTERVAL_MS)
        }
        state = sample(controller, events)
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAtNanos)
        val endPositionMs = state.timeline.positionMs
        assertTrue(
            "sustained playback ended early: elapsedMs=$elapsedMs, startPositionMs=$startPositionMs, " +
                "endPositionMs=$endPositionMs, ${describe(state, events)}",
            remainedActive &&
                elapsedMs >= TimeUnit.SECONDS.toMillis(SUSTAINED_PLAYBACK_SECONDS) &&
                state.lastError == null &&
                state.playbackState == PlaybackStateUi.Playing &&
                endPositionMs >= startPositionMs + MINIMUM_SUSTAINED_ADVANCE_MS,
        )
        assertNull(state.lastError)
        return startPositionMs to endPositionMs
    }

    private fun awaitCopiedSurfaceFrame(
        scenario: ActivityScenario<VesperSurfaceLayoutTestActivity>,
        surfaceHost: SurfaceHostReference,
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
    ): CopiedSurfaceFrame {
        var latest = CopiedSurfaceFrame(PixelCopy.ERROR_SOURCE_INVALID, 0, 0, 0)
        val copied = awaitCondition(SURFACE_TIMEOUT_SECONDS) {
            var surfaceView: SurfaceView? = null
            scenario.onActivity {
                surfaceView = surfaceHost.value.findSurfaceView()?.takeIf { surface ->
                    surface.width > 0 && surface.height > 0 && surface.holder.surface.isValid
                }
            }
            val activeSurface = surfaceView ?: return@awaitCondition false
            latest = copySurfaceFrame(activeSurface)
            sample(controller, events).lastError == null && latest.result == PixelCopy.SUCCESS
        }
        assertTrue(
            "PixelCopy did not receive a frame from the recreated SurfaceView: " +
                "result=${latest.result}, size=${latest.width}x${latest.height}, " +
                describe(sample(controller, events), events),
            copied,
        )
        return latest
    }

    private fun copySurfaceFrame(surfaceView: SurfaceView): CopiedSurfaceFrame {
        val width = surfaceView.width
        val height = surfaceView.height
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val completion = CountDownLatch(1)
        var result = PixelCopy.ERROR_TIMEOUT
        return try {
            PixelCopy.request(
                surfaceView,
                bitmap,
                { copyResult ->
                    result = copyResult
                    completion.countDown()
                },
                Handler(Looper.getMainLooper()),
            )
            if (!completion.await(PIXEL_COPY_ATTEMPT_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                return CopiedSurfaceFrame(PixelCopy.ERROR_TIMEOUT, width, height, 0)
            }
            val bitmapSample = sampleBitmap(bitmap)
            CopiedSurfaceFrame(
                result = result,
                width = width,
                height = height,
                centerPixel = if (result == PixelCopy.SUCCESS) bitmap.getPixel(width / 2, height / 2) else 0,
                sampleHash = if (result == PixelCopy.SUCCESS) bitmapSample.hash else 0L,
                nonBlackSampleCount =
                    if (result == PixelCopy.SUCCESS) bitmapSample.nonBlackSampleCount else 0,
            )
        } catch (_: IllegalArgumentException) {
            CopiedSurfaceFrame(PixelCopy.ERROR_SOURCE_INVALID, width, height, 0)
        } finally {
            if (completion.count == 0L) {
                bitmap.recycle()
            }
        }
    }

    private fun sampleBitmap(bitmap: Bitmap): BitmapSample {
        var hash = PIXEL_HASH_OFFSET_BASIS
        var nonBlackSampleCount = 0
        val xStep = maxOf(1, bitmap.width / PIXEL_HASH_AXIS_SAMPLES)
        val yStep = maxOf(1, bitmap.height / PIXEL_HASH_AXIS_SAMPLES)
        var y = 0
        while (y < bitmap.height) {
            var x = 0
            while (x < bitmap.width) {
                val pixel = bitmap.getPixel(x, y)
                hash = (hash xor (pixel.toLong() and 0xffff_ffffL)) * PIXEL_HASH_PRIME
                if (pixel and 0x00ff_ffff != 0) {
                    nonBlackSampleCount += 1
                }
                x += xStep
            }
            y += yStep
        }
        return BitmapSample(hash, nonBlackSampleCount)
    }

    private fun assertPositionStableWhilePaused(
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
    ): PausedPositionEvidence {
        val deadlineNanos = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(PAUSE_OBSERVATION_MS)
        val initialState = sample(controller, events)
        val initialPositionMs = initialState.timeline.positionMs
        val liveDvr = initialState.timeline.kind == TimelineKind.LiveDvr
        val startSeekableRangeStartMs = initialState.timeline.seekableRange?.startMs
        var minimumPositionMs = initialPositionMs
        var maximumPositionMs = initialPositionMs
        var previousPositionMs = initialPositionMs
        var previousSeekableRangeStartMs = startSeekableRangeStartMs
        var endSeekableRangeStartMs = startSeekableRangeStartMs
        var endExpectedPositionMs = initialPositionMs
        var seekableRangeStartAdvanced = false
        var seekableRangeStartMonotonic = true
        var positionMatchedExpectedClamp = true
        var positionDidNotRegress = true
        var clampObserved = false
        var timelineRemainedLiveDvr = !liveDvr || startSeekableRangeStartMs != null
        var state = initialState
        var remainedPaused =
            initialState.lastError == null &&
                initialState.playbackState == PlaybackStateUi.Paused
        while (System.nanoTime() < deadlineNanos) {
            state = sample(controller, events)
            val positionMs = state.timeline.positionMs
            minimumPositionMs = minOf(minimumPositionMs, positionMs)
            maximumPositionMs = maxOf(maximumPositionMs, positionMs)
            if (state.lastError != null || state.playbackState != PlaybackStateUi.Paused) {
                remainedPaused = false
                break
            }
            if (liveDvr) {
                val currentSeekableRangeStartMs = state.timeline.seekableRange?.startMs
                if (state.timeline.kind != TimelineKind.LiveDvr || currentSeekableRangeStartMs == null) {
                    timelineRemainedLiveDvr = false
                } else {
                    val previousStartMs = previousSeekableRangeStartMs
                    if (previousStartMs != null && currentSeekableRangeStartMs < previousStartMs) {
                        seekableRangeStartMonotonic = false
                    }
                    if (startSeekableRangeStartMs != null &&
                        currentSeekableRangeStartMs > startSeekableRangeStartMs
                    ) {
                        seekableRangeStartAdvanced = true
                    }
                    val expectedPositionMs = maxOf(initialPositionMs, currentSeekableRangeStartMs)
                    endExpectedPositionMs = expectedPositionMs
                    if ((positionMs - expectedPositionMs).absoluteValue >
                        PAUSED_POSITION_TOLERANCE_MS
                    ) {
                        positionMatchedExpectedClamp = false
                    }
                    if (positionMs < previousPositionMs - PAUSED_POSITION_TOLERANCE_MS) {
                        positionDidNotRegress = false
                    }
                    if (currentSeekableRangeStartMs > initialPositionMs &&
                        (positionMs - currentSeekableRangeStartMs).absoluteValue <=
                            PAUSED_POSITION_TOLERANCE_MS
                    ) {
                        clampObserved = true
                    }
                    previousSeekableRangeStartMs = currentSeekableRangeStartMs
                    endSeekableRangeStartMs = currentSeekableRangeStartMs
                }
            }
            previousPositionMs = positionMs
            Thread.sleep(POLL_INTERVAL_MS)
        }
        val spreadMs = maximumPositionMs - minimumPositionMs
        val stable =
            if (liveDvr) {
                timelineRemainedLiveDvr &&
                    seekableRangeStartMonotonic &&
                    positionMatchedExpectedClamp &&
                    positionDidNotRegress &&
                    (seekableRangeStartAdvanced || spreadMs <= PAUSED_POSITION_TOLERANCE_MS)
            } else {
                spreadMs <= PAUSED_POSITION_TOLERANCE_MS
            }
        assertTrue(
            "paused position was not stable: initial=$initialPositionMs, min=$minimumPositionMs, " +
                "max=$maximumPositionMs, spread=$spreadMs, " +
                "startSeekableRangeStartMs=$startSeekableRangeStartMs, " +
                "endSeekableRangeStartMs=$endSeekableRangeStartMs, " +
                "endExpectedPositionMs=$endExpectedPositionMs, " +
                "seekableRangeStartAdvanced=$seekableRangeStartAdvanced, " +
                "seekableRangeStartMonotonic=$seekableRangeStartMonotonic, " +
                "positionMatchedExpectedClamp=$positionMatchedExpectedClamp, " +
                "positionDidNotRegress=$positionDidNotRegress, clampObserved=$clampObserved, " +
                "remainedPaused=$remainedPaused, timelineRemainedLiveDvr=$timelineRemainedLiveDvr, " +
                "${describe(state, events)}",
            remainedPaused && stable,
        )
        assertNull(state.lastError)
        return PausedPositionEvidence(
            minimumPositionMs = minimumPositionMs,
            maximumPositionMs = maximumPositionMs,
            startSeekableRangeStartMs = startSeekableRangeStartMs,
            endSeekableRangeStartMs = endSeekableRangeStartMs,
            seekableRangeStartAdvanced = seekableRangeStartAdvanced,
            clampObserved = clampObserved,
        )
    }

    private fun assertPlayerCondition(
        description: String,
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
        timeoutSeconds: Long = PLAYBACK_TIMEOUT_SECONDS,
        predicate: (PlayerHostUiState) -> Boolean,
    ) {
        var state = sample(controller, events)
        val reached = awaitCondition(timeoutSeconds) {
            state = sample(controller, events)
            if (state.lastError != null) {
                true
            } else {
                predicate(state)
            }
        }
        assertTrue(
            "condition failed while waiting for $description: ${describe(state, events)}",
            reached && state.lastError == null && predicate(state),
        )
        assertNull(state.lastError)
    }

    private fun sample(
        controller: VesperPlayerController,
        events: MutableList<VesperBenchmarkEvent>,
    ): PlayerHostUiState {
        controller.refresh()
        events += controller.drainBenchmarkEvents()
        val excessEvents = events.size - MAX_RETAINED_TEST_BENCHMARK_EVENTS
        if (excessEvents > 0) {
            events.subList(0, excessEvents).clear()
        }
        return controller.uiState.value
    }

    private fun describe(
        state: PlayerHostUiState,
        events: List<VesperBenchmarkEvent>,
    ): String =
        "state=${state.playbackState}, buffering=${state.isBuffering}, " +
            "timeline=${state.timeline.kind}, positionMs=${state.timeline.positionMs}, " +
            "durationMs=${state.timeline.durationMs}, seekable=${state.timeline.isSeekable}, " +
            "seekableRange=${state.timeline.seekableRange}, liveEdgeMs=${state.timeline.liveEdgeMs}, " +
            "liveOffsetMs=${state.timeline.liveOffsetMs}, error=${state.lastError}, " +
            "events=${events.map(VesperBenchmarkEvent::eventName)}"

    private fun awaitCondition(
        timeoutSeconds: Long,
        predicate: () -> Boolean,
    ): Boolean {
        val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(timeoutSeconds)
        while (System.nanoTime() < deadlineNanos) {
            if (predicate()) return true
            Thread.sleep(POLL_INTERVAL_MS)
        }
        return predicate()
    }

    private fun resolveNetworkFixture(): NetworkFixture {
        val arguments = InstrumentationRegistry.getArguments()
        val networkUrl = arguments.getString(NETWORK_URL_ARGUMENT)
        assumeTrue(
            "requires -e $NETWORK_URL_ARGUMENT <http-or-https-url>",
            !networkUrl.isNullOrBlank(),
        )
        val resolvedUrl = requireNotNull(networkUrl)
        val protocol = networkProtocol(arguments.getString(NETWORK_PROTOCOL_ARGUMENT), resolvedUrl)
        return NetworkFixture(
            source =
                VesperPlayerSource.remote(
                    uri = resolvedUrl,
                    label = "network device fixture",
                    protocol = protocol,
                ),
            protocol = protocol,
            expectedTimelineKind =
                expectedTimelineKind(arguments.getString(NETWORK_TIMELINE_ARGUMENT)),
        )
    }

    private fun triggerControlledOutage(
        controlUrl: String,
        outageSeconds: Long,
    ): String {
        val separator = if ('?' in controlUrl) '&' else '?'
        val connection =
            URL("$controlUrl${separator}seconds=$outageSeconds").openConnection() as HttpURLConnection
        return try {
            connection.requestMethod = "POST"
            connection.connectTimeout = NETWORK_CONTROL_TIMEOUT_MS
            connection.readTimeout = NETWORK_CONTROL_TIMEOUT_MS
            val statusCode = connection.responseCode
            assertTrue("outage control returned HTTP $statusCode", statusCode in 200..299)
            connection.inputStream.bufferedReader().use { it.readText() }
        } finally {
            connection.disconnect()
        }
    }

    private fun networkProtocol(
        argument: String?,
        url: String,
    ): VesperPlayerSourceProtocol =
        when (argument?.trim()?.lowercase()) {
            null, "" -> VesperPlayerSource.remote(url, "network protocol inference").protocol
            "progressive" -> VesperPlayerSourceProtocol.Progressive
            "hls" -> VesperPlayerSourceProtocol.Hls
            "dash" -> VesperPlayerSourceProtocol.Dash
            else -> error(
                "$NETWORK_PROTOCOL_ARGUMENT must be progressive, hls, or dash: $argument",
            )
        }

    private fun expectedTimelineKind(argument: String?): TimelineKind? =
        when (argument?.trim()?.lowercase()) {
            null, "" -> null
            "vod" -> TimelineKind.Vod
            "live" -> TimelineKind.Live
            "live-dvr", "live_dvr", "livedvr" -> TimelineKind.LiveDvr
            else -> error(
                "$NETWORK_TIMELINE_ARGUMENT must be vod, live, or live-dvr: $argument",
            )
        }

    private fun resolveFixture(
        context: Context,
        testId: String,
    ): Fixture {
        val argument = InstrumentationRegistry.getArguments().getString(FIXTURE_FILE_ARGUMENT)
        if (!argument.isNullOrBlank()) {
            val file = File(argument)
            require(file.isAbsolute && file.isFile && file.canRead()) {
                "$FIXTURE_FILE_ARGUMENT must name an absolute readable file: $argument"
            }
            return Fixture(file = file, deleteAfterTest = false)
        }

        val destination = File(context.cacheDir, "vesper-$testId-${System.nanoTime()}.m4v")
        context.assets.open(DEFAULT_FIXTURE_ASSET).use { input ->
            destination.outputStream().use(input::copyTo)
        }
        return Fixture(file = destination, deleteAfterTest = true)
    }

    private fun fixtureDurationMs(file: File): Long {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(file.absolutePath)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: -1L
        } finally {
            retriever.release()
        }
    }

    private fun List<VesperBenchmarkEvent>.hasEvent(eventName: String): Boolean =
        any { it.eventName == eventName }

    private fun decoderNames(events: List<VesperBenchmarkEvent>): List<String> =
        events
            .asSequence()
            .filter { it.eventName == VIDEO_DECODER_INITIALIZED_EVENT }
            .mapNotNull { it.attributes["decoderName"] }
            .distinct()
            .toList()

    private fun droppedVideoFrameEvidence(events: List<VesperBenchmarkEvent>): List<Map<String, String>> =
        events
            .asSequence()
            .filter { it.eventName == DROPPED_VIDEO_FRAMES_EVENT }
            .map(VesperBenchmarkEvent::attributes)
            .toList()

    private fun ViewGroup.findSurfaceView(): SurfaceView? {
        repeat(childCount) { index ->
            when (val child = getChildAt(index)) {
                is SurfaceView -> return child
                is ViewGroup -> child.findSurfaceView()?.let { return it }
            }
        }
        return null
    }

    private data class Fixture(
        val file: File,
        val deleteAfterTest: Boolean,
    )

    private data class NetworkFixture(
        val source: VesperPlayerSource,
        val protocol: VesperPlayerSourceProtocol,
        val expectedTimelineKind: TimelineKind?,
    )

    private data class SurfaceHostReference(
        var value: FrameLayout,
    )

    private data class PlaybackProof(
        val minimumPositionMs: Long,
        val maximumPositionMs: Long,
    )

    private data class CopiedSurfaceFrame(
        val result: Int,
        val width: Int,
        val height: Int,
        val centerPixel: Int,
        val sampleHash: Long = 0L,
        val nonBlackSampleCount: Int = 0,
    )

    private data class BitmapSample(
        val hash: Long,
        val nonBlackSampleCount: Int,
    )

    private data class PausedPositionEvidence(
        val minimumPositionMs: Long,
        val maximumPositionMs: Long,
        val startSeekableRangeStartMs: Long?,
        val endSeekableRangeStartMs: Long?,
        val seekableRangeStartAdvanced: Boolean,
        val clampObserved: Boolean,
    ) {
        val first: Long
            get() = minimumPositionMs

        val second: Long
            get() = maximumPositionMs
    }

    private data class HostPauseDetachEvidence(
        val stoppedPositionRange: PausedPositionEvidence,
        val copiedFrame: CopiedSurfaceFrame,
        val positionAfterCopiedFrameMs: Long,
        val finalState: PlayerHostUiState,
    )

    private companion object {
        const val EVIDENCE_TAG = "VesperDevicePlayback"
        const val FIXTURE_FILE_ARGUMENT = "vesperPlaybackFixtureFile"
        const val NETWORK_URL_ARGUMENT = "vesperPlaybackNetworkUrl"
        const val NETWORK_PROTOCOL_ARGUMENT = "vesperPlaybackNetworkProtocol"
        const val NETWORK_TIMELINE_ARGUMENT = "vesperPlaybackExpectedTimeline"
        const val NETWORK_OUTAGE_CONTROL_URL_ARGUMENT = "vesperPlaybackOutageControlUrl"
        const val NETWORK_OUTAGE_SECONDS_ARGUMENT = "vesperPlaybackOutageSeconds"
        const val NETWORK_LONG_RUN_SECONDS_ARGUMENT = "vesperPlaybackLongRunSeconds"
        const val NETWORK_LONG_RUN_EVIDENCE_ID = "android-network-longrun"
        const val DEFAULT_FIXTURE_ASSET = "tiny-h264-aac-mediacodec.m4v"
        const val VIDEO_DECODER_INITIALIZED_EVENT = "video_decoder_initialized"
        const val FIRST_FRAME_EVENT = "first_frame_rendered"
        const val DROPPED_VIDEO_FRAMES_EVENT = "dropped_video_frames"
        const val LOAD_ERROR_EVENT = "load_error"
        const val RETRY_SCHEDULED_EVENT = "retry_scheduled"
        const val MAX_BUFFERED_BENCHMARK_EVENTS = 512
        const val MAX_RETAINED_TEST_BENCHMARK_EVENTS = 2_048
        const val PLAYBACK_TIMEOUT_SECONDS = 15L
        const val NETWORK_PLAYBACK_TIMEOUT_SECONDS = 30L
        const val NETWORK_LIVE_OBSERVATION_SECONDS = 20L
        const val NETWORK_OUTAGE_SIGNAL_TIMEOUT_SECONDS = 10L
        const val NETWORK_RECOVERY_TIMEOUT_SECONDS = 15L
        const val NETWORK_POST_RECOVERY_OBSERVATION_SECONDS = 5L
        const val NETWORK_LONG_RUN_CHUNK_SECONDS = 60L
        const val MINIMUM_NETWORK_LONG_RUN_SECONDS = 60L
        const val MAXIMUM_NETWORK_LONG_RUN_SECONDS = 2_400L
        const val DEFAULT_NETWORK_OUTAGE_SECONDS = 4L
        const val MAX_NETWORK_OUTAGE_SECONDS = 8L
        const val NETWORK_RECOVERY_SETTLE_DELAY_MS = 1_000L
        const val NETWORK_CONTROL_TIMEOUT_MS = 5_000
        const val SURFACE_TIMEOUT_SECONDS = 5L
        const val POLL_INTERVAL_MS = 50L
        const val PIXEL_COPY_ATTEMPT_TIMEOUT_MS = 1_000L
        const val POST_DISPOSE_GC_SETTLE_MS = 1_500L
        const val PIXEL_HASH_AXIS_SAMPLES = 16
        const val PIXEL_HASH_OFFSET_BASIS = 1_469_598_103_934_665_603L
        const val PIXEL_HASH_PRIME = 1_099_511_628_211L
        const val MINIMUM_LONG_FIXTURE_DURATION_MS = 8_000L
        const val MINIMUM_SUSTAINED_FIXTURE_DURATION_MS = 12_000L
        const val MINIMUM_INITIAL_ADVANCE_MS = 250L
        const val SUSTAINED_PLAYBACK_SECONDS = 8L
        const val MINIMUM_SUSTAINED_ADVANCE_MS = 6_000L
        const val PAUSE_OBSERVATION_MS = 750L
        const val PAUSED_POSITION_TOLERANCE_MS = 150L
        const val MINIMUM_RESUME_ADVANCE_MS = 400L
        const val SEEK_PERCENT = 70L
        const val SEEK_RATIO = 0.70f
        const val SEEK_TOLERANCE_MS = 350L
        const val MINIMUM_SEEK_DISTANCE_MS = 500L
        const val MINIMUM_POST_SEEK_ADVANCE_MS = 400L
        const val MINIMUM_NETWORK_RECOVERY_ADVANCE_MS = 400L
        const val MINIMUM_SURFACE_REATTACH_ADVANCE_MS = 400L
        const val MINIMUM_NETWORK_SEEK_DURATION_MS = 8_000L
        const val NETWORK_SEEK_RATIO = 0.30f
        const val NETWORK_SEEK_TOLERANCE_MS = 750L
        const val LIVE_DVR_REWIND_RATIO = 0.20f
        const val MINIMUM_LIVE_DVR_WINDOW_MS = 20_000L
        const val MINIMUM_LIVE_DVR_OFFSET_MS = 10_000L
        const val LIVE_EDGE_TOLERANCE_MS = 3_500L
    }
}
