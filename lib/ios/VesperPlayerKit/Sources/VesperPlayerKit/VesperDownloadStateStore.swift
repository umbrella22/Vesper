import Foundation

final class DownloadTaskStore {
    private var tasksById: [VesperDownloadTaskId: VesperDownloadTaskSnapshot] = [:]
    private var order: [VesperDownloadTaskId] = []

    func replaceAll(_ snapshot: VesperDownloadSnapshot) {
        let activeTasks = snapshot.tasks.filter { $0.state != .removed }
        tasksById = Dictionary(uniqueKeysWithValues: activeTasks.map { ($0.taskId, $0) })
        order = activeTasks.map(\.taskId)
    }

    @discardableResult
    func apply(_ events: [VesperDownloadEvent]) -> VesperDownloadSnapshot {
        for event in events {
            switch event {
            case let .created(task), let .assetIndexUpdated(task):
                upsert(task)
            case let .stateChanged(patch):
                if patch.state == .removed {
                    remove(patch.taskId)
                    continue
                }
                guard let task = tasksById[patch.taskId] else {
                    continue
                }
                tasksById[patch.taskId] = task.withStatePatch(patch)
            case let .progressUpdated(patch):
                guard let task = tasksById[patch.taskId] else {
                    continue
                }
                tasksById[patch.taskId] = task.withProgress(patch.progress)
            }
        }
        return snapshot()
    }

    func snapshot() -> VesperDownloadSnapshot {
        VesperDownloadSnapshot(tasks: order.compactMap { tasksById[$0] })
    }

    private func upsert(_ task: VesperDownloadTaskSnapshot) {
        if tasksById[task.taskId] == nil {
            order.append(task.taskId)
        }
        tasksById[task.taskId] = task
    }

    private func remove(_ taskId: VesperDownloadTaskId) {
        tasksById.removeValue(forKey: taskId)
        order.removeAll { $0 == taskId }
    }
}

final class VesperDownloadStateStore {
    private let fileURL: URL
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(fileURL: URL) {
        self.fileURL = fileURL
    }

    func load() -> VesperDownloadSnapshot {
        guard let data = try? Data(contentsOf: fileURL),
              let snapshot = try? decoder.decode(VesperDownloadSnapshot.self, from: data)
        else {
            return VesperDownloadSnapshot(tasks: [])
        }
        return snapshot
    }

    func save(_ snapshot: VesperDownloadSnapshot) {
        let tasks = snapshot.tasks.filter { $0.state != .removed }
        guard !tasks.isEmpty else {
            try? FileManager.default.removeItem(at: fileURL)
            return
        }
        do {
            try FileManager.default.createDirectory(
                at: fileURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            excludeDownloadItemFromBackup(fileURL.deletingLastPathComponent())
            let data = try encoder.encode(VesperDownloadSnapshot(tasks: tasks))
            try data.write(to: fileURL, options: .atomic)
            excludeDownloadItemFromBackup(fileURL)
        } catch {
            iosHostLog("download state persistence failed: \(error.localizedDescription)")
        }
    }
}
