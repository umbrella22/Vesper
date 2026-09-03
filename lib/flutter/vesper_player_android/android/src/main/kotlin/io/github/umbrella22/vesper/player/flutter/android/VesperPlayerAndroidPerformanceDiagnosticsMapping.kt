package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsConfiguration
import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsReport
import io.github.umbrella22.vesper.player.android.VesperPerformanceFrameCohort
import io.github.umbrella22.vesper.player.android.VesperPerformanceFrameSample
import io.github.umbrella22.vesper.player.android.VesperPerformanceOverlayState
import io.github.umbrella22.vesper.player.android.VesperPerformancePlaybackSummary
import io.github.umbrella22.vesper.player.android.VesperPerformanceSampleClass
import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsException
import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsErrorCode
import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsSession

internal fun PlayerSession.requirePerformanceDiagnostics(
    arguments: Map<String, Any?>,
): VesperPerformanceDiagnosticsSession {
    val requestedRunId = arguments["runId"] as? String
        ?: throw IllegalArgumentException("Missing performance diagnostics runId.")
    return performanceDiagnosticsSession?.takeIf { it.runId == requestedRunId }
        ?: throw VesperPerformanceDiagnosticsException(
            VesperPerformanceDiagnosticsErrorCode.ControllerDisposed,
            "The performance diagnostics session is no longer active.",
        )
}

internal fun Map<String, Any?>.toPerformanceDiagnosticsConfiguration():
    VesperPerformanceDiagnosticsConfiguration {
    val includeRawEvents = optionalPerformanceBoolean(
        "includeRawEvents",
        defaultValue = false,
        code = VesperPerformanceDiagnosticsErrorCode.InvalidConfiguration,
    )
    val maxRawEvents = if (containsKey("maxRawEvents")) {
        requiredPerformanceInt(
            "maxRawEvents",
            VesperPerformanceDiagnosticsErrorCode.InvalidConfiguration,
        )
    } else {
        256
    }
    return VesperPerformanceDiagnosticsConfiguration(
        includeRawEvents = includeRawEvents,
        maxRawEvents = maxRawEvents,
    )
}

internal fun Map<String, Any?>.toPerformanceOverlayState() =
    VesperPerformanceOverlayState(
        active = optionalPerformanceBoolean("active", defaultValue = false),
        sampleClass = VesperPerformanceSampleClass(
            this["sampleClass"] as? String ?: "steady",
        ),
        loadedBasicItemCount = optionalPerformanceInt("loadedBasicItemCount"),
        loadedAdvancedItemCount = optionalPerformanceInt("loadedAdvancedItemCount"),
        advancedEffectsActive = optionalPerformanceBoolean(
            "advancedEffectsActive",
            defaultValue = false,
        ),
    )

internal fun Map<String, Any?>.toPerformanceFrameSample() =
    VesperPerformanceFrameSample(
        loadNs = requiredPerformanceLong("loadNs"),
        budgetNs = requiredPerformanceLong("budgetNs"),
        overlayState = (this["overlayState"] as? Map<*, *>)?.stringMap()
            ?.toPerformanceOverlayState(),
    )

internal fun Map<String, Any?>.optionalPerformanceMarkerValue(): Double? {
    if (!containsKey("value") || this["value"] == null) return null
    val value = (this["value"] as? Number)?.toDouble()
    if (value == null || !value.isFinite()) {
        throw performanceMappingError(
            VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
            "Performance marker value must be a finite number.",
        )
    }
    return value
}

internal fun Map<String, Any?>.optionalPerformanceSequenceIndex(): Int? =
    optionalPerformanceInt("sequenceIndex")

internal fun Map<String, Any?>.optionalPerformanceExpectedOverlayActive(): Boolean? {
    if (!containsKey("expectedOverlayActive") || this["expectedOverlayActive"] == null) return null
    return this["expectedOverlayActive"] as? Boolean
        ?: throw performanceMappingError(
            VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
            "expectedOverlayActive must be a boolean.",
        )
}

