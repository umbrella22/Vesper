package io.github.ikaros.vesper.player.android

import android.content.Context
import android.util.Log
import android.view.Surface
import android.view.ViewGroup
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlin.math.absoluteValue

class VesperNativePlayerBridge(
    private val bindings: VesperNativeBindings = MissingVesperNativeBindings(),
    private val initialSource: VesperPlayerSource? = null,
    private var resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    private var trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
    private val preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
    appContext: Context? = null,
    surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
) : PlayerBridge {
    private var currentSource: VesperPlayerSource? = initialSource
    private var hasInitializedSource = false
    private var pendingAutoPlay = false
    private val i18n = VesperPlayerI18n.fromContext(appContext)

    private val _uiState = MutableStateFlow(
        PlayerHostUiState(
            title = i18n.playerTitle(),
            subtitle = i18n.nativeBridgeReady(),
            sourceLabel = currentSource?.label ?: i18n.noSourceSelected(),
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
    private val surfaceHost = VesperNativeSurfaceHost(bindings, surfaceKind)

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
                    subtitle = i18n.selectSourcePrompt(),
                    sourceLabel = i18n.noSourceSelected(),
                    playbackState = PlaybackStateUi.Ready,
                    isBuffering = false,
                )
            }
            return
        }

        runCatching { bindings.initialize(source, resiliencePolicy, trackPreferencePolicy) }
            .onSuccess {
                hasInitializedSource = true
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
                hasInitializedSource = false
                pendingAutoPlay = false
                Log.e(TAG, "failed to initialize source=${source.uri}", it)
                val message = it.message?.takeUnless(String::isBlank) ?: i18n.nativeBindingsUnavailable()
                updateState {
                    copy(
                        subtitle = i18n.stubError(message),
                        sourceLabel = source.label,
                    )
                }
            }
    }

    override fun dispose() {
        hasInitializedSource = false
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
                subtitle = i18n.openingSource(source.label),
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
        val target = current.clampedPosition(current.positionMs + deltaMs)
        bindings.seekTo(target)
        updateState { copy(timeline = timeline.copy(positionMs = target)) }
        refreshFromNative()
    }

    override fun seekToRatio(ratio: Float) {
        val timeline = _uiState.value.timeline
        val position = timeline.positionForRatio(ratio)
        bindings.seekTo(position)
        updateState { copy(timeline = timeline.copy(positionMs = position)) }
        refreshFromNative()
    }

    override fun seekToLiveEdge() {
        val timeline = _uiState.value.timeline
        val liveEdge = timeline.goLivePositionMs ?: return
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

    override fun setResiliencePolicy(policy: VesperPlaybackResiliencePolicy) {
        if (resiliencePolicy == policy) {
            return
        }

        resiliencePolicy = policy
        val source = currentSource ?: return
        if (!hasInitializedSource) {
            return
        }

        val preservedState = PreservedPlaybackState.capture(
            uiState = _uiState.value,
            trackSelection = _trackSelection.value,
        )

        Log.i(
            TAG,
            "apply resilience policy buffering=${policy.buffering.preset} retry=${policy.retry.backoff} cache=${policy.cache.preset}",
        )
        updateState { copy(isBuffering = true) }
        initialize()
        restorePlaybackState(source, preservedState)
    }

    private inline fun updateState(transform: PlayerHostUiState.() -> PlayerHostUiState) {
        _uiState.value = _uiState.value.transform()
    }

    private fun restorePlaybackState(
        source: VesperPlayerSource,
        preservedState: PreservedPlaybackState,
    ) {
        if (!hasInitializedSource) {
            return
        }

        when {
            preservedState.seekToLiveEdge &&
                _uiState.value.timeline.kind == TimelineKind.LiveDvr -> {
                val liveEdge =
                    _uiState.value.timeline.goLivePositionMs ?: _uiState.value.timeline.positionMs
                bindings.seekTo(liveEdge)
            }
            preservedState.restorePosition &&
                (source.kind == VesperPlayerSourceKind.Local ||
                    source.kind == VesperPlayerSourceKind.Remote) -> {
                bindings.seekTo(preservedState.positionMs.coerceAtLeast(0L))
            }
        }

        if ((preservedState.playbackRate - 1.0f).absoluteValue > 0.001f) {
            bindings.setPlaybackRate(preservedState.playbackRate)
        }

        if (preservedState.videoSelection.mode != VesperTrackSelectionMode.Auto) {
            bindings.setVideoTrackSelection(preservedState.videoSelection)
        }
        if (preservedState.audioSelection.mode != VesperTrackSelectionMode.Auto) {
            bindings.setAudioTrackSelection(preservedState.audioSelection)
        }
        if (preservedState.subtitleSelection.mode != VesperTrackSelectionMode.Auto) {
            bindings.setSubtitleTrackSelection(preservedState.subtitleSelection)
        }
        bindings.setAbrPolicy(preservedState.abrPolicy)

        if (preservedState.shouldResumePlayback) {
            bindings.play()
        } else if (preservedState.playbackState == PlaybackStateUi.Paused) {
            bindings.pause()
        }

        refreshFromNative()
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
                            i18n.surfaceAttached(currentSource?.let(::sourceSubtitle))
                        } else {
                            i18n.surfaceDetached(currentSource?.let(::sourceSubtitle))
                        }
                    )
                }
                is NativeBridgeEvent.SeekCompleted -> updateState {
                    copy(timeline = timeline.copy(positionMs = event.positionMs))
                }
                is NativeBridgeEvent.RetryScheduled -> updateState {
                    copy(
                        subtitle = i18n.retryScheduled(i18n.retryDelay(event.delayMs), event.attempt),
                    )
                }
                is NativeBridgeEvent.Ended -> updateState {
                    copy(playbackState = PlaybackStateUi.Finished, isBuffering = false)
                }
                is NativeBridgeEvent.Error -> updateState {
                    copy(subtitle = i18n.nativeError(event.message))
                }
            }
        }

    }

    private fun sourceSubtitle(source: VesperPlayerSource): String = i18n.sourceSubtitle(source)
}

