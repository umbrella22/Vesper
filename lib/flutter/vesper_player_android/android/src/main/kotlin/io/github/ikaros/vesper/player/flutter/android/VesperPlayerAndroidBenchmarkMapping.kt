package io.github.ikaros.vesper.player.flutter.android

import android.util.Log
import io.github.ikaros.vesper.player.android.VesperBenchmarkEvent
import io.github.ikaros.vesper.player.android.VesperBenchmarkMetricSummary
import io.github.ikaros.vesper.player.android.VesperBenchmarkSinkReport
import io.github.ikaros.vesper.player.android.VesperBenchmarkSummary
import io.github.ikaros.vesper.player.android.VesperBenchmarkThresholdViolation
import io.github.ikaros.vesper.player.android.VesperPluginDiagnostic
import io.github.ikaros.vesper.player.android.VesperPluginMeasurement
import org.json.JSONArray
import org.json.JSONObject

internal fun List<VesperBenchmarkEvent>.toBenchmarkJsonArray(): JSONArray =
    JSONArray().also { array ->
        forEach { event -> array.put(event.toBenchmarkJsonObject()) }
    }

internal fun VesperBenchmarkEvent.toBenchmarkJsonObject(): JSONObject {
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

internal fun VesperBenchmarkSummary.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("runId", runId)
        .put("sessionId", sessionId)
        .put("acceptedEvents", acceptedEvents)
        .put("droppedEvents", droppedEvents)
        .put("pluginAcceptedEvents", pluginAcceptedEvents)
        .put("pluginDroppedEvents", pluginDroppedEvents)
        .put(
            "metrics",
            JSONArray().also { array ->
                metrics.forEach { metric -> array.put(metric.toBenchmarkJsonObject()) }
            },
        )
        .put(
            "pluginErrors",
            JSONArray().also { array ->
                pluginErrors.forEach { error -> array.put(error) }
            },
        )
        .put(
            "pluginFinalReport",
            pluginFinalReport?.toBenchmarkJsonObject() ?: JSONObject.NULL,
        )

internal fun VesperBenchmarkMetricSummary.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("name", name)
        .put("count", count)
        .put("minNs", minNs)
        .put("maxNs", maxNs)
        .put("p50Ns", p50Ns)
        .put("p90Ns", p90Ns)
        .put("p95Ns", p95Ns)

internal fun VesperBenchmarkSinkReport.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("acceptedEvents", acceptedEvents)
        .put("droppedEvents", droppedEvents)
        .put(
            "measurements",
            JSONArray().also { array ->
                measurements.forEach { measurement ->
                    array.put(measurement.toBenchmarkJsonObject())
                }
            },
        )
        .put(
            "thresholdViolations",
            JSONArray().also { array ->
                thresholdViolations.forEach { violation ->
                    array.put(violation.toBenchmarkJsonObject())
                }
            },
        )
        .put(
            "diagnostics",
            JSONArray().also { array ->
                diagnostics.forEach { diagnostic ->
                    array.put(diagnostic.toBenchmarkJsonObject())
                }
            },
        )

private fun VesperPluginMeasurement.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("name", name)
        .put("value", value)
        .put("unit", unit)
        .put("attributes", attributes.toBenchmarkAttributesJsonObject())

private fun VesperBenchmarkThresholdViolation.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("measurement", measurement)
        .put("actual", actual)
        .put("threshold", threshold)
        .put("comparison", comparison)

private fun VesperPluginDiagnostic.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("code", code)
        .put("severity", severity.rawValue)
        .put("message", message)
        .put("attributes", attributes.toBenchmarkAttributesJsonObject())

private fun Map<String, String>.toBenchmarkAttributesJsonObject(): JSONObject =
    JSONObject().also { payload ->
        toSortedMap().forEach { (key, value) -> payload.put(key, value) }
    }

internal fun logBenchmarkJson(json: String) {
    if (json.length <= BENCHMARK_LOG_CHUNK_SIZE) {
        Log.i(BENCHMARK_LOG_TAG, json)
        return
    }

    var offset = 0
    while (offset < json.length) {
        val end = (offset + BENCHMARK_LOG_CHUNK_SIZE).coerceAtMost(json.length)
        Log.i(BENCHMARK_LOG_TAG, json.substring(offset, end))
        offset = end
    }
}
