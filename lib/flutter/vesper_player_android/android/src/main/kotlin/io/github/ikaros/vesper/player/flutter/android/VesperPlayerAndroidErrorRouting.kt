package io.github.ikaros.vesper.player.flutter.android

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
    val methodErrorMap = error.toErrorMap()
    val isObsoleteSubtitleSelectionFailure =
        methodErrorMap["domain"] == "subtitle" &&
            methodErrorMap["code"] in OBSOLETE_SUBTITLE_SELECTION_ERROR_CODES
    if (isCurrentSession && !isObsoleteSubtitleSelectionFailure) {
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
