package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperPipelineEventHookDiagnostic
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookMeasurement
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookOutcome
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookReport
import io.github.ikaros.vesper.player.android.VesperPipelineEventHookReportBatch

internal fun VesperPipelineEventHookReportBatch.toPipelineEventHookReportMap(): Map<String, Any?> =
    mapOf(
        "reports" to reports.map { report -> report.toMap() },
        "droppedEvents" to droppedEvents,
        "droppedReports" to droppedReports,
        "dispatcherError" to dispatcherError,
    )

private fun VesperPipelineEventHookReport.toMap(): Map<String, Any?> =
    mapOf(
        "pluginId" to pluginId,
        "capabilityInstanceId" to capabilityInstanceId,
        "transport" to transport,
        "runId" to runId,
        "sessionId" to sessionId,
        "eventName" to eventName,
        "result" to mapOf(
            "status" to status,
            "outcome" to outcome?.toMap(),
            "error" to error?.let { error ->
                mapOf(
                    "code" to error.code,
                    "message" to error.message,
                )
            },
        ),
    )

private fun VesperPipelineEventHookOutcome.toMap(): Map<String, Any?> =
    mapOf(
        "accepted" to accepted,
        "measurements" to measurements.map(VesperPipelineEventHookMeasurement::toMap),
        "diagnostics" to diagnostics.map(VesperPipelineEventHookDiagnostic::toMap),
    )

private fun VesperPipelineEventHookMeasurement.toMap(): Map<String, Any?> =
    mapOf(
        "name" to name,
        "value" to value,
        "unit" to unit,
        "attributes" to attributes,
    )

private fun VesperPipelineEventHookDiagnostic.toMap(): Map<String, Any?> =
    mapOf(
        "code" to code,
        "severity" to severity,
        "message" to message,
        "attributes" to attributes,
    )
