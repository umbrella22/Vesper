package io.github.ikaros.vesper.player.android

import android.util.Log
import android.view.Surface
import android.view.ViewGroup
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class VesperNativePlayerBridge(
    private val bindings: VesperNativeBindings = MissingVesperNativeBindings(),
    private val initialSource: VesperPlayerSource? = null,
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource
    private var pendingAutoPlay = false

    private val _uiState = MutableStateFlow(
        PlayerHostUiState(
            title = "Vesper",
            subtitle = "Android JNI/ExoPlayer bridge",
            sourceLabel = currentSource?.label ?: "No source selected",
            playbackState = PlaybackStateUi.Ready,
            playbackRate = 1.0f,
            isBuffering = false,
            isInterrupted = false,
            timeline = TimelineUiState(
                kind = TimelineKind.Vod,
                isSeekable = true,
                seekableRange = SeekableRangeUi(0L, 134_100L),
                liveEdgeMs = null,
                positionMs = 0L,
                durationMs = 134_100L,
            ),
        )
    )
    private val _trackCatalog = MutableStateFlow(VesperTrackCatalog.Empty)
    private val _trackSelection = MutableStateFlow(VesperTrackSelectionSnapshot())
    private val surfaceHost = VesperNativeSurfaceHost(bindings)

    override val backend: PlayerBridgeBackend = PlayerBridgeBackend.VesperNativeStub
    override val uiState: StateFlow<PlayerHostUiState> = _uiState.asStateFlow()
    override val trackCatalog: StateFlow<VesperTrackCatalog> = _trackCatalog.asStateFlow()
    override val trackSelection: StateFlow<VesperTrackSelectionSnapshot> =
        _trackSelection.asStateFlow()

    init {
        bindings.setOnNativeUpdateListener(::refreshFromNative)
    }

    override fun initialize() {
        val source = currentSource ?: run {
            updateState {
                copy(
                    subtitle = "Select a media source to begin playback",
                    sourceLabel = "No source selected",
                    playbackState = PlaybackStateUi.Ready,
                    isBuffering = false,
                )
            }
            return
        }

        runCatching { bindings.initialize(source) }
            .onSuccess {
                Log.i(
                    TAG,
                    "initialized source=${source.uri} label=${source.label} kind=${source.kind} protocol=${source.protocol}",
                )
                surfaceHost.reattachIfAvailable()
                val shouldAutoPlay = pendingAutoPlay
                pendingAutoPlay = false
                if (shouldAutoPlay) {
                    Log.i(TAG, "auto-playing selected source=${source.uri}")
                    bindings.play()
                }
                updateState {
                    copy(
                        subtitle = it.subtitle ?: sourceSubtitle(source),
                        sourceLabel = source.label,
                    )
                }
                refreshFromNative()
            }
            .onFailure {
                pendingAutoPlay = false
                Log.e(TAG, "failed to initialize source=${source.uri}", it)
                updateState {
                    copy(
                        subtitle = "Android JNI bridge stub: ${it.message ?: "native bindings unavailable"}",
                        sourceLabel = source.label,
                    )
                }
            }
    }

    override fun dispose() {
        surfaceHost.detach()
        bindings.setOnNativeUpdateListener(null)
        bindings.dispose()
    }

    override fun refresh() {
        bindings.refreshSnapshot()
        refreshFromNative()
    }

    override fun selectSource(source: VesperPlayerSource) {
        currentSource = source
        pendingAutoPlay = true
        Log.i(
            TAG,
            "selecting source=${source.uri} label=${source.label} kind=${source.kind} protocol=${source.protocol}",
        )
        updateState {
            copy(
                subtitle = "Opening ${source.label}",
                sourceLabel = source.label,
                playbackState = PlaybackStateUi.Ready,
                isBuffering = true,
                timeline = timeline.copy(positionMs = 0L),
            )
        }
        initialize()
    }

    override fun attachSurfaceHost(host: ViewGroup) {
        surfaceHost.updateVideoLayout(bindings.currentVideoLayoutInfo())
        surfaceHost.attach(host)
        refreshFromNative()
    }

    override fun detachSurfaceHost(host: ViewGroup?) {
        surfaceHost.detach(host)
    }

    override fun play() {
        bindings.play()
        updateState { copy(playbackState = PlaybackStateUi.Playing, isBuffering = false) }
        refreshFromNative()
    }

    override fun pause() {
        bindings.pause()
        updateState { copy(playbackState = PlaybackStateUi.Paused, isBuffering = false) }
        refreshFromNative()
    }

    override fun togglePause() {
        when (_uiState.value.playbackState) {
            PlaybackStateUi.Playing -> pause()
            PlaybackStateUi.Ready,
            PlaybackStateUi.Paused,
            PlaybackStateUi.Finished,
            -> play()
        }
    }

    override fun stop() {
        bindings.stop()
        updateState {
            copy(
                playbackState = PlaybackStateUi.Ready,
                timeline = timeline.copy(positionMs = 0L),
                isBuffering = false,
            )
        }
        refreshFromNative()
    }

    override fun seekBy(deltaMs: Long) {
        val current = _uiState.value.timeline
        val minimum = current.seekableRange?.startMs ?: 0L
        val maximum = current.seekableRange?.endMs ?: (current.durationMs ?: 0L)
        val target = (current.positionMs + deltaMs).coerceIn(minimum, maximum)
        bindings.seekTo(target)
        updateState { copy(timeline = timeline.copy(positionMs = target)) }
        refreshFromNative()
    }

    override fun seekToRatio(ratio: Float) {
        val timeline = _uiState.value.timeline
        val position = if (timeline.seekableRange != null) {
            val range = timeline.seekableRange
            val width = (range.endMs - range.startMs).toFloat()
            range.startMs + (width * ratio.coerceIn(0f, 1f)).toLong()
        } else {
            ((timeline.durationMs ?: 0L).toFloat() * ratio.coerceIn(0f, 1f)).toLong()
        }
        bindings.seekTo(position)
        updateState { copy(timeline = timeline.copy(positionMs = position)) }
        refreshFromNative()
    }

    override fun seekToLiveEdge() {
        val timeline = _uiState.value.timeline
        val liveEdge = timeline.liveEdgeMs ?: timeline.seekableRange?.endMs ?: return
        bindings.seekTo(liveEdge)
        updateState { copy(timeline = timeline.copy(positionMs = liveEdge)) }
        refreshFromNative()
    }

    override fun setPlaybackRate(rate: Float) {
        bindings.setPlaybackRate(rate)
        updateState { copy(playbackRate = rate) }
        refreshFromNative()
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        bindings.setVideoTrackSelection(selection)
        refreshFromNative()
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        bindings.setAudioTrackSelection(selection)
        refreshFromNative()
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        bindings.setSubtitleTrackSelection(selection)
        refreshFromNative()
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        bindings.setAbrPolicy(policy)
        refreshFromNative()
    }

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
    }

    private fun refreshFromNative() {
        surfaceHost.updateVideoLayout(bindings.currentVideoLayoutInfo())
        _trackCatalog.value = bindings.currentTrackCatalog()
        _trackSelection.value = bindings.currentTrackSelection()

        bindings.pollSnapshot()?.let { snapshot ->
            updateState {
                copy(
                    playbackState = snapshot.playbackState,
                    playbackRate = snapshot.playbackRate,
                    isBuffering = snapshot.isBuffering,
                    isInterrupted = snapshot.isInterrupted,
                    timeline = snapshot.timeline,
                )
            }
        }

        bindings.drainEvents().forEach { event ->
            when (event) {
                is NativeBridgeEvent.PlaybackStateChanged -> updateState {
                    copy(playbackState = event.state)
                }
                is NativeBridgeEvent.PlaybackRateChanged -> updateState {
                    copy(playbackRate = event.rate)
                }
                is NativeBridgeEvent.BufferingChanged -> updateState {
                    copy(isBuffering = event.isBuffering)
                }
                is NativeBridgeEvent.InterruptionChanged -> updateState {
                    copy(isInterrupted = event.isInterrupted)
                }
                is NativeBridgeEvent.VideoSurfaceChanged -> updateState {
                    copy(
                        subtitle = if (event.attached) {
                            currentSource?.let { "${sourceSubtitle(it)} / surface attached" }
                                ?: "Android JNI + ExoPlayer ready / surface attached"
                        } else {
                            currentSource?.let { "${sourceSubtitle(it)} / surface detached" }
                                ?: "Android JNI + ExoPlayer ready / surface detached"
                        }
                    )
                }
                is NativeBridgeEvent.SeekCompleted -> updateState {
                    copy(timeline = timeline.copy(positionMs = event.positionMs))
                }
                is NativeBridgeEvent.Ended -> updateState {
                    copy(playbackState = PlaybackStateUi.Finished, isBuffering = false)
                }
                is NativeBridgeEvent.Error -> updateState {
                    copy(subtitle = "Android native bridge error: ${event.message}")
                }
            }
        }

    }
}

