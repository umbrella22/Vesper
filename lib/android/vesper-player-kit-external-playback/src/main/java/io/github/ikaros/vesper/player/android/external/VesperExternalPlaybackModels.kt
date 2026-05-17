package io.github.ikaros.vesper.player.android.external

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import io.github.ikaros.vesper.player.android.external.internal.relay.DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperExternalProxyPolicy as InternalProxyPolicy
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFallbackFormat as InternalFallbackFormat
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationConfig as InternalFormatAdaptationConfig

enum class VesperExternalPlaybackRouteKind {
    Cast,
    Dlna,
}

data class VesperExternalPlaybackRoute(
    val routeId: String,
    val name: String,
    val kind: VesperExternalPlaybackRouteKind,
    val manufacturer: String? = null,
    val modelName: String? = null,
    val active: Boolean = false,
    val available: Boolean = true,
)

enum class VesperExternalProxyPolicy {
    Auto,
    Always,
    Never,
}

enum class VesperExternalFallbackFormat {
    MpegTs,
    Hls,
}

data class VesperExternalFormatAdaptationConfig(
    val enabled: Boolean = false,
    val preferredFallback: VesperExternalFallbackFormat = VesperExternalFallbackFormat.MpegTs,
    val allowHls: Boolean = true,
    val enableRangeCache: Boolean = true,
    val allowRemoteDashMediaReferences: Boolean = false,
    val allowPrivateRemoteDashMediaAddresses: Boolean = false,
    val remoteDashMediaRequestHeaders: Set<String> = DEFAULT_REMOTE_DASH_MEDIA_REQUEST_HEADERS,
    val debugDiagnostics: Boolean = false,
)

data class VesperExternalPlaybackMediaItem(
    val sources: List<VesperPlayerSource>,
    val metadata: VesperSystemPlaybackMetadata = VesperSystemPlaybackMetadata(title = ""),
    val proxyPolicy: VesperExternalProxyPolicy = VesperExternalProxyPolicy.Auto,
    val formatAdaptation: VesperExternalFormatAdaptationConfig = VesperExternalFormatAdaptationConfig(),
)

sealed class VesperExternalPlaybackResult {
    data class Success(
        val routeId: String? = null,
        val relayEnabled: Boolean = false,
    ) : VesperExternalPlaybackResult()

    data class Unavailable(val message: String) : VesperExternalPlaybackResult()
    data class Unsupported(val message: String) : VesperExternalPlaybackResult()
    data class Failed(val message: String) : VesperExternalPlaybackResult()
}

enum class VesperExternalPlaybackEventKind {
    RouteConnected,
    RouteDisconnected,
    Loaded,
    Playing,
    Paused,
    Stopped,
    Suspended,
    Error,
    DiscoveryDiagnostic,
}

data class VesperExternalPlaybackEvent(
    val kind: VesperExternalPlaybackEventKind,
    val routeId: String? = null,
    val routeName: String? = null,
    val message: String? = null,
    val positionMs: Long? = null,
    val code: String? = null,
    val details: Map<String, String> = emptyMap(),
)

internal fun VesperExternalProxyPolicy.toInternal(): InternalProxyPolicy =
    when (this) {
        VesperExternalProxyPolicy.Auto -> InternalProxyPolicy.Auto
        VesperExternalProxyPolicy.Always -> InternalProxyPolicy.Always
        VesperExternalProxyPolicy.Never -> InternalProxyPolicy.Never
    }

internal fun VesperExternalFallbackFormat.toInternal(): InternalFallbackFormat =
    when (this) {
        VesperExternalFallbackFormat.MpegTs -> InternalFallbackFormat.MpegTs
        VesperExternalFallbackFormat.Hls -> InternalFallbackFormat.Hls
    }

internal fun VesperExternalFormatAdaptationConfig.toInternal(): InternalFormatAdaptationConfig =
    InternalFormatAdaptationConfig(
        enabled = enabled,
        preferredFallback = preferredFallback.toInternal(),
        allowHls = allowHls,
        enableRangeCache = enableRangeCache,
        allowRemoteDashMediaReferences = allowRemoteDashMediaReferences,
        allowPrivateRemoteDashMediaAddresses = allowPrivateRemoteDashMediaAddresses,
        remoteDashMediaRequestHeaders = remoteDashMediaRequestHeaders,
        debugDiagnostics = debugDiagnostics,
    )
