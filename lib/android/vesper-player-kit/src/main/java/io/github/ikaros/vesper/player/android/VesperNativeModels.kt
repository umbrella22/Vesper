package io.github.ikaros.vesper.player.android

enum class NativeVideoSurfaceKind {
    TextureView,
    SurfaceView,
}

data class NativeBridgeStartup(
    val subtitle: String? = null,
)

data class NativeBridgeSnapshot(
    val playbackState: PlaybackStateUi,
    val playbackRate: Float,
    val isBuffering: Boolean,
    val isInterrupted: Boolean,
    val timeline: TimelineUiState,
)

sealed interface NativeBridgeEvent {
    data class PlaybackStateChanged(val state: PlaybackStateUi) : NativeBridgeEvent
    data class PlaybackRateChanged(val rate: Float) : NativeBridgeEvent
    data class BufferingChanged(val isBuffering: Boolean) : NativeBridgeEvent
    data class InterruptionChanged(val isInterrupted: Boolean) : NativeBridgeEvent
    data class VideoSurfaceChanged(val attached: Boolean) : NativeBridgeEvent
    data class SeekCompleted(val positionMs: Long) : NativeBridgeEvent
    data class Ended(val ended: Boolean = true) : NativeBridgeEvent
    data class Error(val message: String) : NativeBridgeEvent
}

sealed interface NativePlayerCommand {
    data object Play : NativePlayerCommand
    data object Pause : NativePlayerCommand
    data class SeekTo(val positionMs: Long) : NativePlayerCommand
    data object Stop : NativePlayerCommand
    data class SetPlaybackRate(val rate: Float) : NativePlayerCommand
}
