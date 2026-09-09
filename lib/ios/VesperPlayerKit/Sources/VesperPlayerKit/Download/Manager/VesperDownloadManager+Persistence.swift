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
        let restored: Bool
        do {
            restored = try restoreTasks(restorable)
        } catch {
            iosHostLog("download state restore rejected persisted tasks: \(error.localizedDescription)")
            return
        }
        guard restored, configuration.autoStart else {
            return
        }
        activeTaskIds.forEach { _ = resumeTask($0) }
        queuedTaskIds.forEach { _ = startTask($0) }
    }

    func persistSnapshot(_ snapshot: VesperDownloadSnapshot) {
        stateStore?.save(snapshot.compactedForPersistence())
    }

    static func stateStoreURL(for configuration: VesperDownloadConfiguration) -> URL {
        let fileManager = FileManager.default
        let root = resolvedVesperDownloadDirectory(
            baseDirectory: configuration.baseDirectory,
            documentDirectory: fileManager.urls(for: .documentDirectory, in: .userDomainMask).first,
            temporaryDirectory: fileManager.temporaryDirectory
        )
        return root.appendingPathComponent("download-state.json")
    }
}
