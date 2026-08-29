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
 * Opt-in physical-device proof for the public Dolby Vision Online Delivery Kit
 * signals used by the example hosts.
 *
 * The catalog is deliberately opt-in because the media is public but external;
 * routine Android test suites must not depend on Dolby availability.
 */
@RunWith(AndroidJUnit4::class)
class VesperDolbyVisionPlaybackInstrumentationTest {
    @Test
    fun publicP81ClearDashRendersVideoAndAdvancesTimeline() {
        assumeNetworkEnabled()
        runPlayback(
            source =
                VesperPlayerSource(
                    uri = CLEAR_P81_30_DASH,
                    label = CLEAR_LABEL,
                    kind = VesperPlayerSourceKind.Remote,
                    protocol = VesperPlayerSourceProtocol.Dash,
                ),
            expectedEvents = setOf("video_decoder_initialized", "first_frame_rendered"),
            expectedDrmEvents = emptySet(),
        )
    }

    @Test
    fun publicP81CencDashLoadsWidevineKeysAndRendersVideo() {
        assumeNetworkEnabled()
        runPlayback(
            source =
                VesperPlayerSource(
                    uri = CENC_P81_30_DASH,
                    label = CENC_LABEL,
                    kind = VesperPlayerSourceKind.Remote,
                    protocol = VesperPlayerSourceProtocol.Dash,
                    drmConfiguration =
                        VesperPlayerDrmConfiguration(
                            keySystem = "widevine",
                            licenseUri = DOLBY_WIDEVINE_LICENSE_URI,
                        ),
                ),
            expectedEvents = setOf("video_decoder_initialized", "first_frame_rendered"),
            expectedDrmEvents = setOf("drm_keys_loaded"),
        )
    }

    private fun assumeNetworkEnabled() {
        assumeTrue(
            "requires -Pandroid.testInstrumentationRunnerArguments.vesperDolbyVisionNetwork=true",
            InstrumentationRegistry.getArguments()
                .getString(NETWORK_ARGUMENT)
                .equals("true", ignoreCase = true),
        )
    }

    private fun runPlayback(
        source: VesperPlayerSource,
        expectedEvents: Set<String>,
        expectedDrmEvents: Set<String>,
    ) {
        val context = ApplicationProvider.getApplicationContext<Context>()
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

                val activeController = requireNotNull(controller)
                val accepted = awaitPlayback(activeController, observedEvents, expectedEvents, expectedDrmEvents)
                activeController.refresh()
                observedEvents += activeController.drainBenchmarkEvents()
                val state = activeController.uiState.value
                val eventNames = observedEvents.map(VesperBenchmarkEvent::eventName).toSet()

                assertTrue(
                    "Dolby playback did not converge: label=${state.sourceLabel}, " +
                        "state=${state.playbackState}, buffering=${state.isBuffering}, " +
                        "positionMs=${state.timeline.positionMs}, error=${state.lastError}, events=$eventNames",
                    accepted,
                )
                assertNull(state.lastError)
                assertEquals(source.label, state.sourceLabel)
                assertTrue(state.timeline.positionMs >= MINIMUM_ADVANCED_POSITION_MS)
                expectedEvents.forEach { eventName -> assertTrue(eventName in eventNames) }
                expectedDrmEvents.forEach { eventName -> assertTrue(eventName in eventNames) }
                assertFalse(
                    "unexpected terminal DRM or playback failure: $eventNames",
                    observedEvents.any(::isTerminalFailureEvent),
                )

                if (source.drmConfiguration != null) {
                    val keysLoaded = observedEvents.first { it.eventName == "drm_keys_loaded" }
                    assertEquals("widevine", keysLoaded.attributes["keySystem"])
                    assertEquals(
                        source.drmConfiguration.licenseUri.substringAfter("//").substringBefore('/'),
                        keysLoaded.attributes["licenseUriHost"],
                    )
                }
            } finally {
                scenario.onActivity {
                    controller?.dispose()
                    controller = null
                }
            }
        }
    }

    private fun awaitPlayback(
        controller: VesperPlayerController,
        observedEvents: MutableList<VesperBenchmarkEvent>,
        expectedEvents: Set<String>,
        expectedDrmEvents: Set<String>,
    ): Boolean {
        val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(PLAYBACK_TIMEOUT_SECONDS)
        while (System.nanoTime() < deadlineNanos) {
            controller.refresh()
            observedEvents += controller.drainBenchmarkEvents()
            val eventNames = observedEvents.mapTo(mutableSetOf(), VesperBenchmarkEvent::eventName)
            val state = controller.uiState.value
            if (state.lastError != null || observedEvents.any(::isTerminalFailureEvent)) return false
            if (state.timeline.positionMs >= MINIMUM_ADVANCED_POSITION_MS &&
                expectedEvents.all(eventNames::contains) &&
                expectedDrmEvents.all(eventNames::contains)
            ) {
                return true
            }
            Thread.sleep(50L)
        }
        return false
    }

    private companion object {
        const val NETWORK_ARGUMENT = "vesperDolbyVisionNetwork"
        const val CLEAR_P81_30_DASH =
            "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/clear/P8_1_30/dash.mpd"
        const val CENC_P81_30_DASH =
            "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/cenc/P8_1_30/dash.mpd"
        const val DOLBY_WIDEVINE_LICENSE_URI = "https://widevine-dash.ezdrm.com/proxy?pX=E8A6EE"
        const val CLEAR_LABEL = "Dolby P8.1 30fps DASH Clear"
        const val CENC_LABEL = "Dolby P8.1 30fps DASH Widevine"
        const val PLAYBACK_TIMEOUT_SECONDS = 60L
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
