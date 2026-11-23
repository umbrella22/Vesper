package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperBenchmarkRecorderTest {
    @Test
    fun disabledRecorderRejectsEvents() {
        val recorder = VesperBenchmarkRecorder()

        recorder.record("dropped_video_frames", null, mapOf("count" to "1"))

        assertTrue(recorder.drainEvents().isEmpty())
        assertEquals(0L, recorder.summary().acceptedEvents)
    }

    @Test
    fun disposedRecorderRejectsLaterEvents() {
        val recorder =
            VesperBenchmarkRecorder(
                VesperBenchmarkConfiguration(enabled = true),
            )
        recorder.record("first_frame_rendered", null)

        recorder.dispose()
        recorder.record("dropped_video_frames", null, mapOf("count" to "1"))

        assertEquals(listOf("first_frame_rendered"), recorder.drainEvents().map { it.eventName })
        assertEquals(1L, recorder.summary().acceptedEvents)
    }

    @Test
    fun legacyPluginReferencesReportRetirementAndKeepLocalEvents() {
        val recorder = VesperBenchmarkRecorder(
            VesperBenchmarkConfiguration(
                enabled = true,
                maxBufferedEvents = 1,
                pluginReferences = listOf(VesperBundledPluginReferences.performanceDiagnostics),
            ),
        )
        repeat(2) { recorder.record("play_command", VesperPlayerSourceProtocol.Hls) }

        val summary = recorder.summary()
        assertEquals(listOf("play_command"), recorder.drainEvents().map { it.eventName })
        assertEquals(2L, summary.acceptedEvents)
        assertEquals(1L, summary.droppedEvents)
        assertEquals(0L, summary.pluginAcceptedEvents)
        assertTrue(summary.pluginErrors.single().contains("retired"))
    }
}
