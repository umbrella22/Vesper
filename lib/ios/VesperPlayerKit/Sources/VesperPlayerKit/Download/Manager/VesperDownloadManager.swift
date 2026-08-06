import Combine
import Foundation

public enum VesperDownloadManagerInitializationError: LocalizedError, Equatable, Sendable {
    case pluginConfiguration(String)
    case nativeSessionCreation(String)
    case invalidSessionHandle

    public var errorDescription: String? {
        switch self {
        case let .pluginConfiguration(message):
            "Download plugin configuration failed: \(message)"
        case let .nativeSessionCreation(message):
            message
        case .invalidSessionHandle:
            "Native download session creation returned an invalid zero handle."
        }
    }
}

@MainActor
public final class VesperDownloadManager: ObservableObject {
    @Published public internal(set) var snapshot: VesperDownloadSnapshot
    /// A sanitized diagnostic set when native command validation quarantines command processing.
    @Published public internal(set) var runtimeCommandDiagnostic: String?

    let executor: any VesperDownloadExecutor
    let bindings: any DownloadBindings
    let configuration: VesperDownloadConfiguration
    let stateStore: VesperDownloadStateStore?
    let taskStore = DownloadTaskStore()
    var eventBuffer: [VesperDownloadEvent] = []
    var droppedBufferedEvents: UInt64 = 0
    var pendingSnapshotResync = false
    let maxEventBufferCapacity = 1_000
    let maxRuntimeCommandBatchesPerSync = 16
    var lastProgressPersistence: [VesperDownloadTaskId: (bytes: UInt64, date: Date)] = [:]
    var needsAuthoritativeSnapshotResync = false
    var isProcessingRuntimeCommands = false
    var pendingRuntimeCommandAcknowledgementCount: UInt = 0
    var sessionHandle: UInt64 = 0

    public init(
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: (any VesperDownloadExecutor)? = nil,
        staleResourceRecoveryHandler: (@Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadSource?)? = nil,
        staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler? = nil
    ) throws {
        let resolvedExecutor = executor ?? VesperForegroundDownloadExecutor(
            baseDirectory: configuration.baseDirectory,
            resumePartialDownloads: configuration.resumePartialDownloads,
            rangeChunkBytes: configuration.rangeChunkBytes,
            minProgressBytes: configuration.minProgressBytes,
            minProgressIntervalMs: configuration.minProgressIntervalMs,
            stalledTransferTimeoutMs: configuration.stalledTransferTimeoutMs,
            staleResourceRecoveryHandler: staleResourceRecoveryHandler,
            staleResourcePlanRecoveryHandler: staleResourcePlanRecoveryHandler
        )
        let resolvedBindings = NativeDownloadBindings()
        let createdSessionHandle = try Self.createSession(
            configuration: configuration,
            executor: resolvedExecutor,
            bindings: resolvedBindings
        )
        self.configuration = configuration
        self.executor = resolvedExecutor
        bindings = resolvedBindings
        let stateStoreURL = Self.stateStoreURL(for: configuration)
        stateStore = configuration.restoreTasksOnStartup
            ? VesperDownloadStateStore(fileURL: stateStoreURL)
            : nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        runtimeCommandDiagnostic = nil
        sessionHandle = createdSessionHandle
        excludeDownloadItemFromBackup(stateStoreURL.deletingLastPathComponent())
        restorePersistedTasks()
        forceFullSync()
    }

    internal init(
        configuration: VesperDownloadConfiguration,
        executor: any VesperDownloadExecutor,
        bindings: any DownloadBindings
    ) throws {
        let createdSessionHandle = try Self.createSession(
            configuration: configuration,
            executor: executor,
            bindings: bindings
        )
        self.configuration = configuration
        self.executor = executor
        self.bindings = bindings
        stateStore = nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        runtimeCommandDiagnostic = nil
        sessionHandle = createdSessionHandle
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
        droppedBufferedEvents = 0
        pendingSnapshotResync = false
        taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
        lastProgressPersistence.removeAll(keepingCapacity: false)
        needsAuthoritativeSnapshotResync = false
        isProcessingRuntimeCommands = false
        pendingRuntimeCommandAcknowledgementCount = 0
        runtimeCommandDiagnostic = nil
        snapshot = VesperDownloadSnapshot(tasks: [])
    }

    public func refresh() {
        syncRuntimeState(processCommands: true)
    }

    public func forceFullSync() {
        forceFullSync(processCommands: true)
    }

    public func drainEvents() -> VesperDownloadEventBatch {
        let requiresSnapshotResync = pendingSnapshotResync
        let snapshotIsAuthoritative = !needsAuthoritativeSnapshotResync
        let batch = VesperDownloadEventBatch(
            events: requiresSnapshotResync ? [] : eventBuffer,
            droppedEvents: requiresSnapshotResync
                ? saturatingAdd(droppedBufferedEvents, UInt64(eventBuffer.count))
                : droppedBufferedEvents,
            requiresSnapshotResync: requiresSnapshotResync,
            snapshotIsAuthoritative: snapshotIsAuthoritative
        )
        if snapshotIsAuthoritative {
            eventBuffer.removeAll(keepingCapacity: true)
            droppedBufferedEvents = 0
            pendingSnapshotResync = false
        }
        return batch
    }

    public func task(_ taskId: VesperDownloadTaskId) -> VesperDownloadTaskSnapshot? {
        snapshot.tasks.first(where: { $0.taskId == taskId })
    }

    public func tasks(forAsset assetId: VesperDownloadAssetId) -> [VesperDownloadTaskSnapshot] {
        snapshot.tasks.filter { $0.assetId == assetId }
    }

    private static func createSession(
        configuration: VesperDownloadConfiguration,
        executor: any VesperDownloadExecutor,
        bindings: any DownloadBindings
    ) throws -> UInt64 {
        do {
            let handle = try bindings.createDownloadSession(configuration: configuration)
            guard handle != 0 else {
                throw VesperDownloadManagerInitializationError.invalidSessionHandle
            }
            return handle
        } catch {
            executor.dispose()
            throw error
        }
    }

}
