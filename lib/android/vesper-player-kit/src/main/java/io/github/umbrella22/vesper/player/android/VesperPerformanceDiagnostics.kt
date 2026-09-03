package io.github.umbrella22.vesper.player.android

import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.view.FrameMetrics
import android.view.Window
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

private const val PERFORMANCE_DIAGNOSTICS_FLUSH_TIMEOUT_MS = 2_000L
private const val PERFORMANCE_DIAGNOSTICS_START_TIMEOUT_MS = 2_000L
private const val PERFORMANCE_DIAGNOSTICS_FINALIZATION_TIMEOUT_MS = 3_000L
private const val PERFORMANCE_MARKER_LIMIT = 64
private const val MAX_PERFORMANCE_MARKER_BYTES = 64

data class VesperPerformanceDiagnosticsConfiguration(
    val includeRawEvents: Boolean = false,
    val maxRawEvents: Int = 256,
)

data class VesperPerformanceSampleClass(val rawValue: String) {
    companion object {
        @JvmField val Steady = VesperPerformanceSampleClass("steady")
        @JvmField val Transition = VesperPerformanceSampleClass("transition")
        @JvmField val Excluded = VesperPerformanceSampleClass("excluded")
    }
}

data class VesperPerformanceProbe(val rawValue: String) {
    companion object {
        @JvmField val FlutterFrameTiming = VesperPerformanceProbe("flutterFrameTiming")
        @JvmField val AndroidFrameMetrics = VesperPerformanceProbe("androidFrameMetrics")
        @JvmField val IosDisplayLink = VesperPerformanceProbe("iosDisplayLink")
    }
}

data class VesperPerformanceOverlayState(
    val active: Boolean,
    val sampleClass: VesperPerformanceSampleClass = VesperPerformanceSampleClass.Steady,
    val loadedBasicItemCount: Int? = null,
    val loadedAdvancedItemCount: Int? = null,
    val advancedEffectsActive: Boolean = false,
)

data class VesperPerformanceFrameCohort(
    val sampleCount: Long,
    val jankCount: Long,
    val severeJankCount: Long,
    val jankRatio: Double,
    val severeJankRatio: Double,
    val minLoadNs: Long,
    val p50LoadNs: Long,
    val p95LoadNs: Long,
    val maxLoadNs: Long,
) {
    val minLoadMs: Double get() = minLoadNs / 1_000_000.0
    val p50LoadMs: Double get() = p50LoadNs / 1_000_000.0
    val p95LoadMs: Double get() = p95LoadNs / 1_000_000.0
    val maxLoadMs: Double get() = maxLoadNs / 1_000_000.0
}

data class VesperPerformanceFrameSample(
    val loadNs: Long,
    val budgetNs: Long,
    val overlayState: VesperPerformanceOverlayState? = null,
)

data class VesperPerformancePlaybackSummary(
    val activeDurationNs: Long,
    val droppedVideoFrames: Long,
    val bufferingCount: Long,
    val bufferingDurationNs: Long,
    val stallCount: Long,
) {
    val activeDurationMs: Double get() = activeDurationNs / 1_000_000.0
    val bufferingDurationMs: Double get() = bufferingDurationNs / 1_000_000.0
}

data class VesperPerformanceDiagnosisKind(val rawValue: String) {
    companion object {
        @JvmField val InsufficientEvidence = VesperPerformanceDiagnosisKind("insufficientEvidence")
        @JvmField val NoSignificantPressure = VesperPerformanceDiagnosisKind("noSignificantPressure")
        @JvmField val OverlayCorrelatedUiPressure = VesperPerformanceDiagnosisKind("overlayCorrelatedUiPressure")
        @JvmField val HostUiPressureUncorrelated = VesperPerformanceDiagnosisKind("hostUiPressureUncorrelated")
        @JvmField val PlaybackPressure = VesperPerformanceDiagnosisKind("playbackPressure")
        @JvmField val MixedPressure = VesperPerformanceDiagnosisKind("mixedPressure")
    }
}

data class VesperPerformanceConfidence(val rawValue: String) {
    companion object {
        @JvmField val Low = VesperPerformanceConfidence("low")
        @JvmField val Medium = VesperPerformanceConfidence("medium")
        @JvmField val High = VesperPerformanceConfidence("high")
    }
}

data class VesperPerformanceDiagnosticSeverity(val rawValue: String) {
    companion object {
        @JvmField val Info = VesperPerformanceDiagnosticSeverity("info")
        @JvmField val Warning = VesperPerformanceDiagnosticSeverity("warning")
        @JvmField val Error = VesperPerformanceDiagnosticSeverity("error")
    }
}

data class VesperPerformanceDiagnosis(
    val kind: VesperPerformanceDiagnosisKind,
    val confidence: VesperPerformanceConfidence,
    val evidenceCodes: List<String>,
)

data class VesperPerformanceDiagnostic(
    val code: String,
    val severity: VesperPerformanceDiagnosticSeverity,
    val message: String,
    val attributes: Map<String, String> = emptyMap(),
)

