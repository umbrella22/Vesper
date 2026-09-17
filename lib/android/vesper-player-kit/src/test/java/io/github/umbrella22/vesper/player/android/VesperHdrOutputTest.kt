package io.github.umbrella22.vesper.player.android

import org.junit.Assert.*
import org.junit.Test

class VesperHdrOutputTest {
    private val hdr = VesperHdrOutputObservation(
        VesperHdrOutputState.Hdr, VesperHdrOutputFormat.Hdr10, "testOutputObserver",
    )

    @Test fun initialSourceAdvancesBothRevisions() {
        val empty = VesperHdrOutputTracker().snapshot.value
        assertEquals(0L, empty.sourceRevision)
        assertEquals(0L, empty.outputGeneration)
        val initial = VesperHdrOutputTracker(true).snapshot.value
        assertEquals(1L, initial.sourceRevision)
        assertEquals(1L, initial.outputGeneration)
    }

    @Test fun synchronousListenerPreservesUnknownBeforeImmediateReconfirmation() {
        val tracker = VesperHdrOutputTracker(true)
        val outputs = mutableListOf<VesperHdrOutputSnapshot>()
        tracker.setListener { outputs.add(it) }
        assertTrue(tracker.apply(tracker.capture(), hdr))
        tracker.outputPathChanged()
        assertTrue(tracker.apply(tracker.capture(), hdr))
        assertEquals(listOf(
            VesperHdrOutputState.Unknown, VesperHdrOutputState.Hdr,
            VesperHdrOutputState.Unknown, VesperHdrOutputState.Hdr,
        ), outputs.map { it.state })
        assertEquals(outputs[1].outputGeneration + 1, outputs[2].outputGeneration)
        assertNull(outputs[2].evidence)
        assertEquals(outputs[2].outputGeneration, outputs[3].outputGeneration)
        tracker.setListener(null)
        tracker.outputPathChanged()
        assertEquals(4, outputs.size)
    }

    @Test fun disposalClearsListenerAndRejectsNewSubscriptions() {
        val tracker = VesperHdrOutputTracker(true)
        var calls = 0
        tracker.setListener { calls += 1 }
        tracker.dispose()
        assertEquals(2, calls)
        tracker.setListener { calls += 1 }
        tracker.dispose()
        tracker.outputPathChanged()
        assertEquals(2, calls)
    }

    @Test fun recurringTrackAndDisplayIdsDoNotReviveOldEvidence() {
        val tracker = VesperHdrOutputTracker(true)
        tracker.videoTrackChanged("A", 1)
        tracker.outputPathChanged("0")
        val firstA = tracker.capture()
        assertTrue(tracker.apply(firstA, hdr))
        tracker.videoTrackChanged("B", 1)
        assertEquals(VesperHdrOutputState.Unknown, tracker.snapshot.value.state)
        assertNull(tracker.snapshot.value.evidence)
        tracker.videoTrackChanged("A", 1)
        assertFalse(tracker.apply(firstA, hdr))
        val sameDisplay = tracker.capture()
        tracker.outputPathChanged("0")
        assertFalse(tracker.apply(sameDisplay, hdr))
        assertEquals(1L, tracker.snapshot.value.sourceRevision)
    }

    @Test fun repeatedSourceAndNewPlayerRejectOldResults() {
        val tracker = VesperHdrOutputTracker(true)
        val old = tracker.capture()
        tracker.sourceChanged()
        assertEquals(2L, tracker.snapshot.value.sourceRevision)
        assertFalse(tracker.apply(old, hdr))
        assertFalse(VesperHdrOutputTracker(true).apply(old, hdr))
        val pending = tracker.capture()
        tracker.dispose()
        assertFalse(tracker.apply(pending, hdr))
        val disposedGeneration = tracker.snapshot.value.outputGeneration
        tracker.dispose()
        tracker.sourceChanged()
        assertEquals(disposedGeneration, tracker.snapshot.value.outputGeneration)
    }

    @Test fun unknownAndSdrNeedTheirOwnOutputSemantics() {
        val tracker = VesperHdrOutputTracker()
        assertEquals("outputObservationUnavailable", tracker.snapshot.value.reason)
        assertFalse(tracker.apply(tracker.capture(), VesperHdrOutputObservation(VesperHdrOutputState.Sdr)))
        assertTrue(tracker.apply(tracker.capture(), hdr))
        assertTrue(tracker.apply(tracker.capture(), VesperHdrOutputObservation(
            VesperHdrOutputState.Sdr, evidence = "testSdrObserver",
        )))
        assertEquals(VesperHdrOutputState.Sdr, tracker.snapshot.value.state)
        assertEquals(VesperHdrOutputFormat.Unknown, tracker.snapshot.value.format)
        tracker.outputPathChanged()
        assertEquals(VesperHdrOutputState.Unknown, tracker.snapshot.value.state)
        assertNull(tracker.snapshot.value.evidence)
    }
}
