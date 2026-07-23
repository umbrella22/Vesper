package io.github.ikaros.vesper.player.android

import android.content.Context
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import androidx.media3.common.text.Cue
import androidx.media3.exoplayer.ExoPlayer
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Device-level proof for local DASH WebVTT discovery, selection, and cue delivery.
 *
 * Stable-id edge cases are covered by JVM tests. These tests own Media3 behavior
 * that cannot be established without an Android player and the JNI host library.
 */
@RunWith(AndroidJUnit4::class)
class VesperSubtitleMedia3InstrumentationTest {
    private var player: ExoPlayer? = null

    @After
    fun tearDown() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                player?.release()
                player = null
            }
    }

    @Test
    fun localDashWebVttIsDiscoveredSelectedAndProducesCue() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val root = fixtureDirectory(context, "vesper-subtitle-instrumentation")
        copyMediaFixture(context, File(root, "video.m4v"))
        writeDashFixture(root)

        val trackDiscovered = CountDownLatch(1)
        val selectionConfirmed = CountDownLatch(1)
        val cueReady = CountDownLatch(1)
        val selectionRequested = AtomicBoolean(false)
        val discoveredStableId = AtomicReference<String>()
        val playbackEvents = Collections.synchronizedList(mutableListOf<String>())

        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val exoPlayer =
                    ExoPlayer.Builder(context, VesperExternalSubtitleRenderersFactory(context))
                        .build()
                        .also { player = it }
                exoPlayer.addListener(
                    object : Player.Listener {
                        override fun onPlaybackStateChanged(playbackState: Int) {
                            playbackEvents += "playbackState=$playbackState"
                        }

                        override fun onPlayerError(error: PlaybackException) {
                            val causeChain =
                                generateSequence<Throwable>(error) { throwable -> throwable.cause }
                                    .joinToString(" <- ") { throwable ->
                                        "${throwable.javaClass.simpleName}:${throwable.message}"
                                    }
                            playbackEvents += "playerError=${error.errorCodeName}:$causeChain"
                        }

                        override fun onTracksChanged(tracks: Tracks) {
                            val textGroup = tracks.groups.firstOrNull { group ->
                                group.type == C.TRACK_TYPE_TEXT && group.length > 0
                            } ?: return
                            val format = textGroup.getTrackFormat(0)
                            discoveredStableId.set(subtitleStableTrackId(format))
                            playbackEvents +=
                                "textTrackDiscovered=" +
                                    (0 until textGroup.length).joinToString { index ->
                                        "${textGroup.getTrackFormat(index).id}:" +
                                            "selected=${textGroup.isTrackSelected(index)}"
                                    }
                            if (selectionRequested.compareAndSet(false, true)) {
                                exoPlayer.trackSelectionParameters =
                                    exoPlayer.trackSelectionParameters
                                        .buildUpon()
                                        .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                                        .setOverrideForType(
                                            TrackSelectionOverride(textGroup.mediaTrackGroup, 0),
                                        )
                                        .build()
                                exoPlayer.seekTo(0L)
                                exoPlayer.playWhenReady = true
                            }
                            trackDiscovered.countDown()
                            if (textGroup.isTrackSelected(0)) {
                                selectionConfirmed.countDown()
                            }
                        }

                        override fun onCues(cueGroup: androidx.media3.common.text.CueGroup) {
                            playbackEvents += "cueCount=${cueGroup.cues.size}"
                            if (cueGroup.cues.any()) {
                                cueReady.countDown()
                            }
                        }
                    },
                )
                exoPlayer.setMediaItem(
                    MediaItem.Builder()
                        .setUri(android.net.Uri.fromFile(File(root, "manifest.mpd")))
                        .setMimeType(MimeTypes.APPLICATION_MPD)
                        .build(),
                )
                exoPlayer.trackSelectionParameters =
                    exoPlayer.trackSelectionParameters
                        .buildUpon()
                        .setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, true)
                        .setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, true)
                        .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                        .build()
                exoPlayer.prepare()
            }

        assertTrue(
            "Media3 did not discover a local DASH WebVTT track; ${currentPlayerState()}; " +
                "events=${playbackEvents.joinToString()}",
            trackDiscovered.await(15, TimeUnit.SECONDS),
        )
        assertEquals("subtitle:dash:sub-en", discoveredStableId.get())
        assertTrue("Test did not issue a manual text-track override", selectionRequested.get())
        assertTrue(
            "Media3 did not confirm the selected DASH WebVTT track; " +
                "events=${playbackEvents.joinToString()}",
            selectionConfirmed.await(15, TimeUnit.SECONDS),
        )
        assertTrue(
            "Media3 did not deliver a WebVTT cue; ${currentPlayerState()}; " +
                "events=${playbackEvents.joinToString()}",
            cueReady.await(15, TimeUnit.SECONDS),
        )
        assertTrue(
            "Media3 reported a playback error while decoding WebVTT; " +
                "events=${playbackEvents.joinToString()}",
            playbackEvents.none { it.startsWith("playerError=") },
        )
    }

    @Test
    fun nativeBindingsPreserveBridgeListenersAcrossReinitialize() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val root = fixtureDirectory(context, "vesper-listener-retention-instrumentation")
        val mediaFile = File(root, "video.m4v")
        copyMediaFixture(context, mediaFile)
        val cueListener: (List<Cue>) -> Unit = {}
        val failureListener: (NativeTrackSelectionFailure) -> Unit = {}

        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val bindings = VesperNativeJniBindings(context)
                try {
                    bindings.setOnSubtitleCuesListener(cueListener)
                    bindings.setOnTrackSelectionFailureListener(failureListener)
                    repeat(2) {
                        bindings.initialize(
                            source =
                                VesperPlayerSource.local(
                                    uri = android.net.Uri.fromFile(mediaFile).toString(),
                                    label = "Listener retention fixture",
                                ),
                            resiliencePolicy = VesperPlaybackResiliencePolicy(),
                            trackPreferencePolicy = VesperTrackPreferencePolicy(),
                            systemPlaybackUsesSourceNormalizerResource = false,
                            systemPlaybackVideoEnabled = false,
                            preparedSourceNormalizer = NativeSourceNormalizerResourcePreparedOpenOutcome(),
                        )
                        assertSame(cueListener, bindings.subtitleCuesListener)
                        assertSame(failureListener, bindings.trackSelectionFailureListener)
                    }
                } finally {
                    bindings.dispose()
                }
            }
    }

    private fun currentPlayerState(): String {
        var state = "player=released"
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                player?.let { exoPlayer ->
                    state =
                        "state=${exoPlayer.playbackState},position=${exoPlayer.currentPosition}," +
                            "bufferedPosition=${exoPlayer.bufferedPosition},duration=${exoPlayer.duration}"
                }
            }
        return state
    }

    private fun fixtureDirectory(context: Context, name: String): File {
        val root = File(context.cacheDir, name)
        root.deleteRecursively()
        check(root.mkdirs()) { "failed to create instrumentation fixture directory" }
        return root
    }

    private fun copyMediaFixture(context: Context, destination: File) {
        context.assets.open("tiny-h264-aac.m4v").use { input ->
            destination.outputStream().use(input::copyTo)
        }
    }

    private fun writeDashFixture(root: File) {
        File(root, "subtitle.vtt").writeText(
            """WEBVTT

00:00:00.500 --> 00:00:01.500
device subtitle proof
""".trimIndent() + "\n",
        )
        val subtitleUri = android.net.Uri.fromFile(File(root, "subtitle.vtt"))
        val mediaUri = android.net.Uri.fromFile(File(root, "video.m4v"))
        File(root, "manifest.mpd").writeText(
            """<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT2S" minBufferTime="PT0.1S">
  <Period id="period-0" start="PT0S">
    <AdaptationSet id="1" contentType="video" mimeType="video/mp4" codecs="avc1.42C00A">
      <Representation id="video-main" bandwidth="57700" width="128" height="72" frameRate="24">
        <BaseURL>$mediaUri</BaseURL>
      </Representation>
    </AdaptationSet>
    <AdaptationSet id="2" contentType="audio" mimeType="audio/mp4" codecs="mp4a.40.2" lang="und">
      <Representation id="audio-main" bandwidth="64000" audioSamplingRate="48000">
        <BaseURL>$mediaUri</BaseURL>
      </Representation>
    </AdaptationSet>
    <AdaptationSet id="3" contentType="text" mimeType="text/vtt" lang="en" label="English">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <SegmentTemplate timescale="1000" media="$subtitleUri">
        <SegmentTimeline>
          <S t="0" d="2000"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>
""".trimIndent() + "\n",
        )
    }
}
