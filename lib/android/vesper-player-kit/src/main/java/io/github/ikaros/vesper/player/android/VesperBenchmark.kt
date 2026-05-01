package io.github.ikaros.vesper.player.android

import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import kotlin.math.ceil

data class VesperBenchmarkConfiguration(
    val enabled: Boolean = false,
    val maxBufferedEvents: Int = 2_048,
    val includeRawEvents: Boolean = true,
    val pluginLibraryPaths: List<String> = emptyList(),
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

data class VesperBenchmarkSummary(
    val runId: String,
    val sessionId: String,
    val acceptedEvents: Long,
    val droppedEvents: Long,
    val pluginAcceptedEvents: Long,
    val pluginDroppedEvents: Long,
    val metrics: List<VesperBenchmarkMetricSummary>,
    val pluginErrors: List<String>,
)

class VesperBenchmarkRecorder(
    private val configuration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
) {
    private val lock = Any()
    private val runId = UUID.randomUUID().toString()
    private val sessionId = UUID.randomUUID().toString()
    private val baseTimestampNs = System.nanoTime()
    private val rawEvents = ArrayList<VesperBenchmarkEvent>()
    private val samplesByName = LinkedHashMap<String, MutableList<Long>>()
    private val sinkSessionHandle: Long?
    private var acceptedEvents = 0L
    private var droppedEvents = 0L
    private var pluginAcceptedEvents = 0L
    private var pluginDroppedEvents = 0L
    private val pluginErrors = ArrayList<String>()

    val isEnabled: Boolean
        get() = configuration.enabled

    init {
        sinkSessionHandle =
            if (configuration.enabled && configuration.pluginLibraryPaths.isNotEmpty()) {
                runCatching {
                    VesperNativeJni.createBenchmarkSinkSession(
                        configuration.pluginLibraryPaths.toTypedArray()
                    )
                }.onFailure { error ->
                    synchronized(lock) {
                        pluginErrors += error.message ?: "benchmark sink session create failed"
                    }
                }.getOrNull()?.takeIf { it != 0L }
            } else {
                null
            }
    }

    fun record(
        eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: Map<String, String> = emptyMap(),
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
            acceptedEvents += 1
            samplesByName.getOrPut(eventName) { ArrayList() }.add(elapsed)
            if (configuration.includeRawEvents) {
                if (rawEvents.size < configuration.maxBufferedEvents.coerceAtLeast(0)) {
                    rawEvents += event
                } else {
                    droppedEvents += 1
                }
            }
        }

        submitBenchmarkEvents(listOf(event))
    }

    fun drainEvents(): List<VesperBenchmarkEvent> =
        synchronized(lock) {
            val events = rawEvents.toList()
            rawEvents.clear()
            events
        }

    fun summary(): VesperBenchmarkSummary =
        synchronized(lock) {
            VesperBenchmarkSummary(
                runId = runId,
                sessionId = sessionId,
                acceptedEvents = acceptedEvents,
                droppedEvents = droppedEvents,
                pluginAcceptedEvents = pluginAcceptedEvents,
                pluginDroppedEvents = pluginDroppedEvents,
                metrics =
                    samplesByName.map { (name, samples) ->
                        metricSummary(name, samples)
                    }.sortedBy { it.name },
                pluginErrors = pluginErrors.toList(),
            )
        }

    fun flushSinks() {
        val handle = sinkSessionHandle ?: return
        runCatching {
            parseSinkReport(VesperNativeJni.flushBenchmarkSinkSession(handle))
        }.onSuccess { report ->
            synchronized(lock) {
                pluginErrors += report.pluginErrors
            }
        }.onFailure { error ->
            synchronized(lock) {
                pluginErrors += error.message ?: "benchmark sink flush failed"
            }
        }
    }

    fun dispose() {
        flushSinks()
        sinkSessionHandle?.let(VesperNativeJni::disposeBenchmarkSinkSession)
    }

    private fun submitBenchmarkEvents(events: List<VesperBenchmarkEvent>) {
        val handle = sinkSessionHandle ?: return
        runCatching {
            val reportJson = VesperNativeJni.submitBenchmarkSinkEvents(
                handle,
                benchmarkBatchJson(events),
            )
            parseSinkReport(reportJson)
        }.onSuccess { report ->
            synchronized(lock) {
                pluginAcceptedEvents += report.acceptedEvents
                pluginDroppedEvents += report.droppedEvents
                pluginErrors += report.pluginErrors
            }
        }.onFailure { error ->
            synchronized(lock) {
                pluginErrors += error.message ?: "benchmark sink submit failed"
            }
        }
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

private data class BenchmarkSinkReport(
    val acceptedEvents: Long,
    val droppedEvents: Long,
    val pluginErrors: List<String>,
)

private fun benchmarkBatchJson(events: List<VesperBenchmarkEvent>): String {
    val array = JSONArray()
    events.forEach { event ->
        array.put(event.toJsonObject())
    }
    return JSONObject().put("events", array).toString()
}

private fun VesperBenchmarkEvent.toJsonObject(): JSONObject {
    val attributesJson = JSONObject()
    attributes.toSortedMap().forEach { (key, value) ->
        attributesJson.put(key, value)
    }
    return JSONObject()
        .put("runId", runId)
        .put("sessionId", sessionId)
        .put("platform", platform)
        .put("sourceProtocol", sourceProtocol ?: JSONObject.NULL)
        .put("eventName", eventName)
        .put("timestampNs", timestampNs)
        .put("elapsedNs", elapsedNs)
        .put("thread", thread ?: JSONObject.NULL)
        .put("attributes", attributesJson)
}

private fun parseSinkReport(json: String): BenchmarkSinkReport {
    val payload = JSONObject(json)
    val errors = payload.optJSONArray("pluginErrors") ?: JSONArray()
    return BenchmarkSinkReport(
        acceptedEvents = payload.optLong("acceptedEvents", 0L),
        droppedEvents = payload.optLong("droppedEvents", 0L),
        pluginErrors =
            buildList {
                for (index in 0 until errors.length()) {
                    add(errors.optString(index))
                }
            },
    )
}
