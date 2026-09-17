package io.github.umbrella22.vesper.player.android

/** Display dimensions after native rotation and pixel aspect correction. */
data class VesperVideoPresentation(
    val displayWidth: Double,
    val displayHeight: Double,
) {
    val displayAspectRatio: Double get() = displayWidth / displayHeight
}

/** Attached surface geometry in dp, relative to the host's top-left corner. */
data class VesperVideoSurfaceGeometry(
    val width: Double,
    val height: Double,
    val contentRect: VesperVideoRect,
)

data class VesperVideoRect(
    val left: Double,
    val top: Double,
    val width: Double,
    val height: Double,
)

internal fun NativeVideoLayoutInfo.toVideoPresentation(): VesperVideoPresentation? {
    if (width <= 0 || height <= 0 || !pixelWidthHeightRatio.isFinite() || pixelWidthHeightRatio <= 0f) return null
    return VesperVideoPresentation(width.toDouble() * pixelWidthHeightRatio, height.toDouble())
}
