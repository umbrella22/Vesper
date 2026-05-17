package io.github.ikaros.vesper.player.android.external.internal.relay

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import java.net.InetAddress

enum class VesperExternalProxyPolicy {
    Auto,
    Always,
    Never,
}

enum class VesperExternalPlaybackTarget {
    Cast,
    Dlna,
}

data class VesperExternalRouteCapabilities(
    val supportsProgressive: Boolean = true,
    val supportsHls: Boolean = false,
    val supportsDash: Boolean = false,
    val supportsMpegTs: Boolean = true,
)

data class VesperExternalSourcePreparationRequest(
    val target: VesperExternalPlaybackTarget,
    val sources: List<VesperPlayerSource>,
    val proxyPolicy: VesperExternalProxyPolicy = VesperExternalProxyPolicy.Auto,
    val capabilities: VesperExternalRouteCapabilities = VesperExternalRouteCapabilities(),
    val formatAdaptation: VesperRelayFormatAdaptationConfig = VesperRelayFormatAdaptationConfig(),
    val routeId: String? = null,
    val routeName: String? = null,
    val routeLocalAddress: InetAddress? = null,
)

sealed class VesperExternalSourcePreparationResult {
    data class Prepared(
        val source: VesperPlayerSource,
        val relayToken: String? = null,
        val relayEnabled: Boolean = false,
        val adaptedFormat: VesperRelayFallbackFormat? = null,
    ) : VesperExternalSourcePreparationResult()

    data class Unsupported(
        val message: String,
        val code: String? = null,
        val details: Map<String, String> = emptyMap(),
    ) : VesperExternalSourcePreparationResult()
}

class VesperExternalPlaybackSourcePreparer(
    private val relayServer: VesperRelayServer,
) {
    fun prepare(request: VesperExternalSourcePreparationRequest): VesperExternalSourcePreparationResult {
        val ordered = request.sources.sortedWith(sourceComparator(request.target))
        val unsupportedReasons = mutableListOf<UnsupportedSourceReason>()

        for (source in ordered) {
            val protocolReason = source.unsupportedProtocolReason(request)
            if (protocolReason != null) {
                unsupportedReasons += protocolReason
                continue
            }

            val fallbackFormat = source.adaptationFallbackFormat(request)
            val requiresRelay = request.proxyPolicy == VesperExternalProxyPolicy.Always ||
                fallbackFormat != null ||
                source.headers.isNotEmpty() ||
                source.kind == VesperPlayerSourceKind.Local ||
                source.protocol == VesperPlayerSourceProtocol.File ||
                source.protocol == VesperPlayerSourceProtocol.Content

            if (requiresRelay && request.proxyPolicy == VesperExternalProxyPolicy.Never) {
                unsupportedReasons += UnsupportedSourceReason(
                    message = "${source.label} requires relay because it is local or carries request headers.",
                    code = "relay_required_by_source",
                    details = source.relayRequirementDetails(request),
                )
                continue
            }

            if (!requiresRelay) {
                return VesperExternalSourcePreparationResult.Prepared(source = source)
            }

            if (source.protocol == VesperPlayerSourceProtocol.Dash && fallbackFormat == null) {
                unsupportedReasons += UnsupportedSourceReason(
                    message = "DASH relay manifest rewrite is not supported in this MVP.",
                    code = "unsupported_dash_relay",
                    details = source.dashRelaySourceDetails(request),
                )
                continue
            }

            val adaptation = fallbackFormat?.let {
                VesperRelayFormatAdaptationRegistration(
                    fallbackFormat = it,
                    config = request.formatAdaptation,
                    routeId = request.routeId,
                    routeName = request.routeName,
                )
            }
            val handle =
                try {
                    relayServer.register(source, adaptation, request.routeLocalAddress)
                } catch (error: VesperRelayRegistrationException) {
                    unsupportedReasons += UnsupportedSourceReason(
                        message = error.diagnostic.message,
                        code = error.diagnostic.code,
                        details = error.diagnostic.details + source.dashRelaySourceDetails(request),
                    )
                    continue
                }
            val relayed = VesperPlayerSource.remote(
                uri = handle.url,
                label = source.label,
                protocol = fallbackFormat?.relayedProtocol() ?: source.protocol.relaxedForRelay(),
                headers = emptyMap(),
            )
            return VesperExternalSourcePreparationResult.Prepared(
                source = relayed,
                relayToken = handle.token,
                relayEnabled = true,
                adaptedFormat = fallbackFormat,
            )
        }

        val unsupported = unsupportedReasons.firstOrNull()
        val message = unsupported?.message
            ?: "No playable external playback source is available."
        return VesperExternalSourcePreparationResult.Unsupported(
            message = message,
            code = unsupported?.code,
            details = unsupported?.details.orEmpty(),
        )
    }

    private fun sourceComparator(
        target: VesperExternalPlaybackTarget,
    ): Comparator<VesperPlayerSource> =
        compareBy { source ->
            when (target) {
                VesperExternalPlaybackTarget.Dlna ->
                    when (source.protocol) {
                        VesperPlayerSourceProtocol.Progressive -> 0
                        VesperPlayerSourceProtocol.Hls -> 1
                        VesperPlayerSourceProtocol.Dash -> 2
                        else -> 3
                    }
                VesperExternalPlaybackTarget.Cast ->
                    when (source.protocol) {
                        VesperPlayerSourceProtocol.Hls -> 0
                        VesperPlayerSourceProtocol.Dash -> 1
                        VesperPlayerSourceProtocol.Progressive -> 2
                        else -> 3
                    }
            }
        }
}

