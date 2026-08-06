package io.github.ikaros.vesper.player.android

import org.json.JSONArray
import org.json.JSONObject

/** One measurement returned by a playback pipeline event hook. */
data class VesperPipelineEventHookMeasurement(
    val name: String,
    val value: Double,
    val unit: String,
    val attributes: Map<String, String> = emptyMap(),
)

/** One structured diagnostic returned by a playback pipeline event hook. */
data class VesperPipelineEventHookDiagnostic(
    val code: String,
    val severity: String,
    val message: String,
    val attributes: Map<String, String> = emptyMap(),
)

/** Successful or rejected outcome returned by a playback pipeline event hook. */
data class VesperPipelineEventHookOutcome(
    val accepted: Boolean,
    val measurements: List<VesperPipelineEventHookMeasurement> = emptyList(),
    val diagnostics: List<VesperPipelineEventHookDiagnostic> = emptyList(),
)

/** Error returned by a playback pipeline event hook. */
data class VesperPipelineEventHookError(
    val code: String,
    val message: String,
)

/** One report emitted after a playback pipeline event was delivered to a hook. */
data class VesperPipelineEventHookReport(
    val pluginId: String,
    val capabilityInstanceId: String?,
    /** The raw transport value is preserved so newer transports remain observable. */
    val transport: String,
    val runId: String,
    val sessionId: String,
    val eventName: String,
    val status: String,
    val outcome: VesperPipelineEventHookOutcome? = null,
    val error: VesperPipelineEventHookError? = null,
)

/** A bounded drain from the playback pipeline event-hook report queue. */
data class VesperPipelineEventHookReportBatch(
    val reports: List<VesperPipelineEventHookReport> = emptyList(),
    val droppedEvents: Long = 0L,
    val droppedReports: Long = 0L,
    val dispatcherError: String? = null,
)

private const val MAX_PIPELINE_EVENT_HOOK_REPORTS = 1_024
private const val MAX_PIPELINE_EVENT_HOOK_MEASUREMENTS = 128
private const val MAX_PIPELINE_EVENT_HOOK_DIAGNOSTICS = 64
private const val MAX_PIPELINE_EVENT_HOOK_ATTRIBUTES = 32
private const val MAX_PIPELINE_EVENT_HOOK_ATTRIBUTE_KEY_BYTES = 64
private const val MAX_PIPELINE_EVENT_HOOK_ATTRIBUTE_VALUE_BYTES = 256
private const val MAX_PIPELINE_EVENT_HOOK_MESSAGE_BYTES = 256
private const val MAX_PIPELINE_EVENT_HOOK_FIELD_BYTES = 64

/**
 * Decodes the bounded native report envelope. Malformed native data is turned
 * into a host diagnostic rather than escaping as an unchecked JSON exception.
 */
internal fun parsePipelineEventHookReportsJson(
    json: String?,
): VesperPipelineEventHookReportBatch {
    if (json.isNullOrBlank()) {
        return VesperPipelineEventHookReportBatch()
    }
    return runCatching {
        val payload = JSONObject(json)
        val reportsJson =
            payload.optNullableArrayStrict("reports")
                ?: throw IllegalArgumentException(
                    "native report payload did not contain a reports array",
                )
        require(reportsJson.length() <= MAX_PIPELINE_EVENT_HOOK_REPORTS) {
            "native report batch exceeds $MAX_PIPELINE_EVENT_HOOK_REPORTS reports"
        }
        VesperPipelineEventHookReportBatch(
            reports = List(reportsJson.length()) { index ->
                parsePipelineEventHookReport(reportsJson.getJSONObject(index))
            },
            droppedEvents = decodePipelineEventHookCounter(payload, "droppedEvents"),
            droppedReports = decodePipelineEventHookCounter(payload, "droppedReports"),
            dispatcherError = payload.optNullableStringStrict("dispatcherError"),
        )
    }.getOrElse { error ->
        VesperPipelineEventHookReportBatch(
            dispatcherError =
                "invalid native pipeline event-hook report payload: " +
                    (error.message ?: "unknown decode error"),
        )
    }
}

