package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperBenchmarkSinkReport
import io.github.ikaros.vesper.player.android.VesperBenchmarkSummary
import io.github.ikaros.vesper.player.android.VesperBenchmarkThresholdViolation
import io.github.ikaros.vesper.player.android.VesperPluginDiagnostic
import io.github.ikaros.vesper.player.android.VesperPluginDiagnosticSeverity
import io.github.ikaros.vesper.player.android.VesperPluginMeasurement
import org.junit.Assert.assertEquals
import org.junit.Test

class VesperPlayerAndroidBenchmarkMappingTest {
    @Test
    fun finalReportPreservesStructuredPayloadAndUnknownSeverity() {
        val summary =
            VesperBenchmarkSummary(
                runId = "run-1",
                sessionId = "session-1",
                acceptedEvents = 4,
                droppedEvents = 1,
                pluginAcceptedEvents = 1,
                pluginDroppedEvents = 0,
                metrics = emptyList(),
                pluginFinalReport =
                    VesperBenchmarkSinkReport(
                        acceptedEvents = 3,
                        droppedEvents = 2,
                        measurements =
                            listOf(
                                VesperPluginMeasurement(
                                    name = "startup",
                                    value = 12.5,
                                    unit = "ms",
                                    attributes = mapOf("route" to "native"),
                                ),
                            ),
                        thresholdViolations =
                            listOf(
                                VesperBenchmarkThresholdViolation(
                                    measurement = "startup",
                                    actual = 12.5,
                                    threshold = 10.0,
                                    comparison = "greaterThan",
                                ),
                            ),
                        diagnostics =
                            listOf(
                                VesperPluginDiagnostic(
                                    code = "benchmark.future",
                                    severity =
                                        VesperPluginDiagnosticSeverity("critical-future"),
                                    message = "future severity retained",
                                    attributes = mapOf("plugin" to "fixture"),
                                ),
                            ),
                    ),
                pluginErrors = listOf("transport warning"),
            )

        val payload = summary.toBenchmarkJsonObject()
        val finalReport = payload.getJSONObject("pluginFinalReport")

        assertEquals(1L, payload.getLong("pluginAcceptedEvents"))
        assertEquals(3L, finalReport.getLong("acceptedEvents"))
        assertEquals(
            "startup",
            finalReport.getJSONArray("measurements").getJSONObject(0).getString("name"),
        )
        assertEquals(
            10.0,
            finalReport
                .getJSONArray("thresholdViolations")
                .getJSONObject(0)
                .getDouble("threshold"),
            0.0,
        )
        assertEquals(
            "critical-future",
            finalReport
                .getJSONArray("diagnostics")
                .getJSONObject(0)
                .getString("severity"),
        )
        assertEquals(
            "fixture",
            finalReport
                .getJSONArray("diagnostics")
                .getJSONObject(0)
                .getJSONObject("attributes")
                .getString("plugin"),
        )
        assertEquals("transport warning", payload.getJSONArray("pluginErrors").getString(0))
    }
}
