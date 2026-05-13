package io.github.ikaros.vesper.player.android

/**
 * Public runtime backend family exposed without leaking bridge or JNI types.
 */
enum class VesperPlayerBackendFamily {
    AndroidHostKit,
    FakeDemo,
}
