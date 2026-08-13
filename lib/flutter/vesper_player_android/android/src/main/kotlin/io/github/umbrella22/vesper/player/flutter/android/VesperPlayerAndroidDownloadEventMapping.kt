package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperDownloadEvent
import io.github.umbrella22.vesper.player.android.VesperDownloadEventBatch
import io.github.umbrella22.vesper.player.android.VesperDownloadState

internal fun VesperDownloadEventBatch.toFlutterDownloadEventMaps(
    downloadId: String,
    snapshot: Map<String, Any?>,
): List<Map<String, Any?>> {
    if (requiresSnapshotResync) {
        if (!snapshotIsAuthoritative) {
            return emptyList()
        }
        return listOf(
            mapOf(
                "downloadId" to downloadId,
                "type" to "downloadResync",
                "snapshot" to snapshot,
                "droppedEvents" to droppedEvents,
            ),
        )
    }

    return events.map { event ->
        when (event) {
            is VesperDownloadEvent.Created ->
                mapOf(
                    "downloadId" to downloadId,
                    "type" to "taskCreated",
                    "task" to event.task.toMap(),
                )
            is VesperDownloadEvent.AssetIndexUpdated ->
                mapOf(
                    "downloadId" to downloadId,
                    "type" to "taskUpdated",
                    "task" to event.task.toMap(),
                )
            is VesperDownloadEvent.StateChanged ->
                if (event.patch.state == VesperDownloadState.Removed) {
                    mapOf(
                        "downloadId" to downloadId,
                        "type" to "taskRemoved",
                        "taskId" to event.patch.taskId,
                    )
                } else {
                    mapOf(
                        "downloadId" to downloadId,
                        "type" to "taskUpdated",
                        "patch" to event.patch.toMap(),
                    )
                }
            is VesperDownloadEvent.ProgressUpdated ->
                mapOf(
                    "downloadId" to downloadId,
                    "type" to "taskUpdated",
                    "progressPatch" to event.patch.toMap(),
                )
        }
    }
}
