package io.github.ikaros.vesper.player.android

object VesperNativeLibrary {
    private const val LIB_NAME = "vesper_player_android"

    private val loadAttempt: Result<Unit> by lazy {
        runCatching { System.loadLibrary(LIB_NAME) }
    }

    fun ensureLoaded() {
        loadAttempt.getOrThrow()
    }

    fun failureMessage(): String? = loadAttempt.exceptionOrNull()?.message
}
