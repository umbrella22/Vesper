package io.github.umbrella22.vesper.player.android

import android.os.Build
import android.os.Process

internal object VesperNativeLibrary {
    private val loadAttempt: Result<Unit> by lazy {
        runCatching {
            loadVesperNativeLibrary(
                supportedAbis = Build.SUPPORTED_ABIS.orEmpty().toList(),
                is64BitProcess = Process.is64Bit(),
                loadLibrary = System::loadLibrary,
            )
        }
    }

    fun ensureLoaded() {
        loadAttempt.getOrThrow()
    }

    fun failureMessage(): String? = loadAttempt.exceptionOrNull()?.message
}

internal fun loadVesperNativeLibrary(
    supportedAbis: List<String>,
    is64BitProcess: Boolean,
    loadLibrary: (String) -> Unit,
) {
    if ("arm64-v8a" !in supportedAbis || !is64BitProcess) {
        throw VesperPlayerUnsupportedOperation(
            "Unsupported Android architecture: device ABIs " +
                "[${supportedAbis.joinToString()}], " +
                "${if (is64BitProcess) "64-bit" else "32-bit"} app process. " +
                "Vesper requires arm64-v8a and a 64-bit app process; " +
                "32-bit Android and Intel ABIs are not supported.",
            mapOf(
                "reason" to "unsupportedArchitecture",
                "requiredAbi" to "arm64-v8a",
                "supportedAbis" to supportedAbis.toList(),
                "is64BitProcess" to is64BitProcess,
            ),
        )
    }
    loadLibrary("vesper_player_android")
}
