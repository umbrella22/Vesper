package io.github.ikaros.vesper.player.android

import android.view.Surface
import androidx.media3.common.Format
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VesperNativePlayerBridgeTest {
    @Test
    fun refreshMirrorsEffectiveVideoTrackIdFromBindings() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:720p",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                ),
                trackSelection = VesperTrackSelectionSnapshot(abrPolicy = VesperAbrPolicy.auto()),
                effectiveVideoTrackId = "video:720p",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)
        assertEquals(
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            ),
            bridge.videoVariantObservation.value,
        )

        bindings.effectiveVideoTrackId = null
        bindings.videoVariantObservation = null
        bridge.refresh()
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun selectSourceClearsStaleEffectiveVideoTrackIdUntilBindingsPublishNewState() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:old",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
                    ),
                effectiveVideoTrackId = "video:old",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)
        assertEquals(1_500_000L, bridge.videoVariantObservation.value?.bitRate)

        bindings.onInitialize = {
            bindings.trackCatalog = VesperTrackCatalog.Empty
            bindings.trackSelection = VesperTrackSelectionSnapshot()
            bindings.effectiveVideoTrackId = null
            bindings.videoVariantObservation = null
        }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)

        bindings.trackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:new",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        bindings.trackSelection = VesperTrackSelectionSnapshot(abrPolicy = VesperAbrPolicy.auto())
        bindings.effectiveVideoTrackId = "video:new"
        bindings.videoVariantObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )

        bridge.refresh()
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(1920, bridge.videoVariantObservation.value?.width)
    }

    @Test
    fun disposeClearsEffectiveVideoTrackIdImmediately() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:720p",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:720p"),
                    ),
                effectiveVideoTrackId = "video:720p",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)
        assertEquals(1280, bridge.videoVariantObservation.value?.width)

        bridge.dispose()
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)

        bridge.refresh()
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun selectSourceFailureClearsStaleTrackState() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:old",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
                    ),
                effectiveVideoTrackId = "video:old",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals(1, bridge.trackCatalog.value.tracks.size)
        assertEquals(
            VesperAbrPolicy.fixedTrack("video:old"),
            bridge.trackSelection.value.abrPolicy,
        )
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)

        bindings.onInitialize = { error("simulated initialize failure") }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))

        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerFromPreviousSourceIsIgnored() {
        val oldTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:old",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val oldTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
            )
        val oldObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val newTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:new",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        val newTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.auto(),
            )
        val newObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )
        val bindings =
            FakeBindings(
                trackCatalog = oldTrackCatalog,
                trackSelection = oldTrackSelection,
                effectiveVideoTrackId = "video:old",
                videoVariantObservation = oldObservation,
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())
        bindings.onInitialize = {
            bindings.trackCatalog = newTrackCatalog
            bindings.trackSelection = newTrackSelection
            bindings.effectiveVideoTrackId = "video:new"
            bindings.videoVariantObservation = newObservation
            bindings.events.clear()
        }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))

        val expectedUiState = bridge.uiState.value
        assertEquals(newTrackCatalog, bridge.trackCatalog.value)
        assertEquals(newTrackSelection, bridge.trackSelection.value)
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(newObservation, bridge.videoVariantObservation.value)

        bindings.trackCatalog = oldTrackCatalog
        bindings.trackSelection = oldTrackSelection
        bindings.effectiveVideoTrackId = "video:old"
        bindings.videoVariantObservation = oldObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale old error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(newTrackCatalog, bridge.trackCatalog.value)
        assertEquals(newTrackSelection, bridge.trackSelection.value)
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(newObservation, bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerAfterDisposeIsIgnored() {
        val staleTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:stale",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val staleTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:stale"),
            )
        val staleObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val bindings =
            FakeBindings(
                trackCatalog = staleTrackCatalog,
                trackSelection = staleTrackSelection,
                effectiveVideoTrackId = "video:stale",
                videoVariantObservation = staleObservation,
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())

        bridge.dispose()
        val expectedUiState = bridge.uiState.value

        bindings.trackCatalog = staleTrackCatalog
        bindings.trackSelection = staleTrackSelection
        bindings.effectiveVideoTrackId = "video:stale"
        bindings.videoVariantObservation = staleObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale disposed error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerAfterResilienceReinitIsIgnored() {
        val oldTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:old",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val oldTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
            )
        val oldObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val reinitTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:reinit",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        val reinitTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.auto(),
            )
        val reinitObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )
        val bindings =
            FakeBindings(
                trackCatalog = oldTrackCatalog,
                trackSelection = oldTrackSelection,
                effectiveVideoTrackId = "video:old",
                videoVariantObservation = oldObservation,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live"),
            )

        bridge.initialize()
        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())

        bindings.onInitialize = {
            bindings.trackCatalog = reinitTrackCatalog
            bindings.trackSelection = reinitTrackSelection
            bindings.effectiveVideoTrackId = "video:reinit"
            bindings.videoVariantObservation = reinitObservation
            bindings.events.clear()
        }

        bridge.setResiliencePolicy(VesperPlaybackResiliencePolicy.resilient())

        val expectedUiState = bridge.uiState.value
        assertEquals(reinitTrackCatalog, bridge.trackCatalog.value)
        assertEquals(reinitTrackSelection, bridge.trackSelection.value)
        assertEquals("video:reinit", bridge.effectiveVideoTrackId.value)
        assertEquals(reinitObservation, bridge.videoVariantObservation.value)

        bindings.trackCatalog = oldTrackCatalog
        bindings.trackSelection = oldTrackSelection
        bindings.effectiveVideoTrackId = "video:old"
        bindings.videoVariantObservation = oldObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale resilience error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(reinitTrackCatalog, bridge.trackCatalog.value)
        assertEquals(reinitTrackSelection, bridge.trackSelection.value)
        assertEquals("video:reinit", bridge.effectiveVideoTrackId.value)
        assertEquals(reinitObservation, bridge.videoVariantObservation.value)
    }

    @Test
    fun resolveVideoVariantObservationUsesRenderedFormat() {
        val observation =
            resolveVideoVariantObservation(
                Format.Builder()
                    .setPeakBitrate(1_500_000)
                    .setWidth(1280)
                    .setHeight(720)
                    .build(),
            )

        assertEquals(
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            ),
            observation,
        )
    }

    @Test
    fun resolveVideoVariantObservationReturnsNilWhenFormatLacksSignal() {
        assertNull(resolveVideoVariantObservation(Format.Builder().build()))
    }

    @Test
    fun resolveEffectiveVideoTrackIdUsesCurrentRenderedFormat() {
        val effectiveTrackId =
            resolveEffectiveVideoTrackId(
                videoTracks =
                    listOf(
                        VesperMediaTrack(
                            id = "group:video-480:0",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 900_000L,
                            width = 854,
                            height = 480,
                            frameRate = 30f,
                        ),
                        VesperMediaTrack(
                            id = "group:video-720:1",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 1_500_000L,
                            width = 1280,
                            height = 720,
                            frameRate = 30f,
                        ),
                    ),
                currentVideoFormat =
                    Format.Builder()
                        .setId("video-720")
                        .setCodecs("avc1.4d401f")
                        .setPeakBitrate(1_500_000)
                        .setWidth(1280)
                        .setHeight(720)
                        .setFrameRate(30f)
                        .build(),
            )

        assertEquals("group:video-720:1", effectiveTrackId)
    }

    @Test
    fun resolveEffectiveVideoTrackIdStaysNilWhenFormatIsTooAmbiguous() {
        val effectiveTrackId =
            resolveEffectiveVideoTrackId(
                videoTracks =
                    listOf(
                        VesperMediaTrack(
                            id = "group:video-480:0",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 900_000L,
                        ),
                        VesperMediaTrack(
                            id = "group:video-720:1",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 1_500_000L,
                        ),
                    ),
                currentVideoFormat =
                    Format.Builder()
                        .setCodecs("avc1.4d401f")
                        .build(),
            )

        assertNull(effectiveTrackId)
    }
}

