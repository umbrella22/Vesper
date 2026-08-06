package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperDownloadEvent
import io.github.ikaros.vesper.player.android.VesperDownloadEventBatch
import io.github.ikaros.vesper.player.android.VesperDownloadProgressSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadTaskProgressPatch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperPlayerAndroidDownloadEventMappingTest {
    private val retainedEvent =
        VesperDownloadEvent.ProgressUpdated(
            VesperDownloadTaskProgressPatch(
                taskId = 7L,
                progress = VesperDownloadProgressSnapshot(receivedBytes = 512L),
            ),
        )

    @Test
    fun pendingSnapshotResyncEmitsNoFlutterEvent() {
        val payloads =
            VesperDownloadEventBatch(
                events = listOf(retainedEvent),
                droppedEvents = 2L,
                requiresSnapshotResync = true,
                snapshotIsAuthoritative = false,
            ).toFlutterDownloadEventMaps(
                downloadId = "downloads",
                snapshot = mapOf("tasks" to emptyList<Any>()),
            )

        assertTrue(payloads.isEmpty())
    }

    @Test
    fun authoritativeSnapshotResyncSuppressesRetainedEvents() {
        val snapshot = mapOf<String, Any?>("tasks" to listOf(mapOf("taskId" to 7L)))
        val payloads =
            VesperDownloadEventBatch(
                events = listOf(retainedEvent),
                droppedEvents = 3L,
                requiresSnapshotResync = true,
                snapshotIsAuthoritative = true,
            ).toFlutterDownloadEventMaps(
                downloadId = "downloads",
                snapshot = snapshot,
            )

        assertEquals(1, payloads.size)
        assertEquals("downloads", payloads.single()["downloadId"])
        assertEquals("downloadResync", payloads.single()["type"])
        assertEquals(3L, payloads.single()["droppedEvents"])
        assertSame(snapshot, payloads.single()["snapshot"])
    }
}
