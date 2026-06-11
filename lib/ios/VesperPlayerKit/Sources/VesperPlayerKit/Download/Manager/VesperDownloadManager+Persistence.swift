import Foundation

extension VesperDownloadManager {
    func restorePersistedTasks() {
        let storedTasks = stateStore?.load().tasks ?? []
        let restorable = storedTasks.filter { $0.state != .removed }
        guard !restorable.isEmpty else {
            return
        }
        let activeTaskIds = restorable
            .filter { $0.state == .preparing || $0.state == .downloading }
            .map(\.taskId)
        let queuedTaskIds = restorable
            .filter { $0.state == .queued }
            .map(\.taskId)
        guard restoreTasks(restorable), configuration.autoStart else {
            return
        }
        activeTaskIds.forEach { _ = resumeTask($0) }
        queuedTaskIds.forEach { _ = startTask($0) }
    }

    func persistSnapshot(_ snapshot: VesperDownloadSnapshot) {
        stateStore?.save(snapshot.compactedForPersistence())
    }

    static func stateStoreURL(for configuration: VesperDownloadConfiguration) -> URL {
        let root = configuration.baseDirectory
            ?? FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
                .appendingPathComponent("vesper-downloads", isDirectory: true)
        return root.appendingPathComponent("download-state.json")
    }
}
