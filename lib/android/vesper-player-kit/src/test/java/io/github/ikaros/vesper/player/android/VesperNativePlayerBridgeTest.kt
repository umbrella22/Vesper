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
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)

        bindings.effectiveVideoTrackId = null
        bridge.refresh()
        assertNull(bridge.effectiveVideoTrackId.value)
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
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)

        bindings.onInitialize = {
            bindings.trackCatalog = VesperTrackCatalog.Empty
            bindings.trackSelection = VesperTrackSelectionSnapshot()
            bindings.effectiveVideoTrackId = null
        }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))
        assertNull(bridge.effectiveVideoTrackId.value)

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

        bridge.refresh()
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
    }

    @Test
    fun disposeClearsEffectiveVideoTrackIdImmediately() {
        val bindings = FakeBindings(effectiveVideoTrackId = "video:720p")
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)

        bridge.dispose()
        assertNull(bridge.effectiveVideoTrackId.value)
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
) : VesperNativeBindings {
    var onInitialize: (() -> Unit)? = null

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

    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = null

    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) = Unit

    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) = Unit

    override fun detachSurface() = Unit

    override fun pollSnapshot(): NativeBridgeSnapshot? = snapshot

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
