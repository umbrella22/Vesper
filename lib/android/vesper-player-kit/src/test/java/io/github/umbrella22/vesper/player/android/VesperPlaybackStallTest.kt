package io.github.umbrella22.vesper.player.android

import org.junit.Assert.*
import org.junit.Test

class VesperPlaybackStallTest {
    @Test fun staleTicksAndPublicationReentryCannotContaminateReplacementAttempt() {
        var now = 0L
        val tracker = VesperPlaybackDiagnosticsTracker { now }
        val token = tracker.beginAttempt()
        tracker.audioDecoder(token, "test.decoder")
        tracker.sampleStall(token, 0, true, false)
        now = 1000
        tracker.sampleStall(token, 100, true, false)
        tracker.setListener { snapshot ->
            if (snapshot.lastStall != null) tracker.beginAttempt()
        }
        var captured: VesperPlaybackStallObservation? = null
        for (time in 2000L..6000L step 1000L) {
            now = time
            captured = tracker.sampleStall(token, 100, true, false)
        }
        assertEquals("test.decoder", captured?.audio?.decoderName)
        assertFalse(tracker.isCurrent(token))
        assertNull(tracker.snapshot.value.lastStall)
        assertNull(tracker.sampleStall(token, 100, true, false))
        tracker.dispose()
        assertNull(tracker.sampleStall(tracker.capture(), 100, true, false))
    }

    @Test fun requiresProgressAndReportsOnlyOncePerAttempt() {
        val detector = VesperPlaybackStallDetector()
        for (tick in 0L..20L) assertNull(detector.sample(tick * 1000, 0, true, true))
        assertNull(detector.sample(21000, 100, true, false))
        for (tick in 22L..25L) assertNull(detector.sample(tick * 1000, 100, true, false))
        assertEquals(PlaybackStallEvidence(VesperPlaybackStallKind.PositionNotAdvancing, 5000), detector.sample(26000, 100, true, false))
        detector.resetWindow()
        for (tick in 27L..40L) detector.sample(tick * 1000, if (tick == 27L) 100 else 200, true, false)
        assertNull(detector.sample(41000, 200, true, false))
    }

    @Test fun pauseSeekAndLongSchedulingGapsRequireNewProgress() {
        val detector = VesperPlaybackStallDetector()
        detector.sample(0, 0, true, false)
        detector.sample(1000, 100, true, false)
        detector.sample(2000, 100, false, false)
        for (tick in 3L..10L) assertNull(detector.sample(tick * 1000, 100, true, false))
        detector.sample(11000, 200, true, false)
        detector.resetWindow() // Includes a seek to the same position.
        for (tick in 12L..20L) assertNull(detector.sample(tick * 1000, 200, true, false))
        detector.sample(21000, 300, true, false)
        assertNull(detector.sample(90000, 300, true, false))
        for (tick in 91L..99L) assertNull(detector.sample(tick * 1000, 300, true, false))
    }

    @Test fun rebufferingUsesItsOwnWindowAndNewAttemptCanReportAgain() {
        val detector = VesperPlaybackStallDetector()
        detector.policy = VesperPlaybackStallPolicy(positionThresholdMs = 2000, bufferingThresholdMs = 4000)
        repeat(2) {
            detector.resetAttempt()
            detector.sample(0, 0, true, false)
            detector.sample(1000, 100, true, false)
            for (tick in 2L..5L) assertNull(detector.sample(tick * 1000, 100, true, true))
            assertEquals(PlaybackStallEvidence(VesperPlaybackStallKind.BufferingTimeout, 4000), detector.sample(6000, 100, true, true))
        }
    }
}
