package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic

internal fun unsupportedDashLayout(
    baseDetails: Map<String, String>,
    message: String,
    details: Map<String, String>,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_dash_layout",
            message = message,
            details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE) + details,
        ),
    )

internal fun unsupportedDynamicDash(baseDetails: Map<String, String>): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_dynamic_dash",
            message = "Dynamic DASH MPD is not supported by host-prepared relay remux v1.",
            details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
        ),
    )

internal fun unsupportedMixedDashOrigin(
    baseDetails: Map<String, String>,
    origin: VesperRelayDashSourceOrigin,
    resolvedUri: String,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_mixed_dash_origin",
            message = "DASH references must stay within the source origin for relay remux.",
            details = baseDetails + mapOf(
                "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
                "sourceOrigin" to origin.kind,
                "resourceUriHash" to hashForDiagnostic(resolvedUri),
            ),
        ),
    )
