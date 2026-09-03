package io.github.umbrella22.vesper.player.android

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingDeque
import java.util.concurrent.TimeUnit
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.ceil

private const val MAX_SAMPLES_PER_EVENT_NAME = 2_048
private const val MAX_BENCHMARK_PLUGIN_ERRORS = 128
private const val BENCHMARK_SINK_QUEUE_CAPACITY = 1_024

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

internal data class VesperBenchmarkSinkConnection(
    val sessionHandle: Long,
    val registry: AutoCloseable,
)

internal interface VesperBenchmarkSinkRuntime {
    fun open(
        context: Context?,
        references: List<VesperPluginReference>,
    ): VesperBenchmarkSinkConnection

    fun submit(
        sessionHandle: Long,
        batchJson: String,
    ): String

    fun flush(sessionHandle: Long): String

    fun dispose(sessionHandle: Long)
}

private object JniVesperBenchmarkSinkRuntime : VesperBenchmarkSinkRuntime {
    override fun open(
        context: Context?,
        references: List<VesperPluginReference>,
    ): VesperBenchmarkSinkConnection {
        val appContext =
            requireNotNull(context) {
                "Android Context is required when benchmark plugin references are configured"
            }.applicationContext
        val registry = VesperEmbeddedPluginRegistry.create(appContext, references)
        return try {
            val sessionHandle =
                VesperNativeJni.createBenchmarkSinkSessionFromRegistry(
                    registry.handle,
                    encodeVesperPluginReferences(references),
                )
            check(sessionHandle != 0L) { "benchmark sink session handle must not be zero" }
            VesperBenchmarkSinkConnection(sessionHandle, registry)
        } catch (error: Throwable) {
            registry.close()
            throw error
        }
    }

    override fun submit(
        sessionHandle: Long,
        batchJson: String,
    ): String = VesperNativeJni.submitBenchmarkSinkEvents(sessionHandle, batchJson)

    override fun flush(sessionHandle: Long): String =
        VesperNativeJni.flushBenchmarkSinkSession(sessionHandle)

    override fun dispose(sessionHandle: Long) {
        VesperNativeJni.disposeBenchmarkSinkSession(sessionHandle)
    }
}

internal interface VesperBenchmarkRecording {
    val isEnabled: Boolean
    fun record(
        eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: Map<String, String> = emptyMap(),
    )
    fun drainEvents(): List<VesperBenchmarkEvent>
    fun snapshotEvents(): List<VesperBenchmarkEvent>
    fun summary(): VesperBenchmarkSummary
    fun flushSinks()
    fun flushSinksAndAwait(timeoutMs: Long): Boolean
    fun awaitSinkReadiness(timeoutMs: Long): VesperBenchmarkSinkReadiness =
        VesperBenchmarkSinkReadiness.Ready
    fun dispose()
    fun awaitSinkShutdown(timeoutMs: Long): Boolean
    fun durationNs(): Long
}

internal sealed interface VesperBenchmarkSinkReadiness {
    data object Ready : VesperBenchmarkSinkReadiness

    data object OpenFailed : VesperBenchmarkSinkReadiness

    data object TimedOut : VesperBenchmarkSinkReadiness
}

