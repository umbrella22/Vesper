package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlaybackSequenceAttachmentTest {
    private class Attachment : VesperPlaybackSequenceAttachment {
        var disposed = false

        override fun onControllerDisposed(controller: VesperPlayerController) {
            disposed = true
        }
    }

    @Test
    fun controllerRejectsSecondSequenceAndDirectSourceSelection() {
        val controller = VesperPlayerController(FakePlayerBridge())
        val first = Attachment()
        val second = Attachment()

        controller.attachPlaybackSequence(first)

        val duplicate = runCatching { controller.attachPlaybackSequence(second) }.exceptionOrNull()
        assertTrue(duplicate is VesperPlaybackSequenceException)
        assertEquals("already_attached", (duplicate as VesperPlaybackSequenceException).code)

        val direct = runCatching {
            controller.selectSource(VesperPlayerSource.remote("https://example.com/a.mp4", "a"))
        }.exceptionOrNull()
        assertTrue(direct is VesperPlaybackSequenceException)
        assertEquals("sequence_attached_conflict", (direct as VesperPlaybackSequenceException).code)

        controller.dispose()
        assertTrue(first.disposed)
    }

    @Test
    fun repeatedControllerDisposeIsIdempotent() {
        val controller = VesperPlayerController(FakePlayerBridge())
        val attachment = Attachment()
        controller.attachPlaybackSequence(attachment)

        controller.dispose()
        controller.dispose()

        assertTrue(attachment.disposed)
    }
}
