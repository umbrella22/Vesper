import Foundation

public final class VesperForegroundDownloadExecutor: VesperDownloadExecutor {
    let lock = NSLock()
    let fileManager = FileManager.default
    struct Execution {
        let id: UUID
        let work: Task<Void, Never>
    }

    var tasks: [VesperDownloadTaskId: Execution] = [:]
    var recoveredSources: [VesperDownloadTaskId: VesperDownloadSource] = [:]
    var httpSessionConfiguration: URLSessionConfiguration?
    let baseDirectory: URL?
    let resumePartialDownloads: Bool
    let rangeChunkBytes: UInt64?
    let minProgressBytes: UInt64
    let minProgressIntervalMs: UInt64
    let stalledTransferTimeoutMs: UInt64
    let staleResourceRecoveryHandler: (@Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadSource?)?
    let staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler?

    public init(
        baseDirectory: URL? = nil,
        resumePartialDownloads: Bool = true,
        rangeChunkBytes: UInt64? = nil,
        minProgressBytes: UInt64 = vesperDownloadDefaultMinProgressBytes,
        minProgressIntervalMs: UInt64 = vesperDownloadDefaultMinProgressIntervalMs,
        stalledTransferTimeoutMs: UInt64 = vesperDownloadDefaultStalledTransferTimeoutMs,
        staleResourceRecoveryHandler: (@Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadSource?)? = nil,
        staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler? = nil
    ) {
        self.baseDirectory = baseDirectory
        self.resumePartialDownloads = resumePartialDownloads
        self.rangeChunkBytes = rangeChunkBytes.flatMap { $0 > 0 ? $0 : nil }
        self.minProgressBytes = max(minProgressBytes, 1)
        self.minProgressIntervalMs = minProgressIntervalMs
        self.stalledTransferTimeoutMs = stalledTransferTimeoutMs
        self.staleResourceRecoveryHandler = staleResourceRecoveryHandler
        self.staleResourcePlanRecoveryHandler = staleResourcePlanRecoveryHandler
    }

    // Publish ownership before work can finish or call back into the manager.
    func launchExecution(
        taskId: VesperDownloadTaskId,
        clearsRecoveredSource: Bool,
        operation: @escaping @Sendable () async -> Void
    ) {
        let id = UUID()
        lock.lock()
        let previous = tasks[taskId]?.work
        let work = Task.detached(priority: .utility) { [weak self] in
            guard let self else { return }
            defer {
                self.finishExecution(taskId: taskId, id: id, clearsRecoveredSource: clearsRecoveredSource)
            }
            guard self.isCurrentExecution(taskId: taskId, id: id), !Task.isCancelled else { return }
            await operation()
        }
        tasks[taskId] = Execution(id: id, work: work)
        lock.unlock()
        previous?.cancel()
    }

    private func isCurrentExecution(taskId: VesperDownloadTaskId, id: UUID) -> Bool {
        lock.withLock { tasks[taskId]?.id == id }
    }

    private func finishExecution(taskId: VesperDownloadTaskId, id: UUID, clearsRecoveredSource: Bool) {
        lock.withLock {
            guard tasks[taskId]?.id == id else { return }
            tasks.removeValue(forKey: taskId)
            if clearsRecoveredSource {
                recoveredSources.removeValue(forKey: taskId)
            }
        }
    }
}
