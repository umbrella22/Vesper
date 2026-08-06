package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperPipelineEventHookDiagnostic
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookError
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookMeasurement
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookOutcome
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookReport
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookReportBatch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlayerAndroidPipelineEventHookMappingTest {
    @Test
    fun mapPreservesTypedOutcomeAndFutureWireValues() {
        val batch =
            VesperPipelineEventHookReportBatch(
                reports =
                    listOf(
                        VesperPipelineEventHookReport(
                            pluginId = "dev.vesper.hook",
                            capabilityInstanceId = "dev.vesper.hook.playback",
                            transport = "future-transport",
                            runId = "run-1",
                            sessionId = "session-1",
                            eventName = "playback.ready",
                            status = "future-status",
                            outcome =
                                VesperPipelineEventHookOutcome(
                                    accepted = true,
                                    measurements =
                                        listOf(
                                            VesperPipelineEventHookMeasurement(
                                                name = "latency",
                                                value = 2.5,
                                                unit = "ms",
                                                attributes = mapOf("stage" to "open"),
                                            ),
                                        ),
                                    diagnostics =
                                        listOf(
                                            VesperPipelineEventHookDiagnostic(
                                                code = "future.code",
                                                severity = "future-severity",
                                                message = "ready",
                                                attributes = mapOf("scope" to "test"),
                                            ),
                                        ),
                                ),
                        ),
                    ),
                droppedEvents = 3L,
                droppedReports = 2L,
            )

        val payload = batch.toPipelineEventHookReportMap()
        assertEquals(3L, payload["droppedEvents"])
        assertEquals(2L, payload["droppedReports"])
        assertNull(payload["dispatcherError"])

        val report = (payload["reports"] as List<*>).single() as Map<*, *>
        assertEquals("future-transport", report["transport"])
        assertEquals("future-status", (report["result"] as Map<*, *>)["status"])
        val outcome = (report["result"] as Map<*, *>)["outcome"] as Map<*, *>
        assertEquals(true, outcome["accepted"])
        val measurement = (outcome["measurements"] as List<*>).single() as Map<*, *>
        assertEquals(2.5, measurement["value"])
        assertEquals(mapOf("stage" to "open"), measurement["attributes"])
        val diagnostic = (outcome["diagnostics"] as List<*>).single() as Map<*, *>
        assertEquals("future-severity", diagnostic["severity"])
        assertEquals(mapOf("scope" to "test"), diagnostic["attributes"])
    }

    @Test
    fun mapPreservesErrorAndNullOutcome() {
        val report =
            VesperPipelineEventHookReport(
                pluginId = "dev.vesper.hook",
                capabilityInstanceId = null,
                transport = "native",
                runId = "run-2",
                sessionId = "session-2",
                eventName = "playback.failed",
                status = "error",
                error =
                    VesperPipelineEventHookError(
                        code = "future-error",
                        message = "failed",
                    ),
            )

        val result =
            ((VesperPipelineEventHookReportBatch(reports = listOf(report))
                .toPipelineEventHookReportMap()["reports"] as List<*>)
                .single() as Map<*, *>)["result"] as Map<*, *>

        assertEquals("error", result["status"])
        assertTrue(result["outcome"] == null)
        assertEquals(
            mapOf("code" to "future-error", "message" to "failed"),
            result["error"],
        )
    }
}
