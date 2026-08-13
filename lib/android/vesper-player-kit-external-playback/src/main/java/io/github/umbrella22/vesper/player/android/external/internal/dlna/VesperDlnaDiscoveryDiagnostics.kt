package io.github.umbrella22.vesper.player.android.external.internal.dlna

import android.util.Log
internal fun VesperDlnaDiscovery.emitDiagnostic(
    code: String,
    severity: VesperDlnaDiscoveryDiagnosticSeverity,
    message: String,
    details: Map<String, String> = emptyMap(),
) {
    if (!running.get()) {
        return
    }
    val filteredDetails = details.filterValues { it.isNotBlank() }
    logDiagnostic(code, severity, message, filteredDetails)
    listener.onDiscoveryDiagnostic(
        VesperDlnaDiscoveryDiagnostic(
            code = code,
            severity = severity,
            message = message,
            details = filteredDetails,
        ),
    )
}

internal fun VesperDlnaDiscovery.logDiagnostic(
    code: String,
    severity: VesperDlnaDiscoveryDiagnosticSeverity,
    message: String,
    details: Map<String, String>,
) {
    val detailText = details.entries.joinToString(", ") { (key, value) -> "$key=$value" }
    val logMessage = if (detailText.isBlank()) {
        "[$code] $message"
    } else {
        "[$code] $message | $detailText"
    }
    runCatching {
        when (severity) {
            VesperDlnaDiscoveryDiagnosticSeverity.Info -> Log.d(LOG_TAG, logMessage)
            VesperDlnaDiscoveryDiagnosticSeverity.Warning -> Log.w(LOG_TAG, logMessage)
            VesperDlnaDiscoveryDiagnosticSeverity.Error -> Log.e(LOG_TAG, logMessage)
        }
    }
}