data class VesperPerformanceDiagnosticsReport(
    val schemaVersion: Int = 1,
    val runId: String,
    val sessionId: String,
    val platform: String,
    val probe: VesperPerformanceProbe,
    val durationNs: Long,
    val frameBudgetNs: Long,
    val cohorts: Map<String, VesperPerformanceFrameCohort>,
    val playback: VesperPerformancePlaybackSummary,
    val diagnosis: VesperPerformanceDiagnosis,
    val acceptedEvents: Long,
    val droppedEvents: Long,
    val rawEventsDropped: Long,
    val diagnostics: List<VesperPerformanceDiagnostic>,
    val rawEvents: List<VesperBenchmarkEvent>,
) {
    val durationMs: Double get() = durationNs / 1_000_000.0
    val frameBudgetMs: Double get() = frameBudgetNs / 1_000_000.0
}

data class VesperPerformanceDiagnosticsErrorCode(val rawValue: String) {
    companion object {
        @JvmField val AlreadyActive = VesperPerformanceDiagnosticsErrorCode("alreadyActive")
        @JvmField val ArtifactUnavailable = VesperPerformanceDiagnosticsErrorCode("artifactUnavailable")
        @JvmField val ProbeUnavailable = VesperPerformanceDiagnosticsErrorCode("probeUnavailable")
        @JvmField val InvalidConfiguration = VesperPerformanceDiagnosticsErrorCode("invalidConfiguration")
        @JvmField val ControllerDisposed = VesperPerformanceDiagnosticsErrorCode("controllerDisposed")
        @JvmField val ProtocolViolation = VesperPerformanceDiagnosticsErrorCode("protocolViolation")
        @JvmField val InternalFailure = VesperPerformanceDiagnosticsErrorCode("internalFailure")
    }
}

class VesperPerformanceDiagnosticsException(
    val code: VesperPerformanceDiagnosticsErrorCode,
    message: String,
    cause: Throwable? = null,
) : IllegalStateException(message, cause)

class VesperPerformanceDiagnosticsSession internal constructor(
    private val controller: VesperPlayerController,
    val runId: String,
) {
    private val stopMutex = Mutex()
    @Volatile private var finalReport: VesperPerformanceDiagnosticsReport? = null

    fun updateOverlayState(state: VesperPerformanceOverlayState) {
        controller.updatePerformanceOverlayState(runId, state)
    }

    fun recordMarker(
        name: String,
        value: Double? = null,
        sequenceIndex: Int? = null,
        expectedOverlayActive: Boolean? = null,
    ) {
        controller.recordPerformanceMarker(
            runId,
            name,
            value,
            sequenceIndex,
            expectedOverlayActive,
        )
    }

    /** Submits one bounded batch from an external UI frame probe such as Flutter FrameTiming. */
    fun submitFrameSamples(samples: List<VesperPerformanceFrameSample>) {
        controller.submitPerformanceFrameSamples(runId, samples)
    }

    suspend fun snapshot(): VesperPerformanceDiagnosticsReport =
        controller.performanceDiagnosticsSnapshot(runId)

    suspend fun stop(): VesperPerformanceDiagnosticsReport = stopMutex.withLock {
        finalReport ?: controller.stopPerformanceDiagnostics(runId).also { finalReport = it }
    }
}