private const val TAG = "VesperPlayerAndroidHost"

private fun sourceSubtitle(source: VesperPlayerSource): String =
    when (source.kind) {
        VesperPlayerSourceKind.Local -> "Android JNI + ExoPlayer ready (local source)"
        VesperPlayerSourceKind.Remote ->
            "Android JNI + ExoPlayer ready (${source.protocol.name.lowercase()} remote source)"
    }

interface VesperNativeBindings {
    fun initialize(source: VesperPlayerSource): NativeBridgeStartup
    fun dispose()
    fun refreshSnapshot()
    fun currentTrackCatalog(): VesperTrackCatalog
    fun currentTrackSelection(): VesperTrackSelectionSnapshot
    fun currentVideoLayoutInfo(): NativeVideoLayoutInfo?
    fun setOnNativeUpdateListener(listener: (() -> Unit)?)
    fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind)
    fun detachSurface()
    fun pollSnapshot(): NativeBridgeSnapshot?
    fun drainEvents(): List<NativeBridgeEvent>
    fun play()
    fun pause()
    fun stop()
    fun seekTo(positionMs: Long)
    fun setPlaybackRate(rate: Float)
    fun setVideoTrackSelection(selection: VesperTrackSelection)
    fun setAudioTrackSelection(selection: VesperTrackSelection)
    fun setSubtitleTrackSelection(selection: VesperTrackSelection)
    fun setAbrPolicy(policy: VesperAbrPolicy)
}

