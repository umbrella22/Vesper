package io.github.umbrella22.vesper.player.android

enum class VesperVideoSurfaceKind {
    SurfaceView,
    TextureView,
}

internal fun VesperVideoSurfaceKind.toNativeSurfaceKind(): NativeVideoSurfaceKind =
    when (this) {
        VesperVideoSurfaceKind.SurfaceView -> NativeVideoSurfaceKind.SurfaceView
        VesperVideoSurfaceKind.TextureView -> NativeVideoSurfaceKind.TextureView
    }