internal class VesperBenchmarkCoordinator(
    configuration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
    private val context: android.content.Context? = null,
    private val performanceArtifactValidator: ((android.content.Context) -> Unit)? = null,
    private val performanceRecorderFactory: ((
        VesperPerformanceDiagnosticsConfiguration,
        android.content.Context?,
    ) -> VesperBenchmarkRecording)? = null,
    private val frameProbeFactory: ((
        VesperPerformanceProbe,
        Window?,
        (Long, Long) -> Unit,
    ) -> AutoCloseable?) = ::createAndroidFrameProbe,
    private val performanceStartTimeoutMs: Long = PERFORMANCE_DIAGNOSTICS_START_TIMEOUT_MS,
) : VesperBenchmarkRecording {
    internal class ActiveRun(
        val recorder: VesperBenchmarkRecording,
        val mode: Mode,
        val probe: VesperPerformanceProbe?,
        overlayState: VesperPerformanceOverlayState,
        initialPlaybackActive: Boolean = false,
        var frameProbe: AutoCloseable? = null,
        var markerCount: Int = 0,
    ) {
        @Volatile var overlayState: VesperPerformanceOverlayState = overlayState
        var playbackPlaying: Boolean = initialPlaybackActive
        var buffering: Boolean = false
        var activePlaybackStartedNs: Long? = if (initialPlaybackActive) System.nanoTime() else null
        var accumulatedActivePlaybackNs: Long = 0

        fun updatePlaybackActivity(nowNs: Long = System.nanoTime()) {
            val shouldBeActive = playbackPlaying && !buffering
            val startedNs = activePlaybackStartedNs
            if (shouldBeActive && startedNs == null) {
                activePlaybackStartedNs = nowNs
            } else if (!shouldBeActive && startedNs != null) {
                accumulatedActivePlaybackNs = accumulatedActivePlaybackNs.saturatingPerformanceAdd(
                    (nowNs - startedNs).coerceAtLeast(0L),
                )
                activePlaybackStartedNs = null
            }
        }

        fun activePlaybackDurationNs(nowNs: Long = System.nanoTime()): Long =
            accumulatedActivePlaybackNs.saturatingPerformanceAdd(
                activePlaybackStartedNs?.let { (nowNs - it).coerceAtLeast(0L) } ?: 0L,
            )
    }

    internal enum class Mode { Legacy, Performance }

    private class PendingFinalization(val runId: String) {
        val completion = CountDownLatch(1)
        @Volatile var report: VesperPerformanceDiagnosticsReport? = null
        @Volatile var failure: VesperPerformanceDiagnosticsException? = null
    }

    private val lock = Any()
    private val disabledRecorder = VesperBenchmarkRecorder()
    @Volatile private var activeRun: ActiveRun? =
        if (configuration.enabled) {
            ActiveRun(
                recorder = VesperBenchmarkRecorder(configuration, context),
                mode = Mode.Legacy,
                probe = null,
                overlayState = VesperPerformanceOverlayState(active = false),
            )
        } else {
            null
        }
    @Volatile private var lastPerformanceReport: VesperPerformanceDiagnosticsReport? = null
    @Volatile private var lastPerformanceFailure:
        Pair<String, VesperPerformanceDiagnosticsException>? = null
    @Volatile private var pendingFinalization: PendingFinalization? = null
    @Volatile private var disposed = false

    override val isEnabled: Boolean
        get() = activeRun?.recorder?.isEnabled == true

    override fun record(
        eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: Map<String, String>,
    ) {
        val run = activeRun ?: return
        if (run.mode == Mode.Legacy) {
            run.recorder.record(eventName, sourceProtocol, attributes)
            return
        }
        if (recordNormalizedPlaybackEvent(run, eventName, attributes)) return
        val safeAttributes = sanitizePerformanceAttributes(eventName, attributes) ?: return
        run.recorder.record(eventName, sourceProtocol, safeAttributes)
    }

    fun startPerformance(
        configuration: VesperPerformanceDiagnosticsConfiguration,
        probe: VesperPerformanceProbe,
        window: Window?,
        initialPlaybackActive: Boolean = false,
    ): String {
        synchronized(lock) {
            if (disposed) {
                throw VesperPerformanceDiagnosticsException(
                    VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
                    "The player controller has been disposed.",
                )
            }
            if (activeRun != null || pendingFinalization != null) {
                throw VesperPerformanceDiagnosticsException(
                    VesperPerformanceDiagnosticsErrorCode.AlreadyActive,
                    "A performance diagnostics run is already active.",
                )
            }
        }
        validatePerformanceConfiguration(configuration)
        if (probe != VesperPerformanceProbe.AndroidFrameMetrics &&
            probe != VesperPerformanceProbe.FlutterFrameTiming
        ) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.ProbeUnavailable,
                "The requested Android performance probe is unavailable.",
            )
        }
        val recorder = createPerformanceRecorder(configuration)
        val run = ActiveRun(
            recorder = recorder,
            mode = Mode.Performance,
            probe = probe,
            overlayState = VesperPerformanceOverlayState(active = false),
            initialPlaybackActive = initialPlaybackActive,
        )
        val published = synchronized(lock) {
            if (!disposed && activeRun == null && pendingFinalization == null) {
                activeRun = run
                true
            } else {
                false
            }
        }
        if (!published) {
            val controllerWasDisposed = disposed
            cleanupUnpublishedRun(run)
            throw VesperPerformanceDiagnosticsException(
                if (controllerWasDisposed) {
                    VesperPerformanceDiagnosticsErrorCode.ControllerDisposed
                } else {
                    VesperPerformanceDiagnosticsErrorCode.AlreadyActive
                },
                if (controllerWasDisposed) {
                    "The player controller has been disposed."
                } else {
                    "A performance diagnostics run is already active."
                },
            )
        }
        val readiness = recorder.awaitSinkReadiness(performanceStartTimeoutMs)
        if (!isCurrentRun(run)) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
                "The player controller was disposed while performance diagnostics started.",
            )
        }
        when (readiness) {
            VesperBenchmarkSinkReadiness.Ready -> Unit
            VesperBenchmarkSinkReadiness.OpenFailed -> {
                unpublishRun(run)
                val cleanupFailure = cleanupUnpublishedRun(run)
                throw performanceError(
                    VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable,
                    "The Vesper performance diagnostics artifact could not be opened.",
                    cleanupFailure,
                )
            }
            VesperBenchmarkSinkReadiness.TimedOut -> {
                unpublishRun(run)
                val cleanupFailure = cleanupUnpublishedRun(run)
                throw performanceError(
                    VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                    "Performance diagnostics artifact startup timed out.",
                    cleanupFailure,
                )
            }
        }
        try {
            run.frameProbe = frameProbeFactory(probe, window) { loadNs, budgetNs ->
                recordPerformanceFrame(run, loadNs, budgetNs)
            }
            recorder.record(
                "performance_session_context",
                null,
                mapOf("probe" to probe.rawValue, "activePlaybackNs" to "0"),
            )
        } catch (error: Throwable) {
            unpublishRun(run)
            val cleanupFailure = cleanupUnpublishedRun(run)
            if (error is VesperPerformanceDiagnosticsException) throw error
            throw VesperPerformanceDiagnosticsException(
                VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                "Performance diagnostics could not start.",
                cleanupFailure ?: error,
            )
        }
        if (!isCurrentRun(run)) {
            cleanupUnpublishedRun(run)
            throw VesperPerformanceDiagnosticsException(
                VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
                "The player controller was disposed while performance diagnostics started.",
            )
        }
        lastPerformanceFailure = null
        return recorder.summary().runId
    }

    fun updateOverlayState(runId: String, state: VesperPerformanceOverlayState) {
        validateOverlayState(state)
        val run = requirePerformanceRun(runId)
        val previous = run.overlayState
        run.overlayState = state
        if (previous.active != state.active || previous.sampleClass != state.sampleClass) {
            run.recorder.record(
                "performance_overlay_transition",
                null,
                overlayAttributes(state),
            )
        }
    }

    fun recordMarker(
        runId: String,
        name: String,
        value: Double?,
        sequenceIndex: Int?,
        expectedOverlayActive: Boolean?,
    ) {
        if (!isValidMarker(name)) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
                "Performance marker names must be ASCII identifiers up to 64 bytes.",
            )
        }
        if (value != null && !value.isFinite()) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
                "Performance marker values must be finite.",
            )
        }
        val run = requirePerformanceRun(runId)
        synchronized(lock) {
            if (run.markerCount >= PERFORMANCE_MARKER_LIMIT) {
                throw performanceError(
                    VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
                    "A performance diagnostics run accepts at most 64 markers.",
                )
            }
            run.markerCount += 1
        }
        run.recorder.record(
            "performance_marker",
            null,
            buildMap {
                put("name", name)
                value?.let { put("value", it.toString()) }
                sequenceIndex?.let { put("sequenceIndex", it.toString()) }
                expectedOverlayActive?.let { put("expectedOverlayActive", it.toString()) }
            },
        )
    }

    fun recordPerformanceFrames(runId: String, samples: List<VesperPerformanceFrameSample>) {
        val run = requirePerformanceRun(runId)
        if (samples.size > 120) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
                "Performance frame batches are limited to 120 samples.",
            )
        }
        for (sample in samples) {
            validateFrameSample(sample)
            recordPerformanceFrame(run, sample.loadNs, sample.budgetNs, sample.overlayState)
        }
    }

    fun snapshot(runId: String): VesperPerformanceDiagnosticsReport {
        val run = requirePerformanceRun(runId)
        recordSessionContext(run)
        if (!run.recorder.flushSinksAndAwait(PERFORMANCE_DIAGNOSTICS_FLUSH_TIMEOUT_MS)) {
            throw VesperPerformanceDiagnosticsException(
                VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                "Performance diagnostics snapshot timed out.",
            )
        }
        return buildPerformanceReport(run)
    }

    fun stop(runId: String): VesperPerformanceDiagnosticsReport {
        lastPerformanceReport?.takeIf { it.runId == runId }?.let { return it }
        lastPerformanceFailure?.takeIf { it.first == runId }?.let { throw it.second }
        var runToFinalize: ActiveRun? = null
        val pending = synchronized(lock) {
            lastPerformanceReport?.takeIf { it.runId == runId }?.let { return it }
            lastPerformanceFailure?.takeIf { it.first == runId }?.let { throw it.second }
            pendingFinalization?.let { existing ->
                if (existing.runId == runId) return@synchronized existing
                throw VesperPerformanceDiagnosticsException(
                    VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
                    "The performance diagnostics session is no longer active.",
                )
            }
            val current = activeRun
            if (current == null || current.mode != Mode.Performance || current.recorder.summary().runId != runId) {
                throw VesperPerformanceDiagnosticsException(
                    VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
                    "The performance diagnostics session is no longer active.",
                )
            }
            activeRun = null
            runToFinalize = current
            PendingFinalization(runId).also { pendingFinalization = it }
        }
        runToFinalize?.let { completeFinalization(it, pending) }
        return awaitFinalization(pending)
    }

    override fun drainEvents(): List<VesperBenchmarkEvent> =
        activeRun?.recorder?.drainEvents() ?: emptyList()

    override fun summary(): VesperBenchmarkSummary =
        activeRun?.recorder?.summary()
            ?: lastPerformanceReport?.let(::legacySummaryFromPerformanceReport)
            ?: disabledRecorder.summary()

    override fun flushSinks() {
        activeRun?.recorder?.flushSinks()
    }

    override fun flushSinksAndAwait(timeoutMs: Long): Boolean =
        activeRun?.recorder?.flushSinksAndAwait(timeoutMs) ?: true

    override fun snapshotEvents(): List<VesperBenchmarkEvent> =
        activeRun?.recorder?.snapshotEvents().orEmpty()

    override fun durationNs(): Long = activeRun?.recorder?.durationNs() ?: 0L

    override fun awaitSinkShutdown(timeoutMs: Long): Boolean =
        activeRun?.recorder?.awaitSinkShutdown(timeoutMs) ?: true

    override fun dispose() {
        var performancePending: PendingFinalization? = null
        val run = synchronized(lock) {
            disposed = true
            activeRun?.also { current ->
                activeRun = null
                if (current.mode == Mode.Performance) {
                    performancePending = PendingFinalization(current.recorder.summary().runId)
                    pendingFinalization = performancePending
                }
            }
        } ?: return
        if (run.mode == Mode.Legacy) {
            run.frameProbe?.close()
            run.recorder.dispose()
            return
        }
        val pending = requireNotNull(performancePending)
        Thread(
            { completeFinalization(run, pending) },
            "vesper-performance-finalizer",
        ).apply {
            isDaemon = true
            start()
        }
    }

    private fun isCurrentRun(run: ActiveRun): Boolean =
        synchronized(lock) { !disposed && activeRun === run }

    private fun unpublishRun(run: ActiveRun) {
        synchronized(lock) {
            if (activeRun === run) activeRun = null
        }
    }

    private fun recordPerformanceFrame(
        expectedRun: ActiveRun,
        loadNs: Long,
        budgetNs: Long,
        sampleOverlayState: VesperPerformanceOverlayState? = null,
    ) {
        val run = activeRun ?: return
        if (run !== expectedRun || run.mode != Mode.Performance || loadNs < 0 || budgetNs <= 0) return
        run.recorder.record(
            "performance_frame_sample",
            null,
            overlayAttributes(sampleOverlayState ?: run.overlayState) + mapOf(
                "frameLoadNs" to loadNs.toString(),
                "frameBudgetNs" to budgetNs.toString(),
                "probe" to requireNotNull(run.probe).rawValue,
            ),
        )
    }

    private fun recordSessionContext(run: ActiveRun) {
        val activePlaybackNs = synchronized(lock) { run.activePlaybackDurationNs() }
        run.recorder.record(
            "performance_session_context",
            null,
            mapOf(
                "probe" to requireNotNull(run.probe).rawValue,
                "activePlaybackNs" to activePlaybackNs.toString(),
            ),
        )
    }

    private fun requirePerformanceRun(runId: String): ActiveRun {
        return synchronized(lock) {
            val run = activeRun
            if (run == null || run.mode != Mode.Performance || run.recorder.summary().runId != runId) {
                throw VesperPerformanceDiagnosticsException(
                    VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
                    "The performance diagnostics session is no longer active.",
                )
            }
            run
        }
    }

    private fun completeFinalization(run: ActiveRun, pending: PendingFinalization) {
        try {
            var cleanupFailure = runCatching { run.frameProbe?.close() }.exceptionOrNull()
            synchronized(lock) { run.updatePlaybackActivity() }
            recordSessionContext(run)
            runCatching { run.recorder.dispose() }
                .onFailure { if (cleanupFailure == null) cleanupFailure = it }
            runCatching {
                if (!run.recorder.awaitSinkShutdown(PERFORMANCE_DIAGNOSTICS_FLUSH_TIMEOUT_MS)) {
                    throw performanceError(
                        VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                        "Performance diagnostics sink shutdown timed out.",
                    )
                }
            }.onFailure { if (cleanupFailure == null) cleanupFailure = it }
            cleanupFailure?.let { throw it }
            val report = buildPerformanceReport(run)
            pending.report = report
            lastPerformanceReport = report
        } catch (error: Throwable) {
            val failure = if (error is VesperPerformanceDiagnosticsException) {
                error
            } else {
                performanceError(
                    VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                    "Performance diagnostics could not stop cleanly.",
                    error,
                )
            }
            pending.failure = failure
            lastPerformanceFailure = pending.runId to failure
        } finally {
            synchronized(lock) {
                if (pendingFinalization === pending) pendingFinalization = null
            }
            pending.completion.countDown()
        }
    }

    private fun cleanupUnpublishedRun(run: ActiveRun): Throwable? {
        var failure = runCatching { run.frameProbe?.close() }.exceptionOrNull()
        runCatching { run.recorder.dispose() }
            .onFailure { if (failure == null) failure = it }
        runCatching {
            if (!run.recorder.awaitSinkShutdown(PERFORMANCE_DIAGNOSTICS_FLUSH_TIMEOUT_MS)) {
                throw performanceError(
                    VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                    "Performance diagnostics cleanup timed out.",
                )
            }
        }.onFailure { if (failure == null) failure = it }
        return failure
    }

    private fun awaitFinalization(pending: PendingFinalization): VesperPerformanceDiagnosticsReport {
        if (!pending.completion.await(
                PERFORMANCE_DIAGNOSTICS_FINALIZATION_TIMEOUT_MS,
                TimeUnit.MILLISECONDS,
            )
        ) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.InternalFailure,
                "Performance diagnostics finalization timed out.",
            )
        }
        pending.report?.let { return it }
        throw pending.failure ?: performanceError(
            VesperPerformanceDiagnosticsErrorCode.InternalFailure,
            "Performance diagnostics finalization did not produce a report.",
        )
    }

    private fun createPerformanceRecorder(
        configuration: VesperPerformanceDiagnosticsConfiguration,
    ): VesperBenchmarkRecording {
        performanceRecorderFactory?.let { factory ->
            return try {
                factory(configuration, context)
            } catch (error: Throwable) {
                throw performanceError(
                    VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable,
                    "The Vesper performance diagnostics artifact is unavailable.",
                    error,
                )
            }
        }
        val appContext = context ?: throw performanceError(
            VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable,
            "An Android application context is required to load the diagnostics artifact.",
        )
        try {
            performanceArtifactValidator?.invoke(appContext)
                ?: VesperBundledPluginResolver.requirePerformanceDiagnostics(appContext)
        } catch (error: Throwable) {
            throw performanceError(
                VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable,
                "The Vesper performance diagnostics artifact is unavailable.",
                error,
            )
        }
        return VesperBenchmarkRecorder(
            configuration = VesperBenchmarkConfiguration(
                enabled = true,
                maxBufferedEvents = if (configuration.includeRawEvents) {
                    configuration.maxRawEvents
                } else {
                    0
                },
                includeRawEvents = configuration.includeRawEvents,
                pluginReferences = listOf(VesperBundledPluginReferences.performanceDiagnostics),
            ),
            context = appContext,
        )
    }

    private fun recordNormalizedPlaybackEvent(
        run: ActiveRun,
        eventName: String,
        attributes: Map<String, String>,
    ): Boolean {
        when (eventName) {
            "playback_state_changed" -> {
                synchronized(lock) {
                    run.playbackPlaying = attributes["state"]?.equals("playing", ignoreCase = true) == true
                    run.updatePlaybackActivity()
                }
                return true
            }
            "buffering_changed" -> {
                val buffering = attributes["isBuffering"]?.toBooleanStrictOrNull() ?: return true
                val standardizedEvent = synchronized(lock) {
                    if (run.buffering == buffering) {
                        null
                    } else {
                        run.buffering = buffering
                        run.updatePlaybackActivity()
                        if (buffering) {
                            "performance_playback_buffering_start"
                        } else {
                            "performance_playback_buffering_end"
                        }
                    }
                }
                standardizedEvent?.let {
                    run.recorder.record(it, null, overlayAttributes(run.overlayState))
                }
                return true
            }
            "playback_stalled" -> {
                val count = attributes["count"]?.toLongOrNull()?.takeIf { it >= 0L } ?: 1L
                val durationNs = attributes["durationNs"]
                    ?.toLongOrNull()
                    ?.takeIf { it >= 0L }
                run.recorder.record(
                    "playback_stalled",
                    null,
                    overlayAttributes(run.overlayState) + buildMap {
                        put("count", count.toString())
                        durationNs?.let { put("durationNs", it.toString()) }
                    },
                )
                return true
            }
        }
        return false
    }
}