private const val TAG = "VesperPlayerAndroidHost"

private data class PreservedPlaybackState(
    val positionMs: Long,
    val restorePosition: Boolean,
    val seekToLiveEdge: Boolean,
    val playbackRate: Float,
    val playbackState: PlaybackStateUi,
    val shouldResumePlayback: Boolean,
    val videoSelection: VesperTrackSelection,
    val audioSelection: VesperTrackSelection,
    val subtitleSelection: VesperTrackSelection,
    val abrPolicy: VesperAbrPolicy,
) {
    companion object {
        fun capture(
            uiState: PlayerHostUiState,
            trackSelection: VesperTrackSelectionSnapshot,
        ): PreservedPlaybackState {
            val seekToLiveEdge =
                uiState.timeline.kind == TimelineKind.LiveDvr &&
                    uiState.timeline.isAtLiveEdge()
            return PreservedPlaybackState(
                positionMs = uiState.timeline.positionMs,
                restorePosition = uiState.timeline.isSeekable || uiState.timeline.durationMs != null,
                seekToLiveEdge = seekToLiveEdge,
                playbackRate = uiState.playbackRate,
                playbackState = uiState.playbackState,
                shouldResumePlayback = uiState.playbackState == PlaybackStateUi.Playing,
                videoSelection = trackSelection.video,
                audioSelection = trackSelection.audio,
                subtitleSelection = trackSelection.subtitle,
                abrPolicy = trackSelection.abrPolicy,
            )
        }
    }
}

interface VesperNativeBindings {
    fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
    ): NativeBridgeStartup
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
    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
    ): NativeBridgeStartup {
        throw UnsupportedOperationException(VesperNativeLibrary.failureMessage())
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
