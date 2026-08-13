package io.github.umbrella22.vesper.player.android

/**
 * Public runtime backend family exposed without leaking bridge or JNI types.
 */
enum class VesperPlayerBackendFamily {
    AndroidHostKit,
    FakeDemo,
}