private fun VesperPlayerSource.unsupportedProtocolReason(
    request: VesperExternalSourcePreparationRequest,
): UnsupportedSourceReason? {
    val capabilities = request.capabilities
    return when (protocol) {
        VesperPlayerSourceProtocol.Progressive,
        VesperPlayerSourceProtocol.Unknown,
        -> if (capabilities.supportsProgressive) {
            null
        } else {
            UnsupportedSourceReason("Progressive media is not supported by this route.")
        }
        VesperPlayerSourceProtocol.Hls ->
            if (capabilities.supportsHls) null else UnsupportedSourceReason("HLS is not supported by this route.")
        VesperPlayerSourceProtocol.Dash ->
            if (request.target == VesperExternalPlaybackTarget.Dlna) {
                if (request.formatAdaptation.enabled && adaptationFallbackFormat(request) != null) {
                    null
                } else if (request.formatAdaptation.enabled) {
                    UnsupportedSourceReason(
                        message = "DASH format adaptation is enabled, but this DLNA route does not report HLS or MPEG-TS support.",
                        code = "unsupported_device_caps",
                        details = mapOf(
                            "supportsHls" to request.capabilities.supportsHls.toString(),
                            "supportsMpegTs" to request.capabilities.supportsMpegTs.toString(),
                        ) + request.routeDetails(),
                    )
                } else {
                    UnsupportedSourceReason("DASH is not supported for DLNA. Enable format adaptation to remux DASH for this route.")
                }
            } else if (capabilities.supportsDash) {
                null
            } else {
                UnsupportedSourceReason("DASH is not supported by this route.")
            }
        VesperPlayerSourceProtocol.File,
        VesperPlayerSourceProtocol.Content,
        -> if (capabilities.supportsProgressive) null else UnsupportedSourceReason("Local media relay is not supported by this route.")
    }
}

private data class UnsupportedSourceReason(
    val message: String,
    val code: String? = null,
    val details: Map<String, String> = emptyMap(),
)

private fun VesperPlayerSource.dashRelaySourceDetails(
    request: VesperExternalSourcePreparationRequest,
): Map<String, String> =
    mapOf(
        "sourceKind" to kind.name,
        "sourceProtocol" to protocol.name,
        "uriScheme" to uri.substringBefore(':', missingDelimiterValue = "").lowercase(),
    ) + request.routeDetails()

private fun VesperPlayerSource.relayRequirementDetails(
    request: VesperExternalSourcePreparationRequest,
): Map<String, String> =
    mapOf(
        "sourceKind" to kind.name,
        "sourceProtocol" to protocol.name,
        "hasHeaders" to headers.isNotEmpty().toString(),
        "proxyPolicy" to request.proxyPolicy.name,
    ) + request.routeDetails()

private fun VesperExternalSourcePreparationRequest.routeDetails(): Map<String, String> =
    listOfNotNull(
        routeId?.let { "routeId" to it },
        routeName?.let { "routeName" to it },
    ).toMap()

private fun VesperPlayerSource.adaptationFallbackFormat(
    request: VesperExternalSourcePreparationRequest,
): VesperRelayFallbackFormat? {
    if (!request.formatAdaptation.enabled ||
        request.target != VesperExternalPlaybackTarget.Dlna ||
        protocol != VesperPlayerSourceProtocol.Dash
    ) {
        return null
    }
    val capabilities = request.capabilities
    val preferred = request.formatAdaptation.preferredFallback
    val candidates = buildList {
        add(preferred)
        if (request.formatAdaptation.allowHls) {
            add(VesperRelayFallbackFormat.Hls)
        }
        add(VesperRelayFallbackFormat.MpegTs)
    }.distinct()

    return candidates.firstOrNull { candidate ->
        when (candidate) {
            VesperRelayFallbackFormat.Hls -> request.formatAdaptation.allowHls && capabilities.supportsHls
            VesperRelayFallbackFormat.MpegTs -> capabilities.supportsMpegTs
        }
    }
}

private fun VesperRelayFallbackFormat.relayedProtocol(): VesperPlayerSourceProtocol =
    when (this) {
        VesperRelayFallbackFormat.Hls -> VesperPlayerSourceProtocol.Hls
        VesperRelayFallbackFormat.MpegTs -> VesperPlayerSourceProtocol.Progressive
    }

private fun VesperPlayerSourceProtocol.relaxedForRelay(): VesperPlayerSourceProtocol =
    when (this) {
        VesperPlayerSourceProtocol.File,
        VesperPlayerSourceProtocol.Content,
        VesperPlayerSourceProtocol.Unknown,
        -> VesperPlayerSourceProtocol.Progressive
        else -> this
    }