private fun createAndroidFrameProbe(
    probe: VesperPerformanceProbe,
    window: Window?,
    onFrame: (Long, Long) -> Unit,
): AutoCloseable? = when (probe) {
    VesperPerformanceProbe.AndroidFrameMetrics -> {
        val activeWindow = window ?: throw performanceError(
            VesperPerformanceDiagnosticsErrorCode.ProbeUnavailable,
            "Android FrameMetrics requires a foreground Window.",
        )
        AndroidFrameMetricsProbe(activeWindow, onFrame)
    }
    VesperPerformanceProbe.FlutterFrameTiming -> null
    else -> throw performanceError(
        VesperPerformanceDiagnosticsErrorCode.ProbeUnavailable,
        "The requested Android performance probe is unavailable.",
    )
}

private class AndroidFrameMetricsProbe(
    private val window: Window,
    private val onFrame: (Long, Long) -> Unit,
) : AutoCloseable {
    private val callbackThread = HandlerThread("vesper-frame-metrics").apply { start() }
    private val callbackHandler = Handler(callbackThread.looper)
    private val listener = Window.OnFrameMetricsAvailableListener { _, metrics, _ ->
        val loadNs = metrics.getMetric(FrameMetrics.TOTAL_DURATION)
        val refreshRate = window.decorView.display?.refreshRate?.takeIf { it > 0f } ?: 60f
        val budgetNs = (1_000_000_000.0 / refreshRate.toDouble()).toLong().coerceAtLeast(1L)
        if (loadNs >= 0L) onFrame(loadNs, budgetNs)
    }
    private val closed = AtomicBoolean(false)
    private val registered = AtomicBoolean(false)

    init {
        try {
            runOnMainAndWait {
                window.addOnFrameMetricsAvailableListener(listener, callbackHandler)
                registered.set(true)
            }
        } catch (error: Throwable) {
            stopCallbackThread()
            throw error
        }
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        try {
            if (registered.compareAndSet(true, false)) {
                runOnMainAndWait(cancelOnTimeout = false) {
                    window.removeOnFrameMetricsAvailableListener(listener)
                }
            }
        } finally {
            stopCallbackThread()
        }
    }

    private fun stopCallbackThread() {
        callbackThread.quitSafely()
        if (Thread.currentThread() !== callbackThread) {
            callbackThread.join(1_000)
        }
    }
}