private class MissingVesperNativeBindings : VesperNativeBindings {
    override fun initialize(source: VesperPlayerSource): NativeBridgeStartup {
        throw UnsupportedOperationException(
            VesperNativeLibrary.failureMessage() ?: "JNI bridge is not wired yet"
        )
    }

    override fun dispose() = Unit
    override fun refreshSnapshot() = Unit
    override fun currentTrackCatalog(): VesperTrackCatalog = VesperTrackCatalog.Empty
    override fun currentTrackSelection(): VesperTrackSelectionSnapshot =
        VesperTrackSelectionSnapshot()
    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = null
    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) = Unit
    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) = Unit
    override fun detachSurface() = Unit
    override fun pollSnapshot(): NativeBridgeSnapshot? = null
    override fun drainEvents(): List<NativeBridgeEvent> = emptyList()
    override fun play() = Unit
    override fun pause() = Unit
    override fun stop() = Unit
    override fun seekTo(positionMs: Long) = Unit
    override fun setPlaybackRate(rate: Float) = Unit
    override fun setVideoTrackSelection(selection: VesperTrackSelection) = Unit
    override fun setAudioTrackSelection(selection: VesperTrackSelection) = Unit
    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) = Unit
    override fun setAbrPolicy(policy: VesperAbrPolicy) = Unit
}
