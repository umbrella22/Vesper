package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.view.Window
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

class VesperPerformanceDiagnosticsTest {
    @Test
    fun invalidConfigurationUsesStableErrorCodeBeforeAllocatingRecorder() {
        var recorderCreated = false
        val coordinator = coordinator(
            runtime = RecordingRuntime(),
            onRecorderCreated = { recorderCreated = true },
        )

        val error = assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.InvalidConfiguration) {
            coordinator.startPerformance(
                VesperPerformanceDiagnosticsConfiguration(maxRawEvents = 2_049),
                VesperPerformanceProbe.FlutterFrameTiming,
                null,
            )
        }

        assertFalse(recorderCreated)
        assertTrue(error.message.orEmpty().contains("2048"))
    }

    @Test
    fun unsupportedProbeDoesNotAllocateRecorderOrLeaveAnActiveRun() {
        var recorderCreated = false
        val coordinator = coordinator(
            runtime = RecordingRuntime(),
            onRecorderCreated = { recorderCreated = true },
        )

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ProbeUnavailable) {
            coordinator.startPerformance(
                VesperPerformanceDiagnosticsConfiguration(),
                VesperPerformanceProbe("futureProbe"),
                null,
            )
        }

        assertFalse(recorderCreated)
        assertFalse(coordinator.isEnabled)
    }

    @Test
    fun sinkOpenFailureReturnsArtifactUnavailableBeforeRegisteringFrameProbe() {
        val runtime = RecordingRuntime(failOpen = true)
        lateinit var recorder: VesperBenchmarkRecorder
        var frameProbeCreated = false
        val coordinator = VesperBenchmarkCoordinator(
            performanceRecorderFactory = { configuration, _ ->
                VesperBenchmarkRecorder(
                    configuration = VesperBenchmarkConfiguration(
                        enabled = true,
                        maxBufferedEvents = configuration.maxRawEvents,
                        includeRawEvents = configuration.includeRawEvents,
                        pluginReferences = listOf(VesperBundledPluginReferences.performanceDiagnostics),
                    ),
                    sinkRuntime = runtime,
                ).also { recorder = it }
            },
            frameProbeFactory = { _, _, _ ->
                frameProbeCreated = true
                AutoCloseable {}
            },
        )

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable) {
            coordinator.startPerformance(
                VesperPerformanceDiagnosticsConfiguration(),
                VesperPerformanceProbe.AndroidFrameMetrics,
                null,
            )
        }

        assertFalse(frameProbeCreated)
        assertFalse(coordinator.isEnabled)
        assertEquals(VesperBenchmarkSinkReadiness.OpenFailed, recorder.awaitSinkReadiness(0))
        assertTrue(recorder.awaitSinkShutdown(0))
        assertEquals(1, runtime.openCount)
        assertEquals(0, runtime.disposeCount)
        assertEquals(0, runtime.closeCount)
    }

    @Test
    fun sinkReadinessTimeoutReturnsInternalFailureAndCleansUpBeforeReturning() {
        val runtime = RecordingRuntime(blockOpen = true)
        lateinit var recorder: VesperBenchmarkRecorder
        var frameProbeCreated = false
        val coordinator = VesperBenchmarkCoordinator(
            performanceRecorderFactory = { configuration, _ ->
                VesperBenchmarkRecorder(
                    configuration = VesperBenchmarkConfiguration(
                        enabled = true,
                        maxBufferedEvents = configuration.maxRawEvents,
                        includeRawEvents = configuration.includeRawEvents,
                        pluginReferences = listOf(VesperBundledPluginReferences.performanceDiagnostics),
                    ),
                    sinkRuntime = runtime,
                ).also { recorder = it }
            },
            frameProbeFactory = { _, _, _ ->
                frameProbeCreated = true
                AutoCloseable {}
            },
            performanceStartTimeoutMs = 10,
        )
        val releaseOpen = Thread {
            runtime.openStarted.await(2, TimeUnit.SECONDS)
            Thread.sleep(30)
            runtime.allowOpen.countDown()
        }.apply { start() }

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.InternalFailure) {
            coordinator.startPerformance(
                VesperPerformanceDiagnosticsConfiguration(),
                VesperPerformanceProbe.AndroidFrameMetrics,
                null,
            )
        }
        releaseOpen.join(2_000)

        assertFalse(frameProbeCreated)
        assertFalse(coordinator.isEnabled)
        assertTrue(recorder.awaitSinkShutdown(0))
        assertEquals(1, runtime.disposeCount)
        assertEquals(1, runtime.closeCount)
    }

    @Test
    fun concurrentStartCreatesAtMostOneActiveRun() {
        val runtime = RecordingRuntime()
        val coordinator = coordinator(runtime)
        val startGate = CountDownLatch(1)
        val results = Collections.synchronizedList(mutableListOf<Result<String>>())
        val threads = List(2) {
            Thread {
                startGate.await(2, TimeUnit.SECONDS)
                results += runCatching {
                    coordinator.startPerformance(
                        VesperPerformanceDiagnosticsConfiguration(),
                        VesperPerformanceProbe.FlutterFrameTiming,
                        null,
                    )
                }
            }.apply { start() }
        }

        startGate.countDown()
        threads.forEach { it.join(2_000) }

        assertEquals(2, results.size)
        assertEquals(1, results.count(Result<String>::isSuccess))
        val failure = results.single(Result<String>::isFailure).exceptionOrNull()
        assertTrue(failure is VesperPerformanceDiagnosticsException)
        assertEquals(
            VesperPerformanceDiagnosticsErrorCode.AlreadyActive,
            (failure as VesperPerformanceDiagnosticsException).code,
        )
        coordinator.stop(results.single(Result<String>::isSuccess).getOrThrow())
    }

    @Test
    fun disposeDuringStartReturnsControllerDisposedAndFinalizesOnce() {
        val runtime = RecordingRuntime(blockOpen = true)
        lateinit var recorder: VesperBenchmarkRecorder
        var frameProbeCreated = false
        val coordinator = VesperBenchmarkCoordinator(
            performanceRecorderFactory = { configuration, _ ->
                VesperBenchmarkRecorder(
                    configuration = VesperBenchmarkConfiguration(
                        enabled = true,
                        maxBufferedEvents = configuration.maxRawEvents,
                        includeRawEvents = configuration.includeRawEvents,
                        pluginReferences = listOf(VesperBundledPluginReferences.performanceDiagnostics),
                    ),
                    sinkRuntime = runtime,
                ).also { recorder = it }
            },
            frameProbeFactory = { _, _, _ ->
                frameProbeCreated = true
                AutoCloseable {}
            },
        )
        val failure = AtomicReference<Throwable?>()
        val startThread = Thread {
            failure.set(
                runCatching {
                    coordinator.startPerformance(
                        VesperPerformanceDiagnosticsConfiguration(),
                        VesperPerformanceProbe.AndroidFrameMetrics,
                        null,
                    )
                }.exceptionOrNull(),
            )
        }.apply { start() }

        assertTrue(runtime.openStarted.await(2, TimeUnit.SECONDS))
        coordinator.dispose()
        runtime.allowOpen.countDown()
        startThread.join(2_000)

        val error = failure.get() as VesperPerformanceDiagnosticsException
        assertEquals(VesperPerformanceDiagnosticsErrorCode.ControllerDisposed, error.code)
        assertFalse(frameProbeCreated)
        assertTrue(recorder.awaitSinkShutdown(2_000))
        assertEquals(1, runtime.disposeCount)
        assertEquals(1, runtime.closeCount)
    }

    @Test
    fun startAfterDisposeReturnsControllerDisposedWithoutAllocatingRecorder() {
        var recorderCreated = false
        val coordinator = coordinator(
            runtime = RecordingRuntime(),
            onRecorderCreated = { recorderCreated = true },
        )
        coordinator.dispose()

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ControllerDisposed) {
            coordinator.startPerformance(
                VesperPerformanceDiagnosticsConfiguration(),
                VesperPerformanceProbe.FlutterFrameTiming,
                null,
            )
        }

        assertFalse(recorderCreated)
    }

    @Test
    fun frameBatchLimitAndMalformedSamplesUseProtocolViolation() {
        val coordinator = coordinator(RecordingRuntime())
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        val sample = VesperPerformanceFrameSample(
            loadNs = 1,
            budgetNs = 2,
            overlayState = VesperPerformanceOverlayState(active = false),
        )

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ProtocolViolation) {
            coordinator.recordPerformanceFrames(runId, List(121) { sample })
        }
        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ProtocolViolation) {
            coordinator.recordPerformanceFrames(runId, listOf(sample.copy(budgetNs = 0)))
        }
        coordinator.stop(runId)
    }

    @Test
    fun frameSamplesKeepTheOverlayStateCapturedByTheCaller() {
        val runtime = RecordingRuntime()
        val coordinator = coordinator(runtime)
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )

        coordinator.recordPerformanceFrames(
            runId,
            listOf(
                VesperPerformanceFrameSample(
                    loadNs = 10,
                    budgetNs = 20,
                    overlayState = VesperPerformanceOverlayState(active = false),
                ),
                VesperPerformanceFrameSample(
                    loadNs = 30,
                    budgetNs = 20,
                    overlayState = VesperPerformanceOverlayState(
                        active = true,
                        loadedBasicItemCount = 42,
                    ),
                ),
            ),
        )
        coordinator.snapshot(runId)

        val frameEvents = runtime.events().filter {
            it.getString("eventName") == "performance_frame_sample"
        }
        assertEquals(2, frameEvents.size)
        assertEquals("false", frameEvents[0].getJSONObject("attributes").getString("overlayActive"))
        assertEquals("true", frameEvents[1].getJSONObject("attributes").getString("overlayActive"))
        assertEquals("42", frameEvents[1].getJSONObject("attributes").getString("loadedBasicItemCount"))
        coordinator.stop(runId)
    }

    @Test
    fun staleFrameProbeCallbacksCannotWriteIntoANewerRun() {
        val runtime = RecordingRuntime()
        val callbacks = mutableListOf<(Long, Long) -> Unit>()
        val coordinator = coordinator(
            runtime = runtime,
            frameProbeFactory = { _, _, callback ->
                callbacks += callback
                AutoCloseable {}
            },
        )
        val firstRunId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.AndroidFrameMetrics,
            null,
        )
        coordinator.stop(firstRunId)
        val secondRunId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.AndroidFrameMetrics,
            null,
        )

        callbacks.first()(30, 20)
        callbacks.last()(40, 20)
        coordinator.snapshot(secondRunId)

        val frames = runtime.events().filter {
            it.getString("eventName") == "performance_frame_sample"
        }
        assertEquals(1, frames.size)
        assertEquals(secondRunId, frames.single().getString("runId"))
        coordinator.stop(secondRunId)
    }

    @Test
    fun rawEventsDisabledRetainsNoEventPayloads() {
        val coordinator = coordinator(RecordingRuntime())
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(includeRawEvents = false),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )

        coordinator.record("first_frame_rendered", null, emptyMap())

        assertTrue(coordinator.snapshotEvents().isEmpty())
        coordinator.stop(runId)
    }

    @Test
    fun markerCountIsBoundedPerRun() {
        val coordinator = coordinator(RecordingRuntime())
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        repeat(64) { index ->
            coordinator.recordMarker(runId, "marker_$index", null, index, null)
        }

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ProtocolViolation) {
            coordinator.recordMarker(runId, "marker_overflow", null, null, null)
        }
        coordinator.stop(runId)
    }

    @Test
    fun bufferingTransitionsAreStandardizedWithoutForwardingHostDetails() {
        val runtime = RecordingRuntime()
        val coordinator = coordinator(runtime)
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
            initialPlaybackActive = true,
        )

        coordinator.record("buffering_changed", null, mapOf("isBuffering" to "true", "reason" to "secret"))
        coordinator.record("buffering_changed", null, mapOf("isBuffering" to "true"))
        coordinator.record("buffering_changed", null, mapOf("isBuffering" to "false"))
        coordinator.snapshot(runId)

        val names = runtime.events().map { it.getString("eventName") }
        assertEquals(1, names.count { it == "performance_playback_buffering_start" })
        assertEquals(1, names.count { it == "performance_playback_buffering_end" })
        val bufferingEvents = runtime.events().filter {
            it.getString("eventName").startsWith("performance_playback_buffering_")
        }
        assertTrue(bufferingEvents.all {
            val attributes = it.getJSONObject("attributes")
            attributes.getString("sampleClass") == "steady" &&
                attributes.getString("overlayActive") == "false"
        })
        assertFalse(runtime.submittedJson.joinToString().contains("secret"))
        coordinator.stop(runId)
    }

    @Test
    fun stalledPlaybackCarriesOnlyNormalizedDurationAndOverlayContext() {
        val runtime = RecordingRuntime()
        val coordinator = coordinator(runtime)
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        coordinator.updateOverlayState(
            runId,
            VesperPerformanceOverlayState(
                active = true,
                sampleClass = VesperPerformanceSampleClass.Transition,
            ),
        )

        coordinator.record(
            "playback_stalled",
            null,
            mapOf(
                "count" to "2",
                "durationNs" to "600000000",
                "reason" to "private-value",
            ),
        )
        coordinator.snapshot(runId)

        val attributes = runtime.events()
            .single { it.getString("eventName") == "playback_stalled" }
            .getJSONObject("attributes")
        assertEquals("2", attributes.getString("count"))
        assertEquals("600000000", attributes.getString("durationNs"))
        assertEquals("true", attributes.getString("overlayActive"))
        assertEquals("transition", attributes.getString("sampleClass"))
        assertFalse(attributes.has("reason"))
        assertFalse(runtime.submittedJson.joinToString().contains("private-value"))
        coordinator.stop(runId)
    }

    @Test
    fun stopAndDisposeCacheTheFinalReport() {
        val coordinator = coordinator(RecordingRuntime())
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )

        val first = coordinator.stop(runId)
        val second = coordinator.stop(runId)

        assertEquals(first, second)
        assertEquals(runId, first.runId)

        val disposedCoordinator = coordinator(RecordingRuntime())
        val disposedRunId = disposedCoordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        disposedCoordinator.dispose()
        assertEquals(disposedRunId, disposedCoordinator.stop(disposedRunId).runId)
    }

    @Test
    fun sinkRejectedEventsAreNotCountedTwiceInTheReport() {
        val runtime = RecordingRuntime(
            finalDroppedEvents = 2,
        )
        val coordinator = coordinator(runtime)
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        coordinator.record("playback_error", null, emptyMap())

        val report = coordinator.snapshot(runId)

        assertEquals(2L, report.droppedEvents)
        coordinator.stop(runId)
    }

    @Test
    fun malformedMeasurementUnitUsesProtocolViolation() {
        val coordinator = coordinator(RecordingRuntime(frameBudgetUnit = "ms"))
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )

        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ProtocolViolation) {
            coordinator.snapshot(runId)
        }
        assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.ProtocolViolation) {
            coordinator.stop(runId)
        }
    }

    @Test
    fun disposeFinalizesOffCallerAndCachesShutdownTimeoutFailure() {
        val recorder = ShutdownTimeoutRecording()
        val coordinator = VesperBenchmarkCoordinator(
            performanceRecorderFactory = { _, _ -> recorder },
            frameProbeFactory = { _, _, _ -> null },
        )
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        val disposeReturned = CountDownLatch(1)
        Thread {
            coordinator.dispose()
            disposeReturned.countDown()
        }.start()

        assertTrue(recorder.shutdownStarted.await(2, TimeUnit.SECONDS))
        assertTrue(disposeReturned.await(500, TimeUnit.MILLISECONDS))

        val firstFailure = AtomicReference<Throwable?>()
        val stopReturned = CountDownLatch(1)
        Thread {
            firstFailure.set(runCatching { coordinator.stop(runId) }.exceptionOrNull())
            stopReturned.countDown()
        }.start()
        recorder.allowShutdown.countDown()
        assertTrue(stopReturned.await(2, TimeUnit.SECONDS))

        val first = firstFailure.get() as VesperPerformanceDiagnosticsException
        assertEquals(VesperPerformanceDiagnosticsErrorCode.InternalFailure, first.code)
        val second = assertDiagnosticsError(VesperPerformanceDiagnosticsErrorCode.InternalFailure) {
            coordinator.stop(runId)
        }
        assertTrue(first === second)
        assertEquals(1, recorder.shutdownCalls)
    }

    @Test
    fun concurrentStopsShareOneBoundedFinalization() {
        val runtime = RecordingRuntime(blockFlush = true)
        val coordinator = coordinator(runtime)
        val runId = coordinator.startPerformance(
            VesperPerformanceDiagnosticsConfiguration(),
            VesperPerformanceProbe.FlutterFrameTiming,
            null,
        )
        val reports = Collections.synchronizedList(mutableListOf<Result<VesperPerformanceDiagnosticsReport>>())
        val first = Thread { reports += runCatching { coordinator.stop(runId) } }.apply { start() }
        assertTrue(runtime.flushStarted.await(2, TimeUnit.SECONDS))
        val second = Thread { reports += runCatching { coordinator.stop(runId) } }.apply { start() }

        runtime.allowFlush.countDown()
        first.join(2_000)
        second.join(2_000)

        assertEquals(2, reports.size)
        assertTrue(reports.all(Result<VesperPerformanceDiagnosticsReport>::isSuccess))
        assertEquals(1, runtime.flushCount)
        assertEquals(reports[0].getOrThrow(), reports[1].getOrThrow())
    }

    private fun coordinator(
        runtime: RecordingRuntime,
        onRecorderCreated: () -> Unit = {},
        frameProbeFactory: (
            VesperPerformanceProbe,
            Window?,
            (Long, Long) -> Unit,
        ) -> AutoCloseable? = { _, _, _ -> null },
    ) = VesperBenchmarkCoordinator(
        performanceRecorderFactory = { configuration, _ ->
            onRecorderCreated()
            VesperBenchmarkRecorder(
                configuration = VesperBenchmarkConfiguration(
                    enabled = true,
                    maxBufferedEvents = configuration.maxRawEvents,
                    includeRawEvents = configuration.includeRawEvents,
                    pluginReferences = listOf(VesperBundledPluginReferences.performanceDiagnostics),
                ),
                sinkRuntime = runtime,
            )
        },
        frameProbeFactory = frameProbeFactory,
    )

    private fun assertDiagnosticsError(
        expectedCode: VesperPerformanceDiagnosticsErrorCode,
        action: () -> Unit,
    ): VesperPerformanceDiagnosticsException {
        try {
            action()
            fail("Expected VesperPerformanceDiagnosticsException")
        } catch (error: VesperPerformanceDiagnosticsException) {
            assertEquals(expectedCode, error.code)
            return error
        }
        error("unreachable")
    }

    private class RecordingRuntime(
        private val blockFlush: Boolean = false,
        private val submitDroppedEvents: Long = 0,
        private val finalDroppedEvents: Long = 0,
        private val frameBudgetUnit: String = "ns",
        private val failOpen: Boolean = false,
        private val blockOpen: Boolean = false,
    ) : VesperBenchmarkSinkRuntime {
        val submittedJson = Collections.synchronizedList(mutableListOf<String>())
        val openStarted = CountDownLatch(1)
        val allowOpen = CountDownLatch(if (blockOpen) 1 else 0)
        val flushStarted = CountDownLatch(1)
        val allowFlush = CountDownLatch(if (blockFlush) 1 else 0)
        @Volatile var openCount = 0
        @Volatile var flushCount = 0
        @Volatile var disposeCount = 0
        @Volatile var closeCount = 0

        override fun open(
            context: Context?,
            references: List<VesperPluginReference>,
        ): VesperBenchmarkSinkConnection {
            openCount += 1
            openStarted.countDown()
            allowOpen.await(2, TimeUnit.SECONDS)
            if (failOpen) throw IllegalStateException("diagnostics artifact rejected")
            return VesperBenchmarkSinkConnection(
                sessionHandle = 1,
                registry = AutoCloseable { closeCount += 1 },
            )
        }

        override fun submit(sessionHandle: Long, batchJson: String): String {
            submittedJson += batchJson
            return emptyReportJson(droppedEvents = submitDroppedEvents)
        }

        override fun flush(sessionHandle: Long): String {
            flushCount += 1
            flushStarted.countDown()
            allowFlush.await(2, TimeUnit.SECONDS)
            return emptyReportJson(droppedEvents = finalDroppedEvents)
        }

        override fun dispose(sessionHandle: Long) {
            disposeCount += 1
        }

        fun events() = submittedJson.flatMap { payload ->
            val events = JSONObject(payload).getJSONArray("events")
            List(events.length()) { index -> events.getJSONObject(index) }
        }

        private fun emptyReportJson(droppedEvents: Long = 0): String {
            val measurements = JSONArray()
            fun addMeasurement(
                name: String,
                value: Number,
                unit: String,
                cohort: String? = null,
            ) {
                measurements.put(
                    JSONObject()
                        .put("name", name)
                        .put("value", value)
                        .put("unit", unit)
                        .put(
                            "attributes",
                            JSONObject().also { attributes ->
                                cohort?.let { attributes.put("cohort", it) }
                            },
                        ),
                )
            }
            for (cohort in listOf("overlayInactive", "overlayActive", "transition", "excluded")) {
                addMeasurement("frame_sample_count", 0, "count", cohort)
                addMeasurement("frame_jank_count", 0, "count", cohort)
                addMeasurement("frame_severe_jank_count", 0, "count", cohort)
                addMeasurement("frame_jank_ratio", 0, "ratio", cohort)
                addMeasurement("frame_severe_jank_ratio", 0, "ratio", cohort)
                addMeasurement("frame_load_min", 0, "ns", cohort)
                addMeasurement("frame_load_p50", 0, "ns", cohort)
                addMeasurement("frame_load_p95", 0, "ns", cohort)
                addMeasurement("frame_load_max", 0, "ns", cohort)
            }
            addMeasurement("frame_budget", 0, frameBudgetUnit)
            addMeasurement("overlay_transitions", 0, "count")
            addMeasurement("active_playback_duration", 0, "ns")
            addMeasurement("dropped_video_frames", 0, "count")
            addMeasurement("buffering_count", 0, "count")
            addMeasurement("buffering_duration", 0, "ns")
            addMeasurement("stall_count", 0, "count")
            val diagnosis = JSONObject()
                .put("code", "performance.diagnosis")
                .put("severity", "warning")
                .put("message", "Correlation only.")
                .put(
                    "attributes",
                    JSONObject()
                        .put("kind", "insufficientEvidence")
                        .put("confidence", "low")
                        .put("evidenceCodes", "steady_cohorts_below_120"),
                )
            return JSONObject()
                .put("acceptedEvents", 0)
                .put("droppedEvents", droppedEvents)
                .put("measurements", measurements)
                .put("thresholdViolations", JSONArray())
                .put("diagnostics", JSONArray().put(diagnosis))
                .toString()
        }
    }

    private class ShutdownTimeoutRecording : VesperBenchmarkRecording {
        val shutdownStarted = CountDownLatch(1)
        val allowShutdown = CountDownLatch(1)
        @Volatile var shutdownCalls = 0
        @Volatile private var disposed = false

        override val isEnabled: Boolean
            get() = !disposed

        override fun record(
            eventName: String,
            sourceProtocol: VesperPlayerSourceProtocol?,
            attributes: Map<String, String>,
        ) = Unit

        override fun drainEvents(): List<VesperBenchmarkEvent> = emptyList()

        override fun snapshotEvents(): List<VesperBenchmarkEvent> = emptyList()

        override fun summary() = VesperBenchmarkSummary(
            runId = "shutdown-timeout-run",
            sessionId = "shutdown-timeout-session",
            acceptedEvents = 0,
            droppedEvents = 0,
            pluginAcceptedEvents = 0,
            pluginDroppedEvents = 0,
            metrics = emptyList(),
            pluginFinalReport = VesperBenchmarkSinkReport(0, 0),
            pluginErrors = emptyList(),
        )

        override fun flushSinks() = Unit

        override fun flushSinksAndAwait(timeoutMs: Long): Boolean = true

        override fun dispose() {
            disposed = true
        }

        override fun awaitSinkShutdown(timeoutMs: Long): Boolean {
            shutdownCalls += 1
            shutdownStarted.countDown()
            allowShutdown.await(timeoutMs, TimeUnit.MILLISECONDS)
            return false
        }

        override fun durationNs(): Long = 1
    }
}
