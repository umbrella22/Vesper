package io.github.ikaros.vesper.player.android

open class VesperPlayerUnsupportedOperation(
    message: String,
    val details: Map<String, Any?> = emptyMap(),
) : UnsupportedOperationException(message)

/** Structured failure returned by a player command outside a specialized domain. */
class VesperPlayerCommandException(
    val errorState: VesperPlayerErrorState,
) : RuntimeException(errorState.message)

/** Structured rejection for an explicit fixed-video-track command. */
class VesperFixedTrackSelectionException(
    val code: String,
    val trackId: String?,
    val expectedCatalogRevision: Long?,
    val actualCatalogRevision: Long?,
    message: String,
    extraDetails: Map<String, Any?> = emptyMap(),
) : VesperPlayerUnsupportedOperation(
    message,
    buildMap {
        put("domain", "fixedTrack")
        put("code", code)
        put("trackId", trackId)
        put("expectedCatalogRevision", expectedCatalogRevision)
        put("actualCatalogRevision", actualCatalogRevision)
        put("message", message)
        putAll(extraDetails)
    },
)

fun VesperPlayerUnsupportedOperation.toPlayerErrorState(): VesperPlayerErrorState =
    VesperPlayerErrorState(
        message = message ?: "Unsupported Vesper player operation.",
        code = VesperPlayerErrorCode.Unsupported,
        category = VesperPlayerErrorCategory.Capability,
        // Structured bridge failures may be retryable even when their outer
        // transport exception is an UnsupportedOperationException (for
        // example a temporarily saturated source-load queue).
        retriable = details["retriable"] as? Boolean ?: false,
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
