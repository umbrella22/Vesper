package io.github.umbrella22.vesper.player.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPipelineEventHookTest {
    @Test
    fun parserPreservesOutcomeDiagnosticsAndUnknownTransport() {
        val batch =
            parsePipelineEventHookReportsJson(
                """
                {
                  "reports": [{
                    "pluginId": "dev.vesper.hook",
                    "capabilityInstanceId": "dev.vesper.hook.playback",
                    "transport": "future-transport",
                    "runId": "run-1",
                    "sessionId": "session-1",
                    "eventName": "playback.first_frame",
                    "result": {
                      "status": "accepted",
                      "outcome": {
                        "accepted": true,
                        "measurements": [{
                          "name": "latency",
                          "value": 2.5,
                          "unit": "ms",
                          "attributes": {"source": "test"}
                        }],
                        "diagnostics": [{
                          "code": "observed",
                          "severity": "info",
                          "message": "frame observed",
                          "attributes": {}
                        }]
                      }
                    }
                  }],
                  "droppedEvents": 3,
                  "droppedReports": 2
                }
                """.trimIndent(),
            )

        assertEquals(1, batch.reports.size)
        assertEquals("future-transport", batch.reports.single().transport)
        assertEquals("accepted", batch.reports.single().status)
        assertEquals(2.5, batch.reports.single().outcome?.measurements?.single()?.value ?: 0.0, 0.0)
        assertEquals("info", batch.reports.single().outcome?.diagnostics?.single()?.severity)
        assertEquals(3L, batch.droppedEvents)
        assertEquals(2L, batch.droppedReports)
        assertNull(batch.dispatcherError)
    }

    @Test
    fun parserTurnsMalformedPayloadIntoBoundedDiagnostic() {
        val batch = parsePipelineEventHookReportsJson("{\"reports\": [null]}")

        assertTrue(batch.reports.isEmpty())
        assertNotNull(batch.dispatcherError)
        assertTrue(batch.dispatcherError.orEmpty().startsWith("invalid native"))
    }

    @Test
    fun parserRejectsNegativeCountersInsteadOfCoercingThem() {
        val batch =
            parsePipelineEventHookReportsJson(
                """{"reports": [], "droppedEvents": -1}""",
            )

        assertTrue(batch.reports.isEmpty())
        assertTrue(batch.dispatcherError.orEmpty().contains("non-negative integer"))
    }

    @Test
    fun parserRejectsProtocolCollectionAndUtf8TextOverflows() {
        val outcomes =
            listOf(
                JSONObject()
                    .put("accepted", true)
                    .put(
                        "measurements",
                        JSONArray().apply {
                            repeat(129) {
                                put(validMeasurement())
                            }
                        },
                    ),
                JSONObject()
                    .put("accepted", true)
                    .put(
                        "diagnostics",
                        JSONArray().apply {
                            repeat(65) {
                                put(validDiagnostic())
                            }
                        },
                    ),
                JSONObject()
                    .put("accepted", true)
                    .put(
                        "measurements",
                        JSONArray().put(
                            validMeasurement().put(
                                "attributes",
                                JSONObject().apply {
                                    repeat(33) { index ->
                                        put("key-$index", "value")
                                    }
                                },
                            ),
                        ),
                    ),
                JSONObject()
                    .put("accepted", true)
                    .put(
                        "diagnostics",
                        JSONArray().put(
                            validDiagnostic().put("message", "é".repeat(129)),
                        ),
                    ),
            )

        outcomes.forEach { outcome ->
            val batch = parsePipelineEventHookReportsJson(reportPayload(outcome).toString())
            assertTrue(batch.reports.isEmpty())
            assertTrue(batch.dispatcherError.orEmpty().startsWith("invalid native"))
        }
    }

    private fun reportPayload(outcome: JSONObject): JSONObject =
        JSONObject().put(
            "reports",
            JSONArray().put(
                JSONObject()
                    .put("pluginId", "dev.vesper.hook")
                    .put("transport", "native")
                    .put("runId", "run")
                    .put("sessionId", "session")
                    .put("eventName", "event")
                    .put(
                        "result",
                        JSONObject()
                            .put("status", "accepted")
                            .put("outcome", outcome),
                    ),
            ),
        )

    private fun validMeasurement(): JSONObject =
        JSONObject()
            .put("name", "latency")
            .put("value", 1.0)
            .put("unit", "ms")

    private fun validDiagnostic(): JSONObject =
        JSONObject()
            .put("code", "diagnostic")
            .put("severity", "info")
            .put("message", "message")
}