internal class VesperBenchmarkRecorder(
    private val configuration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
    context: Context? = null,
    private val sinkRuntime: VesperBenchmarkSinkRuntime = JniVesperBenchmarkSinkRuntime,
) : VesperBenchmarkRecording {
    private val lock = Any()
    private val runId = UUID.randomUUID().toString()
    private val sessionId = UUID.randomUUID().toString()
    private val baseTimestampNs = System.nanoTime()
    private val rawEvents = ArrayList<VesperBenchmarkEvent>()
    private val samplesByName = LinkedHashMap<String, MutableList<Long>>()
    private val sinkWorker: BenchmarkSinkWorker?
    private val disposed = AtomicBoolean(false)
    private var acceptedEvents = 0L
    private var droppedEvents = 0L
    private var pluginAcceptedEvents = 0L
    private var pluginDroppedEvents = 0L
    private var pluginFinalReport: VesperBenchmarkSinkReport? = null
    private val pluginErrors = ArrayList<String>()
    private val sinkShutdownTimeoutRecorded = AtomicBoolean(false)

    override val isEnabled: Boolean
        get() = configuration.enabled

    init {
        sinkWorker =
            if (configuration.enabled && configuration.pluginReferences.isNotEmpty()) {
                BenchmarkSinkWorker(
                    context = context,
                    references = configuration.pluginReferences,
                    runtime = sinkRuntime,
                    onSubmitReport = ::recordSinkSubmitReport,
                    onFinalReport = ::recordSinkFinalReport,
                    onError = ::recordPluginError,
                    onQueueDrop = ::recordPluginQueueDrop,
                ).also { it.start() }
            } else {
                null
            }
    }

    override fun record(
        eventName: String,
        sourceProtocol: VesperPlayerSourceProtocol?,
        attributes: Map<String, String>,
    ) {
        if (!configuration.enabled || disposed.get()) {
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

        sinkWorker?.offer(event)
    }

    override fun drainEvents(): List<VesperBenchmarkEvent> =
        synchronized(lock) {
            val events = rawEvents.toList()
            rawEvents.clear()
            events
        }

    override fun snapshotEvents(): List<VesperBenchmarkEvent> =
        synchronized(lock) {
            rawEvents.toList()
        }

    override fun durationNs(): Long = (System.nanoTime() - baseTimestampNs).coerceAtLeast(0L)

    override fun summary(): VesperBenchmarkSummary =
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
                pluginFinalReport = pluginFinalReport,
                pluginErrors = pluginErrors.toList(),
            )
        }

    override fun flushSinks() {
        if (disposed.get()) {
            return
        }
        sinkWorker?.flush()
    }

    override fun flushSinksAndAwait(timeoutMs: Long): Boolean {
        if (disposed.get()) {
            return true
        }
        return sinkWorker?.flushAndAwait(timeoutMs) ?: true
    }

    override fun awaitSinkReadiness(timeoutMs: Long): VesperBenchmarkSinkReadiness =
        sinkWorker?.awaitReadiness(timeoutMs) ?: VesperBenchmarkSinkReadiness.Ready

    override fun dispose() {
        if (!disposed.compareAndSet(false, true)) {
            return
        }
        sinkWorker?.dispose()
    }

    override fun awaitSinkShutdown(timeoutMs: Long): Boolean {
        val completed = sinkWorker?.awaitShutdown(timeoutMs) ?: true
        if (!completed && sinkShutdownTimeoutRecorded.compareAndSet(false, true)) {
            recordPluginError("benchmark sink shutdown timed out")
        }
        return completed
    }

    private fun recordSinkSubmitReport(report: VesperBenchmarkSinkReport) {
        synchronized(lock) {
            pluginAcceptedEvents += report.acceptedEvents
            pluginDroppedEvents += report.droppedEvents
        }
    }

    private fun recordSinkFinalReport(report: VesperBenchmarkSinkReport) {
        synchronized(lock) {
            pluginFinalReport = report
        }
    }

    private fun recordPluginQueueDrop() {
        synchronized(lock) {
            pluginDroppedEvents += 1
        }
    }

    private fun recordPluginError(error: String) {
        synchronized(lock) {
            if (pluginErrors.size >= MAX_BENCHMARK_PLUGIN_ERRORS) {
                pluginErrors.removeAt(0)
            }
            pluginErrors += error
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

private sealed interface BenchmarkSinkCommand {
    data class Submit(val event: VesperBenchmarkEvent) : BenchmarkSinkCommand

    class Flush(
        val completions: MutableList<CountDownLatch> = ArrayList(),
    ) : BenchmarkSinkCommand

    data object Dispose : BenchmarkSinkCommand
}

/**
 * Owns all potentially blocking plugin calls and serializes their lifecycle.
 * The queue is deliberately bounded. Incoming events are rejected at capacity;
 * lifecycle controls may evict only the newest pending event.
 */
private class BenchmarkSinkWorker(
    private val context: Context?,
    private val references: List<VesperPluginReference>,
    private val runtime: VesperBenchmarkSinkRuntime,
    private val onSubmitReport: (VesperBenchmarkSinkReport) -> Unit,
    private val onFinalReport: (VesperBenchmarkSinkReport) -> Unit,
    private val onError: (String) -> Unit,
    private val onQueueDrop: () -> Unit,
) {
    private val queue = LinkedBlockingDeque<BenchmarkSinkCommand>(BENCHMARK_SINK_QUEUE_CAPACITY)
    private val accepting = AtomicBoolean(true)
    private val readiness = CountDownLatch(1)
    @Volatile private var openFailed = false
    private val shutdown = CountDownLatch(1)
    private val thread =
        Thread(::run, "vesper-benchmark-sink").apply {
            isDaemon = true
        }

    fun start() {
        thread.start()
    }

    fun offer(event: VesperBenchmarkEvent) {
        if (!accepting.get()) {
            onQueueDrop()
            return
        }
        synchronized(queue) {
            if (!accepting.get()) {
                onQueueDrop()
                return
            }
            if (queue.offerLast(BenchmarkSinkCommand.Submit(event))) {
                return
            }
            onQueueDrop()
        }
    }

    fun flush() {
        synchronized(queue) {
            if (!accepting.get() || queue.any { it is BenchmarkSinkCommand.Flush }) {
                return
            }
            enqueueControlLocked(BenchmarkSinkCommand.Flush())
        }
    }

    fun flushAndAwait(timeoutMs: Long): Boolean {
        if (!accepting.get()) {
            return true
        }
        val completion = CountDownLatch(1)
        synchronized(queue) {
            if (!accepting.get()) {
                return true
            }
            val pendingFlush = queue.firstOrNull { it is BenchmarkSinkCommand.Flush }
                as? BenchmarkSinkCommand.Flush
            if (pendingFlush != null) {
                pendingFlush.completions += completion
            } else if (!enqueueControlLocked(BenchmarkSinkCommand.Flush(mutableListOf(completion)))) {
                return false
            }
        }
        return completion.await(timeoutMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)
    }

    fun awaitReadiness(timeoutMs: Long): VesperBenchmarkSinkReadiness {
        if (!readiness.await(timeoutMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)) {
            return VesperBenchmarkSinkReadiness.TimedOut
        }
        return if (openFailed) {
            VesperBenchmarkSinkReadiness.OpenFailed
        } else {
            VesperBenchmarkSinkReadiness.Ready
        }
    }

    fun dispose() {
        if (!accepting.compareAndSet(true, false)) {
            return
        }
        synchronized(queue) {
            enqueueControlLocked(BenchmarkSinkCommand.Dispose)
        }
    }

    fun awaitShutdown(timeoutMs: Long): Boolean =
        shutdown.await(timeoutMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)

    private fun enqueueControlLocked(command: BenchmarkSinkCommand): Boolean {
        if (queue.offerLast(command)) {
            return true
        }
        val newestPendingEvent = queue.toList().lastOrNull { it is BenchmarkSinkCommand.Submit }
        if (newestPendingEvent != null && queue.removeLastOccurrence(newestPendingEvent)) {
            onQueueDrop()
        }
        if (!queue.offerLast(command)) {
            // Controls are never discarded. Reaching this branch means the
            // bounded queue contains only controls, which violates the worker contract.
            onError("benchmark sink control queue is unavailable")
            return false
        }
        return true
    }

    private fun run() {
        var connection: VesperBenchmarkSinkConnection? = null
        try {
            connection = try {
                runtime.open(context, references)
            } catch (error: Throwable) {
                openFailed = true
                onError(error.message ?: "benchmark sink session create failed")
                null
            } finally {
                readiness.countDown()
            }

            var disposing = false
            while (!disposing) {
                val command = try {
                    queue.take()
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    break
                }
                when (command) {
                    is BenchmarkSinkCommand.Submit -> {
                        val activeConnection = connection ?: continue
                        submit(activeConnection, drainBatch(command.event))
                    }
                    is BenchmarkSinkCommand.Flush -> {
                        try {
                            connection?.let(::flush)
                        } finally {
                            command.completions.forEach(CountDownLatch::countDown)
                        }
                    }
                    BenchmarkSinkCommand.Dispose -> disposing = true
                }
            }

            // Dispose is appended after all accepted event commands, so this
            // drain preserves submit -> final flush -> dispose ordering.
            while (true) {
                val command = queue.pollFirst() ?: break
                when (command) {
                    is BenchmarkSinkCommand.Submit -> {
                        val activeConnection = connection ?: continue
                        submit(activeConnection, drainBatch(command.event))
                    }
                    is BenchmarkSinkCommand.Flush -> {
                        try {
                            connection?.let(::flush)
                        } finally {
                            command.completions.forEach(CountDownLatch::countDown)
                        }
                    }
                    BenchmarkSinkCommand.Dispose -> Unit
                }
            }
            connection?.let { activeConnection ->
                flush(activeConnection)
                try {
                    runtime.dispose(activeConnection.sessionHandle)
                } catch (error: Throwable) {
                    onError(error.message ?: "benchmark sink dispose failed")
                }
                try {
                    activeConnection.registry.close()
                } catch (error: Throwable) {
                    onError(error.message ?: "benchmark plugin registry close failed")
                }
            }
        } finally {
            shutdown.countDown()
        }
    }

    private fun submit(
        connection: VesperBenchmarkSinkConnection,
        events: List<VesperBenchmarkEvent>,
    ) {
        try {
            onSubmitReport(
                parseSinkReport(
                    runtime.submit(
                        connection.sessionHandle,
                        benchmarkBatchJson(events),
                    ),
                ),
            )
        } catch (error: Throwable) {
            onError(error.message ?: "benchmark sink submit failed")
        }
    }

    private fun drainBatch(first: VesperBenchmarkEvent): List<VesperBenchmarkEvent> {
        val events = ArrayList<VesperBenchmarkEvent>(120)
        events += first
        synchronized(queue) {
            while (events.size < 120) {
                val next = queue.peekFirst() as? BenchmarkSinkCommand.Submit ?: break
                queue.removeFirst()
                events += next.event
            }
        }
        return events
    }

    private fun flush(connection: VesperBenchmarkSinkConnection) {
        try {
            onFinalReport(parseSinkReport(runtime.flush(connection.sessionHandle)))
        } catch (error: Throwable) {
            onError(error.message ?: "benchmark sink flush failed")
        }
    }
}

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

private fun parseSinkReport(json: String): VesperBenchmarkSinkReport {
    val payload = JSONObject(json)
    return VesperBenchmarkSinkReport(
        acceptedEvents = payload.getLong("acceptedEvents"),
        droppedEvents = payload.getLong("droppedEvents"),
        measurements =
            payload.getJSONArray("measurements").mapObjects { measurement ->
                VesperPluginMeasurement(
                    name = measurement.getString("name"),
                    value = measurement.getDouble("value"),
                    unit = measurement.getString("unit"),
                    attributes = measurement.getJSONObject("attributes").toStringMap(),
                )
            },
        thresholdViolations =
            payload.getJSONArray("thresholdViolations").mapObjects { violation ->
                VesperBenchmarkThresholdViolation(
                    measurement = violation.getString("measurement"),
                    actual = violation.getDouble("actual"),
                    threshold = violation.getDouble("threshold"),
                    comparison = violation.getString("comparison"),
                )
            },
        diagnostics =
            payload.getJSONArray("diagnostics").mapObjects { diagnostic ->
                VesperPluginDiagnostic(
                    code = diagnostic.getString("code"),
                    severity =
                        VesperPluginDiagnosticSeverity(
                            diagnostic.getString("severity"),
                        ),
                    message = diagnostic.getString("message"),
                    attributes = diagnostic.getJSONObject("attributes").toStringMap(),
                )
            },
    )
}

private inline fun <T> JSONArray.mapObjects(transform: (JSONObject) -> T): List<T> =
    buildList {
        for (index in 0 until length()) {
            add(transform(getJSONObject(index)))
        }
    }

private fun JSONObject.toStringMap(): Map<String, String> =
    buildMap {
        val keys = keys()
        while (keys.hasNext()) {
            val key = keys.next()
            put(key, getString(key))
        }
    }
