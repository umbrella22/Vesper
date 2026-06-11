import Foundation

public final class VesperForegroundDownloadExecutor: VesperDownloadExecutor {
    let lock = NSLock()
    let fileManager = FileManager.default
    var tasks: [VesperDownloadTaskId: Task<Void, Never>] = [:]
    var recoveredSources: [VesperDownloadTaskId: VesperDownloadSource] = [:]
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

}
