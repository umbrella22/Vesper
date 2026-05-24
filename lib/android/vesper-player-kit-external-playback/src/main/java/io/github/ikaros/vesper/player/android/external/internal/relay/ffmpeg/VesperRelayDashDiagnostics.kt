package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import java.security.MessageDigest
import java.util.Locale

internal fun VesperRelayFormatAdaptationRequest.hostInputBaseDetails(): Map<String, String> =
    mapOf(
        "sessionId" to sessionId,
        "fallbackFormat" to fallbackFormat.name,
        "resourcePath" to resourcePath,
        "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
        "sourceUriHash" to hashForDiagnostic(source.uri),
        "sourceKind" to source.kind.name,
        "sourceProtocol" to source.protocol.name,
        "uriScheme" to source.uri.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US),
    ) + listOfNotNull(
        routeId?.let { "routeId" to it },
        routeName?.let { "routeName" to it },
    ).toMap()

internal fun resolverDetails(
    request: VesperRelayFormatAdaptationRequest,
    resolver: VesperRelayDashResourceResolver,
): Map<String, String> =
    request.hostInputBaseDetails() + mapOf(
        "sourceOrigin" to resolver.origin.kind,
        "sourceKind" to request.source.kind.name,
        "sourceProtocol" to request.source.protocol.name,
        "uriScheme" to request.source.uri.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US),
    )

internal fun Map<String, String>.withSegmentHash(uri: String): Map<String, String> =
    this + mapOf("segmentUriHash" to hashForDiagnostic(uri))

internal fun Map<String, String>.withHostError(message: String?): Map<String, String> =
    this + listOfNotNull(message?.takeIf { it.isNotBlank() }?.let { "hostError" to it }).toMap()

internal fun hashForDiagnostic(value: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(value.toByteArray(Charsets.UTF_8))
    return digest.take(8).joinToString("") { byte -> "%02x".format(byte) }
}