private fun runOnMainAndWait(
    cancelOnTimeout: Boolean = true,
    action: () -> Unit,
) {
    if (Looper.myLooper() == Looper.getMainLooper()) {
        action()
        return
    }
    val completion = CountDownLatch(1)
    val cancelled = AtomicBoolean(false)
    val failure = AtomicReference<Throwable?>(null)
    val posted = Handler(Looper.getMainLooper()).post {
        try {
            if (!cancelled.get()) action()
        } catch (error: Throwable) {
            failure.set(error)
        } finally {
            completion.countDown()
        }
    }
    check(posted) { "Android main-thread operation could not be posted" }
    if (!completion.await(1, TimeUnit.SECONDS)) {
        if (cancelOnTimeout) cancelled.set(true)
        error("Android main-thread operation timed out")
    }
    failure.get()?.let { throw it }
}

private fun overlayAttributes(state: VesperPerformanceOverlayState): Map<String, String> =
    buildMap {
        put("overlayActive", state.active.toString())
        put("sampleClass", state.sampleClass.rawValue)
        put("advancedEffectsActive", state.advancedEffectsActive.toString())
        state.loadedBasicItemCount?.let { put("loadedBasicItemCount", it.toString()) }
        state.loadedAdvancedItemCount?.let { put("loadedAdvancedItemCount", it.toString()) }
    }

