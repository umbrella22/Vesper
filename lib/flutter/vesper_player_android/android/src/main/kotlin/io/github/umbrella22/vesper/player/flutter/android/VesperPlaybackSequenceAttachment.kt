package io.github.umbrella22.vesper.player.flutter.android

internal fun <T> createAttachedPlaybackSequence(
    create: () -> T,
    attach: (T) -> Unit,
    dispose: (T) -> Unit,
): T {
    val sequence = create()
    try {
        attach(sequence)
    } catch (error: Throwable) {
        try {
            dispose(sequence)
        } catch (cleanupError: Throwable) {
            if (cleanupError !== error) {
                error.addSuppressed(cleanupError)
            }
        }
        throw error
    }
    return sequence
}
