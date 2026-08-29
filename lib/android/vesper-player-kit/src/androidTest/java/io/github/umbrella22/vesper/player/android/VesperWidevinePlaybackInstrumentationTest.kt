package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Opt-in physical-device proof for Vesper's direct Media3 Widevine route.
 *
 * The fixture pair is published by the Apache-2.0 Shaka Player demo catalog.
 * Keep this test opt-in so routine device suites do not depend on public network
 * or license-server availability.
 */
@RunWith(AndroidJUnit4::class)
class VesperWidevinePlaybackInstrumentationTest {
    @Test
    fun publicWidevineAssetLoadsKeysRendersVideoAndAdvancesTimeline() {
        assumeTrue(
            "requires -Pandroid.testInstrumentationRunnerArguments.vesperWidevineNetwork=true",
            InstrumentationRegistry.getArguments()
                .getString(WIDEVINE_NETWORK_ARGUMENT)
                .equals("true", ignoreCase = true),
        )

        val context = ApplicationProvider.getApplicationContext<Context>()
        val source =
            VesperPlayerSource(
                uri = SHAKA_ANGEL_ONE_WIDEVINE_MANIFEST,
                label = "Shaka Angel One Widevine",
                kind = VesperPlayerSourceKind.Remote,
                protocol = VesperPlayerSourceProtocol.Dash,
                drmConfiguration =
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = SHAKA_WIDEVINE_LICENSE_SERVER,
                    ),
            )
        val observedEvents = mutableListOf<VesperBenchmarkEvent>()
        var controller: VesperPlayerController? = null

        ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
            try {
                scenario.onActivity { activity ->
                    val surfaceHost: FrameLayout = activity.replaceSurfaceHost()
                    controller =
                        VesperPlayerControllerFactory.createDefault(
                            context = context,
                            initialSource = source,
                            resiliencePolicy = VesperPlaybackResiliencePolicy.streaming(),
                            decoderBackend = VesperDecoderBackend.SystemOnly,
                            surfaceKind = VesperVideoSurfaceKind.SurfaceView,
                            keepScreenOnDuringPlayback = false,
                            benchmarkConfiguration =
                                VesperBenchmarkConfiguration(
                                    enabled = true,
                                    maxBufferedEvents = 4_096,
                                ),
                        ).also { player ->
                            player.attachSurfaceHost(surfaceHost)
                            player.initialize()
                            player.play()
                        }
                }

                val accepted = awaitWidevinePlayback(controller, observedEvents)
                val activeController = requireNotNull(controller)
                activeController.refresh()
                observedEvents += activeController.drainBenchmarkEvents()
                val state = activeController.uiState.value
                val eventNames = observedEvents.map(VesperBenchmarkEvent::eventName)

                assertTrue(
                    "Widevine playback did not converge: state=${state.playbackState}, " +
                        "buffering=${state.isBuffering}, positionMs=${state.timeline.positionMs}, " +
                        "error=${state.lastError}, events=$eventNames",
                    accepted,
                )
                assertNull(state.lastError)
                assertEquals("Shaka Angel One Widevine", state.sourceLabel)
                assertTrue(state.timeline.positionMs >= MINIMUM_ADVANCED_POSITION_MS)
                assertTrue(eventNames.contains("drm_keys_loaded"))
                assertTrue(eventNames.contains("video_decoder_initialized"))
                assertTrue(eventNames.contains("first_frame_rendered"))
                assertFalse(
                    "unexpected terminal DRM or playback failure: $eventNames",
                    observedEvents.any(::isTerminalFailureEvent),
                )

                val keysLoaded = observedEvents.first { it.eventName == "drm_keys_loaded" }
                assertEquals("widevine", keysLoaded.attributes["keySystem"])
                assertEquals("proxy.uat.widevine.com", keysLoaded.attributes["licenseUriHost"])
            } finally {
                scenario.onActivity {
                    controller?.dispose()
                    controller = null
                }
            }
        }
    }

    private fun awaitWidevinePlayback(
        controller: VesperPlayerController?,
        observedEvents: MutableList<VesperBenchmarkEvent>,
    ): Boolean {
        val activeController = requireNotNull(controller)
        val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(WIDEVINE_TIMEOUT_SECONDS)
        while (System.nanoTime() < deadlineNanos) {
            activeController.refresh()
            observedEvents += activeController.drainBenchmarkEvents()
            val state = activeController.uiState.value
            val eventNames = observedEvents.mapTo(mutableSetOf(), VesperBenchmarkEvent::eventName)
            if (state.lastError != null || observedEvents.any(::isTerminalFailureEvent)) {
                return false
            }
            if (state.timeline.positionMs >= MINIMUM_ADVANCED_POSITION_MS &&
                "drm_keys_loaded" in eventNames &&
                "video_decoder_initialized" in eventNames &&
                "first_frame_rendered" in eventNames
            ) {
                return true
            }
            Thread.sleep(50L)
        }
        return false
    }

    private companion object {
        const val WIDEVINE_NETWORK_ARGUMENT = "vesperWidevineNetwork"
        const val SHAKA_ANGEL_ONE_WIDEVINE_MANIFEST =
            "https://storage.googleapis.com/shaka-demo-assets/angel-one-widevine/dash.mpd"
        const val SHAKA_WIDEVINE_LICENSE_SERVER = "https://proxy.uat.widevine.com/proxy"
        const val WIDEVINE_TIMEOUT_SECONDS = 60L
        const val MINIMUM_ADVANCED_POSITION_MS = 3_000L

        fun isTerminalFailureEvent(event: VesperBenchmarkEvent): Boolean =
            when (event.eventName) {
                "playback_error" -> true
                "drm_session_manager_error" ->
                    event.attributes["attemptsExhausted"].equals("true", ignoreCase = true)
                else -> false
            }
    }
}