private fun sanitizePerformanceAttributes(
    eventName: String,
    attributes: Map<String, String>,
): Map<String, String>? {
    val allowedKeys = when (eventName) {
        "dropped_video_frames", "playback_stalled" -> setOf("count")
        "playback_error" -> setOf("code", "category", "retriable")
        "first_frame_rendered", "playback_ended",
        "initialize_start", "initialize_completed", "source_load_start",
        "source_load_configured", "performance_playback_buffering_start",
        "performance_playback_buffering_end" -> emptySet()
        else -> return null
    }
    return attributes.filterKeys(allowedKeys::contains)
}

private fun isValidMarker(name: String): Boolean =
    name.isNotEmpty() &&
        name.toByteArray(Charsets.US_ASCII).size <= MAX_PERFORMANCE_MARKER_BYTES &&
        (name.first() in 'a'..'z' || name.first() in 'A'..'Z' || name.first() == '_') &&
        name.all {
            it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' ||
                it == '_' || it == '.' || it == '-'
        }

private fun validatePerformanceConfiguration(
    configuration: VesperPerformanceDiagnosticsConfiguration,
) {
    if (configuration.maxRawEvents !in 0..2_048) {
        throw performanceError(
            VesperPerformanceDiagnosticsErrorCode.InvalidConfiguration,
            "maxRawEvents must be between 0 and 2048.",
        )
    }
}

