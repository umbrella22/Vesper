import Combine
import Foundation

@MainActor
public final class VesperDownloadManager: ObservableObject {
    @Published public internal(set) var snapshot: VesperDownloadSnapshot

    let executor: any VesperDownloadExecutor
    let bindings: any DownloadBindings
    let configuration: VesperDownloadConfiguration
    let stateStore: VesperDownloadStateStore?
    let taskStore = DownloadTaskStore()
    var eventBuffer: [VesperDownloadEvent] = []
    let maxEventBufferCapacity = 1_000
    var lastProgressPersistence: [VesperDownloadTaskId: (bytes: UInt64, date: Date)] = [:]
    var sessionHandle: UInt64 = 0

    public init(
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: (any VesperDownloadExecutor)? = nil,
        staleResourceRecoveryHandler: (@Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadSource?)? = nil,
        staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler? = nil
    ) {
        self.configuration = configuration
        self.executor = executor ?? VesperForegroundDownloadExecutor(
            baseDirectory: configuration.baseDirectory,
            resumePartialDownloads: configuration.resumePartialDownloads,
            rangeChunkBytes: configuration.rangeChunkBytes,
            minProgressBytes: configuration.minProgressBytes,
            minProgressIntervalMs: configuration.minProgressIntervalMs,
            stalledTransferTimeoutMs: configuration.stalledTransferTimeoutMs,
            staleResourceRecoveryHandler: staleResourceRecoveryHandler,
            staleResourcePlanRecoveryHandler: staleResourcePlanRecoveryHandler
        )
        bindings = NativeDownloadBindings()
        let stateStoreURL = Self.stateStoreURL(for: configuration)
        stateStore = configuration.restoreTasksOnStartup
            ? VesperDownloadStateStore(fileURL: stateStoreURL)
            : nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        excludeDownloadItemFromBackup(stateStoreURL.deletingLastPathComponent())
        sessionHandle = bindings.createDownloadSession(configuration: configuration)
        if sessionHandle == 0 {
            iosHostLog("native download session creation failed")
        }
        restorePersistedTasks()
        forceFullSync()
    }

    internal init(
        configuration: VesperDownloadConfiguration,
        executor: any VesperDownloadExecutor,
        bindings: any DownloadBindings
    ) {
        self.configuration = configuration
        self.executor = executor
        self.bindings = bindings
        stateStore = nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        sessionHandle = bindings.createDownloadSession(configuration: configuration)
        if sessionHandle == 0 {
            iosHostLog("native download session creation failed")
        }
        forceFullSync()
    }

    deinit {
        if sessionHandle != 0 {
            bindings.disposeDownloadSession(sessionHandle)
        }
    }

    public func dispose() {
        snapshot.tasks
            .filter { $0.state == .preparing || $0.state == .downloading }
            .forEach { _ = pauseTask($0.taskId) }
        persistSnapshot(snapshot)
        executor.dispose()
        if sessionHandle != 0 {
            bindings.disposeDownloadSession(sessionHandle)
            sessionHandle = 0
        }
        eventBuffer.removeAll(keepingCapacity: false)
        taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
        lastProgressPersistence.removeAll(keepingCapacity: false)
        snapshot = VesperDownloadSnapshot(tasks: [])
    }

    public func refresh() {
        syncRuntimeState(processCommands: true)
    }

    public func forceFullSync() {
        forceFullSync(processCommands: true)
    }

    public func drainEvents() -> [VesperDownloadEvent] {
        let events = eventBuffer
        eventBuffer.removeAll(keepingCapacity: true)
        return events
    }

    public func task(_ taskId: VesperDownloadTaskId) -> VesperDownloadTaskSnapshot? {
        snapshot.tasks.first(where: { $0.taskId == taskId })
    }

    public func tasks(forAsset assetId: VesperDownloadAssetId) -> [VesperDownloadTaskSnapshot] {
        snapshot.tasks.filter { $0.assetId == assetId }
    }

}
