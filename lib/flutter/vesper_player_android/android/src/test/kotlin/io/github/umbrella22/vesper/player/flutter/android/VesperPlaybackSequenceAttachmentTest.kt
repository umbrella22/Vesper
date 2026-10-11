package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperPlaybackSequenceException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Test

class VesperPlaybackSequenceAttachmentTest {
    @Test
    fun rejectedDifferentSequenceDisposesOnlyTheNewOwner() {
        val controller = TrackingController()
        val original = TrackingSequence("A")
        original.attach(controller)
        val rejected = TrackingSequence("B")

        val error = assertThrows(VesperPlaybackSequenceException::class.java) {
            createAttachedPlaybackSequence<TrackingSequence>(
                create = { rejected },
                attach = { it.attach(controller) },
                dispose = { it.dispose() },
            )
        }

        assertEquals("already_attached", error.code)
        assertEquals("already_attached", error.message)
        assertEquals(1, rejected.disposeCalls)
        assertEquals(0, original.disposeCalls)
        assertSame(original, controller.attachment)
    }

    @Test
    fun successfulAttachmentRetainsTheNewOwner() {
        val controller = TrackingController()
        val created = TrackingSequence("B")

        val retained = createAttachedPlaybackSequence<TrackingSequence>(
            create = { created },
            attach = { it.attach(controller) },
            dispose = { it.dispose() },
        )

        assertSame(created, retained)
        assertSame(created, controller.attachment)
        assertEquals(0, created.disposeCalls)
    }

    @Test
    fun constructorFailureDoesNotInventAnOwnerToAttachOrDispose() {
        val failure = IllegalStateException("native constructor failed")
        var constructorCalls = 0
        var attachCalls = 0
        var disposeCalls = 0

        val error = assertThrows(IllegalStateException::class.java) {
            createAttachedPlaybackSequence<TrackingSequence>(
                create = {
                    constructorCalls++
                    throw failure
                },
                attach = { attachCalls++ },
                dispose = { disposeCalls++ },
            )
        }

        assertSame(failure, error)
        assertEquals(1, constructorCalls)
        assertEquals(0, attachCalls)
        assertEquals(0, disposeCalls)
    }

    @Test
    fun cleanupFailureIsSuppressedWithoutReplacingTheAttachmentError() {
        val rejected = TrackingSequence("B")
        val failure = VesperPlaybackSequenceException("already_attached")
        val cleanupFailure = IllegalStateException("dispose failed")

        val error = assertThrows(VesperPlaybackSequenceException::class.java) {
            createAttachedPlaybackSequence<TrackingSequence>(
                create = { rejected },
                attach = { throw failure },
                dispose = {
                    it.dispose()
                    throw cleanupFailure
                },
            )
        }

        assertSame(failure, error)
        assertEquals("already_attached", error.code)
        assertEquals(1, rejected.disposeCalls)
        assertEquals(1, error.suppressed.size)
        assertSame(cleanupFailure, error.suppressed.single())
    }

    @Test
    fun cleanupRethrowingTheSameErrorPreservesThePrimaryFailure() {
        val rejected = TrackingSequence("B")
        val failure = VesperPlaybackSequenceException("already_attached")

        val error = assertThrows(VesperPlaybackSequenceException::class.java) {
            createAttachedPlaybackSequence<TrackingSequence>(
                create = { rejected },
                attach = { throw failure },
                dispose = {
                    it.dispose()
                    throw failure
                },
            )
        }

        assertSame(failure, error)
        assertEquals(1, rejected.disposeCalls)
        assertEquals(0, error.suppressed.size)
    }

    // The JNI-owning kit sequence requires a MainLooper and the native library.
    // These owners exercise the same synchronous attach-or-dispose contract.
    private class TrackingSequence(val id: String) {
        var disposeCalls = 0
        private var controller: TrackingController? = null

        fun attach(target: TrackingController) {
            target.attach(this)
            controller = target
        }

        fun dispose() {
            disposeCalls++
            controller?.detach(this)
            controller = null
        }
    }

    private class TrackingController {
        var attachment: TrackingSequence? = null
            private set

        fun attach(sequence: TrackingSequence) {
            if (attachment != null) {
                throw VesperPlaybackSequenceException("already_attached")
            }
            attachment = sequence
        }

        fun detach(sequence: TrackingSequence) {
            if (attachment === sequence) {
                attachment = null
            }
        }
    }
}