private fun parsePipelineEventHookReport(value: JSONObject): VesperPipelineEventHookReport {
    val result = value.optNullableObjectStrict("result")
        ?: throw IllegalArgumentException("native event-hook report result was not an object")
    val status = requirePipelineEventHookString(result, "status")
    val outcome = result.optNullableObjectStrict("outcome")?.let(::parsePipelineEventHookOutcome)
    val error = result.optNullableObjectStrict("error")?.let { errorJson ->
        VesperPipelineEventHookError(
            code = requirePipelineEventHookString(errorJson, "code"),
            message = requirePipelineEventHookString(
                errorJson,
                "message",
                maxBytes = MAX_PIPELINE_EVENT_HOOK_MESSAGE_BYTES,
            ),
        )
    }
    return VesperPipelineEventHookReport(
        pluginId = requirePipelineEventHookString(value, "pluginId"),
        capabilityInstanceId = value.optNullableStringStrict("capabilityInstanceId"),
        transport = requirePipelineEventHookString(value, "transport"),
        runId = requirePipelineEventHookString(value, "runId"),
        sessionId = requirePipelineEventHookString(value, "sessionId"),
        eventName = requirePipelineEventHookString(value, "eventName"),
        status = status,
        outcome = outcome,
        error = error,
    )
}

private fun parsePipelineEventHookOutcome(value: JSONObject): VesperPipelineEventHookOutcome {
    val measurementsJson = value.optNullableArrayStrict("measurements") ?: JSONArray()
    val diagnosticsJson = value.optNullableArrayStrict("diagnostics") ?: JSONArray()
    require(measurementsJson.length() <= MAX_PIPELINE_EVENT_HOOK_MEASUREMENTS) {
        "native event-hook outcome exceeds $MAX_PIPELINE_EVENT_HOOK_MEASUREMENTS measurements"
    }
    require(diagnosticsJson.length() <= MAX_PIPELINE_EVENT_HOOK_DIAGNOSTICS) {
        "native event-hook outcome exceeds $MAX_PIPELINE_EVENT_HOOK_DIAGNOSTICS diagnostics"
    }
    require(value.has("accepted") && value.opt("accepted") is Boolean) {
        "native event-hook outcome accepted field was not a boolean"
    }
    return VesperPipelineEventHookOutcome(
        accepted = value.optBoolean("accepted", false),
        measurements = List(measurementsJson.length()) { index ->
            val measurement = measurementsJson.getJSONObject(index)
            val rawMeasurementValue = measurement.opt("value")
            require(rawMeasurementValue is Number) {
                "native event-hook measurement value must be numeric"
            }
            val measurementValue = rawMeasurementValue.toDouble()
            require(measurementValue.isFinite()) {
                "native event-hook measurement value must be finite"
            }
            VesperPipelineEventHookMeasurement(
                name = requirePipelineEventHookString(
                    measurement,
                    "name",
                    maxBytes = MAX_PIPELINE_EVENT_HOOK_FIELD_BYTES,
                ),
                value = measurementValue,
                unit = requirePipelineEventHookString(
                    measurement,
                    "unit",
                    maxBytes = MAX_PIPELINE_EVENT_HOOK_FIELD_BYTES,
                ),
                attributes = parseStringAttributes(
                    measurement.optNullableObjectStrict("attributes"),
                ),
            )
        },
        diagnostics = List(diagnosticsJson.length()) { index ->
            val diagnostic = diagnosticsJson.getJSONObject(index)
            VesperPipelineEventHookDiagnostic(
                code = requirePipelineEventHookString(
                    diagnostic,
                    "code",
                    maxBytes = MAX_PIPELINE_EVENT_HOOK_FIELD_BYTES,
                ),
                severity = requirePipelineEventHookString(diagnostic, "severity"),
                message = requirePipelineEventHookString(
                    diagnostic,
                    "message",
                    maxBytes = MAX_PIPELINE_EVENT_HOOK_MESSAGE_BYTES,
                ),
                attributes = parseStringAttributes(
                    diagnostic.optNullableObjectStrict("attributes"),
                ),
            )
        },
    )
}

