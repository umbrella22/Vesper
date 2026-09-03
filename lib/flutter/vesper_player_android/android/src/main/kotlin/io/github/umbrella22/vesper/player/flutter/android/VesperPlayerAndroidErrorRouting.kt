package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperPerformanceDiagnosticsException

private val OBSOLETE_SUBTITLE_SELECTION_ERROR_CODES =
    setOf(
        "subtitle_selection_cancelled",
        "subtitle_source_changed",
        "subtitle_selection_superseded",
    )

internal fun routeAsyncSessionCommandFailure(
    error: Throwable,
    isCurrentSession: Boolean,
    publishPlayerError: (Map<String, Any?>) -> Unit,
    returnMethodError: (String, String?, Map<String, Any?>) -> Unit,
) {
    if (error is VesperPerformanceDiagnosticsException) {
        returnMethodError(
            "vesper_performance_diagnostics",
            error.message,
            mapOf("performanceDiagnosticsCode" to error.code.rawValue),
        )
        return
    }
    val methodErrorMap = error.toErrorMap()
    val isObsoleteSubtitleSelectionFailure =
        methodErrorMap["domain"] == "subtitle" &&
            methodErrorMap["code"] in OBSOLETE_SUBTITLE_SELECTION_ERROR_CODES
    val commandDetails = methodErrorMap["details"] as? Map<*, *>
    val isObsoleteCommandFailure = commandDetails?.get("obsolete") == true
    if (isCurrentSession && !isObsoleteSubtitleSelectionFailure && !isObsoleteCommandFailure) {
        publishPlayerError(methodErrorMap.toEventErrorMap())
    }
    val flutterErrorCode =
        if (methodErrorMap["domain"] == "subtitle") {
            "vesper_subtitle_error"
        } else {
            "vesper_operation_failed"
        }
    returnMethodError(flutterErrorCode, error.message, methodErrorMap)
}
