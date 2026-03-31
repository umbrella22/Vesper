package io.github.ikaros.vesper.player.android

import kotlin.jvm.JvmField

enum class NativeVideoSurfaceKind {
    TextureView,
    SurfaceView,
}

enum class NativeTrackKind {
    Video,
    Audio,
    Subtitle,
}

enum class NativeTrackSelectionMode {
    Auto,
    Disabled,
    Track,
}

enum class NativeAbrMode {
    Auto,
    Constrained,
    FixedTrack,
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

class NativeTrackInfo(
    @JvmField val id: String,
    @JvmField val kindOrdinal: Int,
    @JvmField val label: String?,
    @JvmField val language: String?,
    @JvmField val codec: String?,
    @JvmField val hasBitRate: Boolean,
    @JvmField val bitRate: Long,
    @JvmField val hasWidth: Boolean,
    @JvmField val width: Int,
    @JvmField val hasHeight: Boolean,
    @JvmField val height: Int,
    @JvmField val hasFrameRate: Boolean,
    @JvmField val frameRate: Float,
    @JvmField val hasChannels: Boolean,
    @JvmField val channels: Int,
    @JvmField val hasSampleRate: Boolean,
    @JvmField val sampleRate: Int,
    @JvmField val isDefault: Boolean,
    @JvmField val isForced: Boolean,
)

class NativeTrackCatalog(
    @JvmField val tracks: Array<NativeTrackInfo>,
    @JvmField val adaptiveVideo: Boolean,
    @JvmField val adaptiveAudio: Boolean,
)

class NativeTrackSelectionPayload(
    @JvmField val modeOrdinal: Int,
    @JvmField val trackId: String?,
)

class NativeAbrPolicyPayload(
    @JvmField val modeOrdinal: Int,
    @JvmField val trackId: String?,
    @JvmField val hasMaxBitRate: Boolean,
    @JvmField val maxBitRate: Long,
    @JvmField val hasMaxWidth: Boolean,
    @JvmField val maxWidth: Int,
    @JvmField val hasMaxHeight: Boolean,
    @JvmField val maxHeight: Int,
)

class NativeTrackSelectionSnapshotPayload(
    @JvmField val video: NativeTrackSelectionPayload,
    @JvmField val audio: NativeTrackSelectionPayload,
    @JvmField val subtitle: NativeTrackSelectionPayload,
    @JvmField val abrPolicy: NativeAbrPolicyPayload,
)

data class NativeVideoLayoutInfo(
    val width: Int,
    val height: Int,
    val pixelWidthHeightRatio: Float = 1.0f,
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
    data class SetVideoTrackSelection(val selection: NativeTrackSelectionPayload) : NativePlayerCommand
    data class SetAudioTrackSelection(val selection: NativeTrackSelectionPayload) : NativePlayerCommand
    data class SetSubtitleTrackSelection(val selection: NativeTrackSelectionPayload) : NativePlayerCommand
    data class SetAbrPolicy(val policy: NativeAbrPolicyPayload) : NativePlayerCommand
}
