package io.github.umbrella22.vesper.player.android

import org.junit.Assert.*
import org.junit.Test

class VesperPlaybackDiagnosticsTest {
    @Test
    fun startupUsesMonotonicElapsedTimeAndDeduplicatesWithinAttempt() {
        var clock = 100L
        val tracker = VesperPlaybackDiagnosticsTracker { clock }
        val token = tracker.beginAttempt()
        clock += 125
        assertTrue(tracker.firstFrame(token, 90_000))
        assertEquals(125L, tracker.snapshot.value.firstFrame?.elapsedSinceLoadStartMs)
        assertEquals(90_000L, tracker.snapshot.value.firstFrame?.mediaPositionMs)
        clock += 100
        assertFalse(tracker.firstFrame(token, 90_100))
        assertEquals(125L, tracker.snapshot.value.firstFrame?.elapsedSinceLoadStartMs)
    }

    @Test
    fun replacementRejectsQueuedAudioAndVideoIncludingRecurringSources() {
        val tracker = VesperPlaybackDiagnosticsTracker { 100L }
        val old = tracker.beginAttempt()
        tracker.audioDecoder(old, "old.decoder")
        tracker.invalidate()
        assertFalse(tracker.firstFrame(old, 0))
        val replacement = tracker.beginAttempt()
        tracker.audioDecoder(old, "late.decoder")
        assertNull(tracker.snapshot.value.audio.decoderName)
        assertNull(tracker.audioIssue(old, VesperAudioDiagnosticIssueKind.SinkError, "old", "old"))
        assertFalse(tracker.firstFrame(old, 0))
        assertTrue(tracker.firstFrame(replacement, 0))
        tracker.dispose()
        tracker.audioDecoder(replacement, "disposed.decoder")
        assertNull(tracker.snapshot.value.audio.decoderName)
    }

    @Test
    fun recoverableAudioIssueRetainsInputEvidenceWithoutClaimingOutputProgress() {
        var clock = 10L
        val tracker = VesperPlaybackDiagnosticsTracker { clock }
        val token = tracker.beginAttempt()
        tracker.audioDecoder(token, "test.eac3.decoder")
        tracker.audioFormat(token, VesperAudioPlaybackDiagnostics(
            codec = "ec-3", sampleMimeType = "audio/eac3",
            evidence = VesperAudioDiagnosticEvidence.RuntimeFormat,
        ))
        clock = 40
        val result = tracker.audioIssue(token, VesperAudioDiagnosticIssueKind.SinkError, "write", "failed")!!
        assertEquals("test.eac3.decoder", result.audio.decoderName)
        assertEquals("ec-3", result.audio.codec)
        assertNull(result.audio.sampleRate)
        assertEquals(30L, result.audio.lastIssue?.elapsedSinceLoadStartMs)
        assertNull(result.firstFrame)
        tracker.audioFormat(token, VesperAudioPlaybackDiagnostics(codec = "mp4a.40.2"))
        assertEquals(result.audio.lastIssue, tracker.snapshot.value.audio.lastIssue)
        tracker.audioDisabled(token)
        assertNull(tracker.snapshot.value.audio.codec)
        assertNull(tracker.snapshot.value.audio.decoderName)
        assertEquals(result.audio.lastIssue, tracker.snapshot.value.audio.lastIssue)
        tracker.beginAttempt()
        assertNull(tracker.snapshot.value.audio.lastIssue)
    }

    @Test
    fun synchronousSourceReplacementCannotRetagAnOldIssue() {
        val tracker = VesperPlaybackDiagnosticsTracker { 100L }
        val token = tracker.beginAttempt()
        tracker.setListener { if (it.audio.lastIssue != null) tracker.invalidate() }
        val captured = tracker.audioIssue(token, VesperAudioDiagnosticIssueKind.SinkError, "write", "failed")!!
        assertEquals(token.epoch, captured.playbackEpoch)
        assertNotNull(captured.audio.lastIssue)
        assertFalse(tracker.isCurrent(token))
        assertNotEquals(token.epoch, tracker.snapshot.value.playbackEpoch)
        assertNull(tracker.snapshot.value.audio.lastIssue)
    }

    @Test
    fun synchronousDisposalDuringFirstFrameRejectsFurtherCallbackWork() {
        val tracker = VesperPlaybackDiagnosticsTracker { 100L }
        val token = tracker.beginAttempt()
        tracker.setListener { if (it.firstFrame != null) tracker.dispose() }
        assertTrue(tracker.firstFrame(token, 0))
        assertFalse(tracker.isCurrent(token))
        assertFalse(tracker.firstFrame(token, 1))
    }

    @Test
    fun tokensFromAnotherControllerCannotConfirmFirstFrame() {
        val first = VesperPlaybackDiagnosticsTracker { 0L }
        val second = VesperPlaybackDiagnosticsTracker { 0L }
        val token = first.beginAttempt()
        second.beginAttempt()
        assertFalse(second.firstFrame(token, 0))
    }
}
