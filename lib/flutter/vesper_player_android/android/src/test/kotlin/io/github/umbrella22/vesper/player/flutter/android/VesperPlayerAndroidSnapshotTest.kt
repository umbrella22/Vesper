package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.PlayerHostUiState
import io.github.umbrella22.vesper.player.android.VesperPlayerController
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import io.github.umbrella22.vesper.player.android.VesperPlayerErrorCategory
import io.github.umbrella22.vesper.player.android.VesperPlayerErrorCode
import io.github.umbrella22.vesper.player.android.VesperPlayerErrorState
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class VesperPlayerAndroidSnapshotTest {
    @Test
    fun recoverySnapshotDoesNotRestoreTheErrorClearedByTheCommand() {
        val controller = VesperPlayerControllerFactory.createPreview()
        try {
            val state = mutableUiState(controller)
            val error = hostError("Previous source failed")
            state.value = state.value.copy(lastError = error)
            val session = PlayerSession("player", controller, lastError = error.toMap())
            val plugin = VesperPlayerAndroidPlugin()

            // stop/selectSource clear the command error before the native bridge
            // invalidates HDR output, while the old host error is still visible.
            session.lastError = null
            assertEquals(error.toMap(), snapshot(plugin, session)["lastError"])
            assertNull(session.lastError)

            state.value = state.value.copy(lastError = null)
            assertNull(snapshot(plugin, session)["lastError"])
            assertNull(snapshot(plugin, session)["lastError"])
        } finally {
            controller.dispose()
        }
    }

    @Test
    fun samplingHostErrorPreservesTheIndependentCommandError() {
        val controller = VesperPlayerControllerFactory.createPreview()
        try {
            val state = mutableUiState(controller)
            val hostError = hostError("Host error")
            val commandError = mapOf<String, Any?>("message" to "Command error")
            val session = PlayerSession("player", controller, lastError = commandError)
            val plugin = VesperPlayerAndroidPlugin()

            state.value = state.value.copy(lastError = hostError)
            assertEquals(hostError.toMap(), snapshot(plugin, session)["lastError"])
            assertSame(commandError, session.lastError)

            state.value = state.value.copy(lastError = null)
            assertEquals(commandError, snapshot(plugin, session)["lastError"])
        } finally {
            controller.dispose()
        }
    }

    private fun hostError(message: String) = VesperPlayerErrorState(
        message = message,
        code = VesperPlayerErrorCode.BackendFailure,
        category = VesperPlayerErrorCategory.Playback,
        retriable = true,
    )

    @Suppress("UNCHECKED_CAST")
    private fun mutableUiState(controller: VesperPlayerController): MutableStateFlow<PlayerHostUiState> {
        val bridge = VesperPlayerController::class.java.getDeclaredField("bridge").apply {
            isAccessible = true
        }.get(controller)
        return bridge.javaClass.getDeclaredField("_uiState").apply {
            isAccessible = true
        }.get(bridge) as MutableStateFlow<PlayerHostUiState>
    }

    @Suppress("UNCHECKED_CAST")
    private fun snapshot(plugin: VesperPlayerAndroidPlugin, session: PlayerSession): Map<String, Any?> =
        VesperPlayerAndroidPlugin::class.java.getDeclaredMethod("buildSnapshotMap", PlayerSession::class.java).apply {
            isAccessible = true
        }.invoke(plugin, session) as Map<String, Any?>
}