private fun validateOverlayState(state: VesperPerformanceOverlayState) {
    val validSampleClass = state.sampleClass == VesperPerformanceSampleClass.Steady ||
        state.sampleClass == VesperPerformanceSampleClass.Transition ||
        state.sampleClass == VesperPerformanceSampleClass.Excluded
    if (!validSampleClass ||
        state.loadedBasicItemCount?.let { it < 0 } == true ||
        state.loadedAdvancedItemCount?.let { it < 0 } == true
    ) {
        throw performanceError(
            VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
            "The performance overlay state is invalid.",
        )
    }
}

private fun validateFrameSample(sample: VesperPerformanceFrameSample) {
    if (sample.loadNs < 0 || sample.budgetNs <= 0) {
        throw performanceError(
            VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
            "Performance frame samples require a non-negative load and positive budget.",
        )
    }
    sample.overlayState?.let(::validateOverlayState)
}

private fun performanceError(
    code: VesperPerformanceDiagnosticsErrorCode,
    message: String,
    cause: Throwable? = null,
) = VesperPerformanceDiagnosticsException(code, message, cause)

private fun Long.saturatingPerformanceAdd(other: Long): Long =
    if (other > 0 && this > Long.MAX_VALUE - other) Long.MAX_VALUE else this + other

private fun buildPerformanceReport(
    run: VesperBenchmarkCoordinator.ActiveRun,
): VesperPerformanceDiagnosticsReport {
    val recorder = run.recorder
    val summary = recorder.summary()
    val pluginReport = summary.pluginFinalReport ?: throw performanceError(
        VesperPerformanceDiagnosticsErrorCode.InternalFailure,
        "The performance diagnostics sink did not produce a report.",
    )
    if (pluginReport.acceptedEvents < 0 || pluginReport.droppedEvents < 0) {
        throw performanceReportProtocolViolation()
    }
    val measurements = PerformanceMeasurementReader(pluginReport.measurements)
    fun cohort(name: String): VesperPerformanceFrameCohort {
        val cohort = VesperPerformanceFrameCohort(
            sampleCount = measurements.count("frame_sample_count", name),
            jankCount = measurements.count("frame_jank_count", name),
            severeJankCount = measurements.count("frame_severe_jank_count", name),
            jankRatio = measurements.ratio("frame_jank_ratio", name),
            severeJankRatio = measurements.ratio("frame_severe_jank_ratio", name),
            minLoadNs = measurements.nanoseconds("frame_load_min", name),
            p50LoadNs = measurements.nanoseconds("frame_load_p50", name),
            p95LoadNs = measurements.nanoseconds("frame_load_p95", name),
            maxLoadNs = measurements.nanoseconds("frame_load_max", name),
        )
        if (cohort.severeJankCount > cohort.jankCount ||
            cohort.jankCount > cohort.sampleCount ||
            cohort.minLoadNs > cohort.p50LoadNs ||
            cohort.p50LoadNs > cohort.p95LoadNs ||
            cohort.p95LoadNs > cohort.maxLoadNs
        ) {
            throw performanceReportProtocolViolation()
        }
        return cohort
    }
    val diagnosisDiagnostics = pluginReport.diagnostics.filter {
        it.code == "performance.diagnosis"
    }
    if (diagnosisDiagnostics.size != 1) throw performanceReportProtocolViolation()
    val diagnosisDiagnostic = diagnosisDiagnostics.single()
    val diagnosisKind = diagnosisDiagnostic.attributes["kind"]
        ?.takeIf(String::isNotEmpty) ?: throw performanceReportProtocolViolation()
    val diagnosisConfidence = diagnosisDiagnostic.attributes["confidence"]
        ?.takeIf(String::isNotEmpty) ?: throw performanceReportProtocolViolation()
    val evidenceCodes = diagnosisDiagnostic.attributes["evidenceCodes"]
        ?.split(',')?.takeIf { codes -> codes.isNotEmpty() && codes.all(String::isNotEmpty) }
        ?: throw performanceReportProtocolViolation()
    val cohorts = listOf("overlayInactive", "overlayActive", "transition", "excluded")
        .associateWith(::cohort)
    val frameBudgetNs = measurements.nanoseconds("frame_budget")
    if (frameBudgetNs == 0L && cohorts.values.any { it.sampleCount > 0 }) {
        throw performanceReportProtocolViolation()
    }
    return VesperPerformanceDiagnosticsReport(
        runId = summary.runId,
        sessionId = summary.sessionId,
        platform = "android",
        probe = run.probe ?: VesperPerformanceProbe("unknown"),
        durationNs = recorder.durationNs(),
        frameBudgetNs = frameBudgetNs,
        cohorts = cohorts,
        playback = VesperPerformancePlaybackSummary(
            activeDurationNs = measurements.nanoseconds("active_playback_duration"),
            droppedVideoFrames = measurements.count("dropped_video_frames"),
            bufferingCount = measurements.count("buffering_count"),
            bufferingDurationNs = measurements.nanoseconds("buffering_duration"),
            stallCount = measurements.count("stall_count"),
        ),
        diagnosis = VesperPerformanceDiagnosis(
            kind = VesperPerformanceDiagnosisKind(diagnosisKind),
            confidence = VesperPerformanceConfidence(diagnosisConfidence),
            evidenceCodes = evidenceCodes,
        ),
        acceptedEvents = pluginReport.acceptedEvents,
        droppedEvents = maxOf(pluginReport.droppedEvents, summary.pluginDroppedEvents),
        rawEventsDropped = summary.droppedEvents,
        diagnostics = pluginReport.diagnostics.map { diagnostic ->
            VesperPerformanceDiagnostic(
                code = diagnostic.code,
                severity = VesperPerformanceDiagnosticSeverity(diagnostic.severity.rawValue),
                message = diagnostic.message,
                attributes = diagnostic.attributes,
            )
        },
        rawEvents = recorder.snapshotEvents(),
    )
}

