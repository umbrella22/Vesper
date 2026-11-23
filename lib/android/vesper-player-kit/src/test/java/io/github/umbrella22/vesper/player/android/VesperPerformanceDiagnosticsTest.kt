package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPerformanceDiagnosticsTest {
    @Test
    fun retiredReportPluginDoesNotDisableLocalBenchmarkRecording() {
        val bridge = FakePlayerBridge(
            benchmarkConfiguration = VesperBenchmarkConfiguration(enabled = true),
        )

        val error = assertThrows(VesperPerformanceDiagnosticsException::class.java) {
            bridge.startPerformanceDiagnostics(
                VesperPerformanceDiagnosticsConfiguration(),
                VesperPerformanceProbe.FlutterFrameTiming,
                null,
            )
        }

        assertEquals(VesperPerformanceDiagnosticsErrorCode.ArtifactUnavailable, error.code)
        assertTrue(error.message.orEmpty().contains("retired"))
        bridge.play()
        assertTrue(bridge.drainBenchmarkEvents().isNotEmpty())
    }

    @Test
    fun staleDiagnosticsRunCannotReturnAReport() {
        val error = assertThrows(VesperPerformanceDiagnosticsException::class.java) {
            FakePlayerBridge().performanceDiagnosticsSnapshot("previous-run")
        }
        assertEquals(VesperPerformanceDiagnosticsErrorCode.ControllerDisposed, error.code)
    }
}
