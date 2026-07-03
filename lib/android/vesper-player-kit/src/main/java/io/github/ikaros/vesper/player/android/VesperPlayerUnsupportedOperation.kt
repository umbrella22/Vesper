package io.github.ikaros.vesper.player.android

class VesperPlayerUnsupportedOperation(
    message: String,
    val details: Map<String, Any?> = emptyMap(),
) : UnsupportedOperationException(message)

fun VesperPlayerUnsupportedOperation.toPlayerErrorState(): VesperPlayerErrorState =
    VesperPlayerErrorState(
        message = message ?: "Unsupported Vesper player operation.",
        code = VesperPlayerErrorCode.Unsupported,
        category = VesperPlayerErrorCategory.Capability,
        retriable = false,
        details = details,
    )

fun drmUnsupportedRouteDetails(
    source: VesperPlayerSource,
    route: String,
    reason: String = "drmUnsupportedRoute",
): Map<String, Any?> =
    mapOf(
        "reason" to reason,
        "route" to route,
        "keySystem" to source.drmConfiguration?.keySystem,
    )

fun drmUnsupportedRouteMessage(route: String): String =
    "DRM is not supported on the $route playback route."
