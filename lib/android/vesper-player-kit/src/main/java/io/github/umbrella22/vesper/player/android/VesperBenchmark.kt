package io.github.umbrella22.vesper.player.android

import java.util.UUID
import kotlin.math.ceil

private const val MAX_SAMPLES_PER_EVENT_NAME = 2_048

data class VesperBenchmarkConfiguration(
    val enabled: Boolean = false,
    val maxBufferedEvents: Int = 2_048,
    val includeRawEvents: Boolean = true,
    val consoleLogging: Boolean = false,
    val pluginReferences: List<VesperPluginReference> = emptyList(),
) {
    companion object {
        val Disabled = VesperBenchmarkConfiguration()
    }
}

data class VesperBenchmarkEvent(
    val runId: String,
    val sessionId: String,
    val platform: String,
    val sourceProtocol: String?,
    val eventName: String,
    val timestampNs: Long,
    val elapsedNs: Long,
    val thread: String?,
    val attributes: Map<String, String> = emptyMap(),
)

data class VesperBenchmarkMetricSummary(
    val name: String,
    val count: Int,
    val minNs: Long,
    val maxNs: Long,
    val p50Ns: Long,
    val p90Ns: Long,
    val p95Ns: Long,
)

data class VesperPluginMeasurement(
    val name: String,
    val value: Double,
    val unit: String,
    val attributes: Map<String, String> = emptyMap(),
)

data class VesperBenchmarkThresholdViolation(
    val measurement: String,
    val actual: Double,
    val threshold: Double,
    val comparison: String,
)

data class VesperPluginDiagnosticSeverity(
    val rawValue: String,
) {
    companion object {
        val Info = VesperPluginDiagnosticSeverity("info")
        val Warning = VesperPluginDiagnosticSeverity("warning")
        val Error = VesperPluginDiagnosticSeverity("error")
    }
}

data class VesperPluginDiagnostic(
    val code: String,
    val severity: VesperPluginDiagnosticSeverity,
    val message: String,
    val attributes: Map<String, String> = emptyMap(),
)

data class VesperBenchmarkSinkReport(
    val acceptedEvents: Long,
    val droppedEvents: Long,
    val measurements: List<VesperPluginMeasurement> = emptyList(),
    val thresholdViolations: List<VesperBenchmarkThresholdViolation> = emptyList(),
    val diagnostics: List<VesperPluginDiagnostic> = emptyList(),
)

data class VesperBenchmarkSummary(
    val runId: String,
    val sessionId: String,
    val acceptedEvents: Long,
    val droppedEvents: Long,
    val pluginAcceptedEvents: Long,
    val pluginDroppedEvents: Long,
    val metrics: List<VesperBenchmarkMetricSummary>,
    val pluginFinalReport: VesperBenchmarkSinkReport?,
    val pluginErrors: List<String>,
)

internal interface VesperBenchmarkRecording {
    val isEnabled: Boolean
    fun record(
        eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: Map<String, String> = emptyMap(),
    )
    fun drainEvents(): List<VesperBenchmarkEvent>
    fun summary(): VesperBenchmarkSummary
    fun dispose()
}

internal class VesperBenchmarkRecorder(
    private val configuration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
) : VesperBenchmarkRecording {
    private val lock = Any()
    private val runId = UUID.randomUUID().toString()
    private val sessionId = UUID.randomUUID().toString()
    private val baseTimestampNs = System.nanoTime()
    private val rawEvents = ArrayList<VesperBenchmarkEvent>()
    private val samplesByName = LinkedHashMap<String, MutableList<Long>>()
    private var disposed = false
    private var acceptedEvents = 0L
    private var droppedEvents = 0L
    private val pluginErrors = if (configuration.enabled && configuration.pluginReferences.isNotEmpty()) {
        listOf("Benchmark sink plugins have been retired. Benchmark events are recorded by the host.")
    } else {
        emptyList()
    }

    override val isEnabled: Boolean
        get() = configuration.enabled && synchronized(lock) { !disposed }

    override fun record(
        eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: Map<String, String>,
    ) {
        if (!configuration.enabled) {
            return
        }

        val now = System.nanoTime()
        val elapsed = (now - baseTimestampNs).coerceAtLeast(0L)
        val event =
            VesperBenchmarkEvent(
                runId = runId,
                sessionId = sessionId,
                platform = "android",
                sourceProtocol = sourceProtocol?.name?.lowercase(),
                eventName = eventName,
                timestampNs = now,
                elapsedNs = elapsed,
                thread = Thread.currentThread().name,
                attributes = attributes,
            )

        synchronized(lock) {
            if (disposed) return
            acceptedEvents += 1
            val samples = samplesByName.getOrPut(eventName) { ArrayList() }
            if (samples.size >= MAX_SAMPLES_PER_EVENT_NAME) {
                samples.removeAt(0)
            }
            samples.add(elapsed)
            if (configuration.includeRawEvents) {
                if (rawEvents.size < configuration.maxBufferedEvents.coerceAtLeast(0)) {
                    rawEvents += event
                } else {
                    droppedEvents += 1
                }
            }
        }
    }

    override fun drainEvents(): List<VesperBenchmarkEvent> =
        synchronized(lock) {
            val events = rawEvents.toList()
            rawEvents.clear()
            events
        }

    override fun summary(): VesperBenchmarkSummary =
        synchronized(lock) {
            VesperBenchmarkSummary(
                runId = runId,
                sessionId = sessionId,
                acceptedEvents = acceptedEvents,
                droppedEvents = droppedEvents,
                pluginAcceptedEvents = 0,
                pluginDroppedEvents = 0,
                metrics =
                    samplesByName.map { (name, samples) ->
                        metricSummary(name, samples)
                    }.sortedBy { it.name },
                pluginFinalReport = null,
                pluginErrors = pluginErrors,
            )
        }

    override fun dispose() {
        synchronized(lock) { disposed = true }
    }

    private fun metricSummary(
        name: String,
        samples: List<Long>,
    ): VesperBenchmarkMetricSummary {
        val sorted = samples.sorted()
        return VesperBenchmarkMetricSummary(
            name = name,
            count = sorted.size,
            minNs = sorted.firstOrNull() ?: 0L,
            maxNs = sorted.lastOrNull() ?: 0L,
            p50Ns = percentile(sorted, 0.50),
            p90Ns = percentile(sorted, 0.90),
            p95Ns = percentile(sorted, 0.95),
        )
    }

    private fun percentile(
        sorted: List<Long>,
        ratio: Double,
    ): Long {
        if (sorted.isEmpty()) {
            return 0L
        }
        val index = ceil((sorted.size - 1).toDouble() * ratio).toInt()
            .coerceIn(0, sorted.lastIndex)
        return sorted[index]
    }
}