private fun parseStringAttributes(value: JSONObject?): Map<String, String> {
    if (value == null) {
        return emptyMap()
    }
    val keys = value.keys()
    val result = linkedMapOf<String, String>()
    while (keys.hasNext()) {
        if (result.size >= MAX_PIPELINE_EVENT_HOOK_ATTRIBUTES) {
            throw IllegalArgumentException(
                "native event-hook attributes exceed $MAX_PIPELINE_EVENT_HOOK_ATTRIBUTES entries",
            )
        }
        val key = keys.next()
        require(key.isNotEmpty()) { "native event-hook attribute key must not be empty" }
        require(key.toByteArray(Charsets.UTF_8).size <= MAX_PIPELINE_EVENT_HOOK_ATTRIBUTE_KEY_BYTES) {
            "native event-hook attribute key exceeds $MAX_PIPELINE_EVENT_HOOK_ATTRIBUTE_KEY_BYTES bytes"
        }
        val rawValue = value.opt(key)
        require(rawValue is String && rawValue.isNotEmpty()) {
            "native event-hook attribute value must be a non-empty string"
        }
        require(rawValue.toByteArray(Charsets.UTF_8).size <= MAX_PIPELINE_EVENT_HOOK_ATTRIBUTE_VALUE_BYTES) {
            "native event-hook attribute value exceeds $MAX_PIPELINE_EVENT_HOOK_ATTRIBUTE_VALUE_BYTES bytes"
        }
        result[key] = rawValue
    }
    return result
}

private fun requirePipelineEventHookString(
    value: JSONObject,
    key: String,
    maxBytes: Int? = null,
): String {
    val raw = value.opt(key)
    require(raw is String && raw.isNotEmpty()) {
        "native event-hook $key was missing or empty"
    }
    if (maxBytes != null) {
        require(raw.toByteArray(Charsets.UTF_8).size <= maxBytes) {
            "native event-hook $key exceeds $maxBytes bytes"
        }
    }
    return raw
}

private fun JSONObject.optNullableStringStrict(key: String): String? {
    if (isNull(key) || !has(key)) {
        return null
    }
    val value = opt(key)
    require(value is String) { "native event-hook $key was not a string" }
    return value
}

private fun JSONObject.optNullableObjectStrict(key: String): JSONObject? {
    if (isNull(key) || !has(key)) {
        return null
    }
    val value = opt(key)
    require(value is JSONObject) { "native event-hook $key was not an object" }
    return value
}

private fun JSONObject.optNullableArrayStrict(key: String): JSONArray? {
    if (isNull(key) || !has(key)) {
        return null
    }
    val value = opt(key)
    require(value is JSONArray) { "native event-hook $key was not an array" }
    return value
}

private fun decodePipelineEventHookCounter(payload: JSONObject, key: String): Long {
    if (isNullOrMissing(payload, key)) {
        return 0L
    }
    val raw = payload.opt(key)
    if (raw is Int || raw is Long || raw is Short || raw is Byte) {
        val value = (raw as Number).toLong()
        require(value >= 0L) {
            "native event-hook $key was not a non-negative integer"
        }
        return value
    }
    val number = raw as? Number
        ?: throw IllegalArgumentException(
            "native event-hook $key was not a non-negative integer",
        )
    val doubleValue = number.toDouble()
    require(
        doubleValue.isFinite() &&
            doubleValue >= 0.0 &&
            doubleValue == kotlin.math.floor(doubleValue) &&
            doubleValue < Long.MAX_VALUE.toDouble(),
    ) {
        "native event-hook $key was not a non-negative integer"
    }
    return number.toLong()
}

private fun isNullOrMissing(payload: JSONObject, key: String): Boolean =
    !payload.has(key) || payload.isNull(key)
