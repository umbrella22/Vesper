package io.github.ikaros.vesper.player.android.relay

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol

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
)

data class VesperExternalSourcePreparationRequest(
    val target: VesperExternalPlaybackTarget,
    val sources: List<VesperPlayerSource>,
    val proxyPolicy: VesperExternalProxyPolicy = VesperExternalProxyPolicy.Auto,
    val capabilities: VesperExternalRouteCapabilities = VesperExternalRouteCapabilities(),
)

sealed class VesperExternalSourcePreparationResult {
    data class Prepared(
        val source: VesperPlayerSource,
        val relayToken: String? = null,
        val relayEnabled: Boolean = false,
    ) : VesperExternalSourcePreparationResult()

    data class Unsupported(val message: String) : VesperExternalSourcePreparationResult()
}

class VesperExternalPlaybackSourcePreparer(
    private val relayServer: VesperRelayServer,
) {
    fun prepare(request: VesperExternalSourcePreparationRequest): VesperExternalSourcePreparationResult {
        val ordered = request.sources.sortedWith(sourceComparator(request.target))
        val unsupportedReasons = mutableListOf<String>()

        for (source in ordered) {
            val protocolReason = source.unsupportedProtocolReason(request)
            if (protocolReason != null) {
                unsupportedReasons += protocolReason
                continue
            }

            val requiresRelay = request.proxyPolicy == VesperExternalProxyPolicy.Always ||
                source.headers.isNotEmpty() ||
                source.kind == VesperPlayerSourceKind.Local ||
                source.protocol == VesperPlayerSourceProtocol.File ||
                source.protocol == VesperPlayerSourceProtocol.Content

            if (requiresRelay && request.proxyPolicy == VesperExternalProxyPolicy.Never) {
                unsupportedReasons +=
                    "${source.label} requires relay because it is local or carries request headers."
                continue
            }

            if (!requiresRelay) {
                return VesperExternalSourcePreparationResult.Prepared(source = source)
            }

            if (source.protocol == VesperPlayerSourceProtocol.Dash) {
                unsupportedReasons += "DASH relay manifest rewrite is not supported in this MVP."
                continue
            }

            val handle = relayServer.register(source)
            val relayed = VesperPlayerSource.remote(
                uri = handle.url,
                label = source.label,
                protocol = source.protocol.relaxedForRelay(),
                headers = emptyMap(),
            )
            return VesperExternalSourcePreparationResult.Prepared(
                source = relayed,
                relayToken = handle.token,
                relayEnabled = true,
            )
        }

        return VesperExternalSourcePreparationResult.Unsupported(
            unsupportedReasons.firstOrNull() ?: "No playable external playback source is available.",
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
): String? {
    val capabilities = request.capabilities
    return when (protocol) {
        VesperPlayerSourceProtocol.Progressive,
        VesperPlayerSourceProtocol.Unknown,
        -> if (capabilities.supportsProgressive) null else "Progressive media is not supported by this route."
        VesperPlayerSourceProtocol.Hls ->
            if (capabilities.supportsHls) null else "HLS is not supported by this route."
        VesperPlayerSourceProtocol.Dash ->
            if (request.target == VesperExternalPlaybackTarget.Dlna) {
                "DASH is not supported for DLNA in this MVP."
            } else if (capabilities.supportsDash) {
                null
            } else {
                "DASH is not supported by this route."
            }
        VesperPlayerSourceProtocol.File,
        VesperPlayerSourceProtocol.Content,
        -> if (capabilities.supportsProgressive) null else "Local media relay is not supported by this route."
    }
}

private fun VesperPlayerSourceProtocol.relaxedForRelay(): VesperPlayerSourceProtocol =
    when (this) {
        VesperPlayerSourceProtocol.File,
        VesperPlayerSourceProtocol.Content,
        VesperPlayerSourceProtocol.Unknown,
        -> VesperPlayerSourceProtocol.Progressive
        else -> this
    }
