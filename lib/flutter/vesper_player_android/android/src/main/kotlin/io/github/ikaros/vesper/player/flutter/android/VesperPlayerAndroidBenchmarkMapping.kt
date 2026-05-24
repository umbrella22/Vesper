package io.github.ikaros.vesper.player.flutter.android

import android.util.Log
import io.github.ikaros.vesper.player.android.VesperBenchmarkEvent
import io.github.ikaros.vesper.player.android.VesperBenchmarkMetricSummary
import io.github.ikaros.vesper.player.android.VesperBenchmarkSummary
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

internal fun VesperBenchmarkMetricSummary.toBenchmarkJsonObject(): JSONObject =
    JSONObject()
        .put("name", name)
        .put("count", count)
        .put("minNs", minNs)
        .put("maxNs", maxNs)
        .put("p50Ns", p50Ns)
        .put("p90Ns", p90Ns)
        .put("p95Ns", p95Ns)

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

