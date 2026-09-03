package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsErrorCode
import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsException
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class VesperPlayerAndroidPerformanceDiagnosticsMappingTest {
    @Test
    fun configurationRejectsFractionalAndOverflowingIntegers() {
        listOf<Any>(1.5, Long.MAX_VALUE).forEach { invalid ->
            val error = assertDiagnosticsError {
                mapOf<String, Any?>("maxRawEvents" to invalid)
                    .toPerformanceDiagnosticsConfiguration()
            }
            assertEquals(VesperPerformanceDiagnosticsErrorCode.InvalidConfiguration, error.code)
        }
    }

    @Test
    fun frameAndOverlayInputsRejectLossyIntegerConversions() {
        assertDiagnosticsError {
            mapOf<String, Any?>("loadNs" to 1.5, "budgetNs" to 2L)
                .toPerformanceFrameSample()
        }
        assertDiagnosticsError {
            mapOf<String, Any?>(
                "active" to true,
                "loadedBasicItemCount" to Long.MAX_VALUE,
            ).toPerformanceOverlayState()
        }
    }

    @Test
    fun markerMetadataRejectsWrongWireTypes() {
        assertDiagnosticsError {
            mapOf<String, Any?>("sequenceIndex" to 1.0)
                .optionalPerformanceSequenceIndex()
        }
        assertDiagnosticsError {
            mapOf<String, Any?>("expectedOverlayActive" to 1)
                .optionalPerformanceExpectedOverlayActive()
        }
    }

    private fun assertDiagnosticsError(
        operation: () -> Unit,
    ): VesperPerformanceDiagnosticsException {
        try {
            operation()
            fail("Expected VesperPerformanceDiagnosticsException")
        } catch (error: VesperPerformanceDiagnosticsException) {
            return error
        }
        error("unreachable")
    }
}