internal fun VesperPerformanceDiagnosticsReport.toMap(): Map<String, Any?> =
    mapOf(
        "schemaVersion" to schemaVersion,
        "runId" to runId,
        "sessionId" to sessionId,
        "platform" to platform,
        "probe" to probe.rawValue,
        "durationNs" to durationNs,
        "frameBudgetNs" to frameBudgetNs,
        "cohorts" to cohorts.mapValues { (_, value) -> value.toMap() },
        "playback" to playback.toMap(),
        "diagnosis" to mapOf(
            "kind" to diagnosis.kind.rawValue,
            "confidence" to diagnosis.confidence.rawValue,
            "evidenceCodes" to diagnosis.evidenceCodes,
        ),
        "acceptedEvents" to acceptedEvents,
        "droppedEvents" to droppedEvents,
        "rawEventsDropped" to rawEventsDropped,
        "diagnostics" to diagnostics.map { diagnostic ->
            mapOf(
                "code" to diagnostic.code,
                "severity" to diagnostic.severity.rawValue,
                "message" to diagnostic.message,
                "attributes" to diagnostic.attributes,
            )
        },
        "rawEvents" to rawEvents.map { event ->
            mapOf(
                "runId" to event.runId,
                "sessionId" to event.sessionId,
                "platform" to event.platform,
                "sourceProtocol" to event.sourceProtocol,
                "eventName" to event.eventName,
                "timestampNs" to event.timestampNs,
                "elapsedNs" to event.elapsedNs,
                "thread" to event.thread,
                "attributes" to event.attributes,
            )
        },
    )

private fun VesperPerformanceFrameCohort.toMap(): Map<String, Any?> =
    mapOf(
        "sampleCount" to sampleCount,
        "jankCount" to jankCount,
        "severeJankCount" to severeJankCount,
        "jankRatio" to jankRatio,
        "severeJankRatio" to severeJankRatio,
        "minLoadNs" to minLoadNs,
        "p50LoadNs" to p50LoadNs,
        "p95LoadNs" to p95LoadNs,
        "maxLoadNs" to maxLoadNs,
    )

private fun VesperPerformancePlaybackSummary.toMap(): Map<String, Any?> =
    mapOf(
        "activeDurationNs" to activeDurationNs,
        "droppedVideoFrames" to droppedVideoFrames,
        "bufferingCount" to bufferingCount,
        "bufferingDurationNs" to bufferingDurationNs,
        "stallCount" to stallCount,
    )

private fun Map<String, Any?>.requiredPerformanceLong(
    key: String,
    code: VesperPerformanceDiagnosticsErrorCode =
        VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
): Long = exactPerformanceLong(this[key])
    ?: throw performanceMappingError(code, "$key must be a signed 64-bit integer.")

private fun Map<String, Any?>.requiredPerformanceInt(
    key: String,
    code: VesperPerformanceDiagnosticsErrorCode =
        VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
): Int {
    val value = requiredPerformanceLong(key, code)
    if (value !in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()) {
        throw performanceMappingError(code, "$key must be a platform integer.")
    }
    return value.toInt()
}

private fun Map<String, Any?>.optionalPerformanceInt(
    key: String,
    code: VesperPerformanceDiagnosticsErrorCode =
        VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
): Int? {
    if (!containsKey(key) || this[key] == null) return null
    return requiredPerformanceInt(key, code)
}

private fun Map<String, Any?>.optionalPerformanceBoolean(
    key: String,
    defaultValue: Boolean,
    code: VesperPerformanceDiagnosticsErrorCode =
        VesperPerformanceDiagnosticsErrorCode.ProtocolViolation,
): Boolean {
    if (!containsKey(key)) return defaultValue
    return this[key] as? Boolean
        ?: throw performanceMappingError(code, "$key must be a boolean.")
}

private fun exactPerformanceLong(value: Any?): Long? = when (value) {
    is Byte -> value.toLong()
    is Short -> value.toLong()
    is Int -> value.toLong()
    is Long -> value
    else -> null
}

private fun performanceMappingError(
    code: VesperPerformanceDiagnosticsErrorCode,
    message: String,
) = VesperPerformanceDiagnosticsException(code, message)
