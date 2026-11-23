package io.github.umbrella22.vesper.player.android

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

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

internal fun retiredPerformanceDiagnostics(): Nothing =
    throw VesperPerformanceDiagnosticsException(
        VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable,
        "The Vesper performance diagnostics plugin has been retired.",
    )

internal fun inactivePerformanceDiagnostics(): Nothing =
    throw VesperPerformanceDiagnosticsException(
        VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
        "The performance diagnostics session is no longer active.",
    )