private class PerformanceMeasurementReader(
    private val measurements: List<VesperPluginMeasurement>,
) {
    fun count(name: String, cohort: String? = null): Long =
        exactNonnegativeLong(name, "count", cohort)

    fun nanoseconds(name: String, cohort: String? = null): Long =
        exactNonnegativeLong(name, "ns", cohort)

    fun ratio(name: String, cohort: String? = null): Double {
        val value = requiredValue(name, "ratio", cohort)
        if (value !in 0.0..1.0) throw performanceReportProtocolViolation()
        return value
    }

    private fun exactNonnegativeLong(name: String, unit: String, cohort: String?): Long {
        val value = requiredValue(name, unit, cohort)
        if (value < 0.0 || value % 1.0 != 0.0 || value >= 9_223_372_036_854_775_808.0) {
            throw performanceReportProtocolViolation()
        }
        return value.toLong()
    }

    private fun requiredValue(name: String, unit: String, cohort: String?): Double {
        val matches = measurements.filter { measurement ->
            measurement.name == name && measurement.attributes["cohort"] == cohort
        }
        if (matches.size != 1) throw performanceReportProtocolViolation()
        val measurement = matches.single()
        if (measurement.unit != unit || !measurement.value.isFinite() || measurement.value < 0.0) {
            throw performanceReportProtocolViolation()
        }
        return measurement.value
    }
}

private fun performanceReportProtocolViolation() = performanceError(
    VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
    "The performance diagnostics sink returned a malformed schema v1 report.",
)

private fun legacySummaryFromPerformanceReport(
    report: VesperPerformanceDiagnosticsReport,
): VesperBenchmarkSummary = VesperBenchmarkSummary(
    runId = report.runId,
    sessionId = report.sessionId,
    acceptedEvents = report.acceptedEvents,
    droppedEvents = report.rawEventsDropped,
    pluginAcceptedEvents = report.acceptedEvents,
    pluginDroppedEvents = report.droppedEvents,
    metrics = emptyList(),
    pluginFinalReport = null,
    pluginErrors = emptyList(),
)