private class FakeBindings(
    private var snapshot: NativeBridgeSnapshot? = null,
    var trackCatalog: VesperTrackCatalog = VesperTrackCatalog.Empty,
    var trackSelection: VesperTrackSelectionSnapshot = VesperTrackSelectionSnapshot(),
    var effectiveVideoTrackId: String? = null,
    var videoVariantObservation: VesperVideoVariantObservation? = null,
) : VesperNativeBindings {
    var onInitialize: (() -> Unit)? = null
    val events = mutableListOf<NativeBridgeEvent>()
    private var updateListener: (() -> Unit)? = null

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
    ): NativeBridgeStartup {
        onInitialize?.invoke()
        return NativeBridgeStartup(subtitle = null)
    }

    override fun dispose() = Unit

    override fun refreshSnapshot() = Unit

    override fun currentTrackCatalog(): VesperTrackCatalog = trackCatalog

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = trackSelection

    override fun currentEffectiveVideoTrackId(): String? = effectiveVideoTrackId

    override fun currentVideoVariantObservation(): VesperVideoVariantObservation? =
        videoVariantObservation

    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = null

    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) {
        updateListener = listener
    }

    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) = Unit

    override fun detachSurface() = Unit

    override fun pollSnapshot(): NativeBridgeSnapshot? = snapshot

    override fun drainEvents(): List<NativeBridgeEvent> = events.toList().also { events.clear() }

    override fun play() = Unit

    override fun pause() = Unit

    override fun stop() = Unit

    override fun seekTo(positionMs: Long) = Unit

    override fun setPlaybackRate(rate: Float) = Unit

    override fun setVideoTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAudioTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) = Unit

    override fun setAbrPolicy(policy: VesperAbrPolicy) = Unit

    fun currentUpdateListener(): (() -> Unit)? = updateListener
}
