import Flutter
import Foundation
import VesperPlayerKit

func flutterDownloadEventPayloads(
    downloadId: String,
    snapshot: [String: Any],
    batch: VesperDownloadEventBatch
) -> [[String: Any]] {
    if batch.requiresSnapshotResync {
        guard batch.snapshotIsAuthoritative else { return [] }
        return [[
            "downloadId": downloadId,
            "type": "downloadResync",
            "snapshot": snapshot,
            "droppedEvents": NSNumber(value: batch.droppedEvents),
        ]]
    }

    return batch.events.map { event in
        switch event {
        case .created(let task):
            [
                "downloadId": downloadId,
                "type": "taskCreated",
                "task": task.toMap,
            ]
        case .assetIndexUpdated(let task):
            [
                "downloadId": downloadId,
                "type": "taskUpdated",
                "task": task.toMap,
            ]
        case .stateChanged(let patch):
            if patch.state == .removed {
                [
                    "downloadId": downloadId,
                    "type": "taskRemoved",
                    "taskId": NSNumber(value: patch.taskId),
                ]
            } else {
                [
                    "downloadId": downloadId,
                    "type": "taskUpdated",
                    "patch": patch.toMap,
                ]
            }
        case .progressUpdated(let patch):
            [
                "downloadId": downloadId,
                "type": "taskUpdated",
                "progressPatch": patch.toMap,
            ]
        }
    }
}

final class DownloadEventStreamHandler: NSObject, FlutterStreamHandler {
    private weak var plugin: VesperPlayerIosPlugin?

    init(plugin: VesperPlayerIosPlugin) {
        self.plugin = plugin
    }

    func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        Task { @MainActor [weak plugin] in
            guard let plugin else { return }
            plugin.downloadEventSink = events
            plugin.downloadSessions.values.forEach {
                plugin.emitDownloadSnapshot(for: $0)
                plugin.emitDownloadRuntimeEvents(for: $0)
            }
        }
        return nil
    }

    func onCancel(withArguments arguments: Any?) -> FlutterError? {
        Task { @MainActor [weak plugin] in
            plugin?.downloadEventSink = nil
        }
        return nil
    }
}
