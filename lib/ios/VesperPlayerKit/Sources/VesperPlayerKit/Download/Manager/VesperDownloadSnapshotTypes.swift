import Combine
import Foundation
import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif
public struct VesperDownloadProgressSnapshot: Equatable, Codable {
    public let receivedBytes: UInt64
    public let totalBytes: UInt64?
    public let receivedSegments: UInt32
    public let totalSegments: UInt32?

    public init(
        receivedBytes: UInt64 = 0,
        totalBytes: UInt64? = nil,
        receivedSegments: UInt32 = 0,
        totalSegments: UInt32? = nil
    ) {
        self.receivedBytes = receivedBytes
        self.totalBytes = totalBytes
        self.receivedSegments = receivedSegments
        self.totalSegments = totalSegments
    }

    public var completionRatio: Double? {
        guard let totalBytes, totalBytes > 0 else {
            return nil
        }
        return Double(receivedBytes) / Double(totalBytes)
    }
}

public enum VesperDownloadState: Int, Equatable, Codable {
    case queued = 0
    case preparing = 1
    case downloading = 2
    case paused = 3
    case completed = 4
    case failed = 5
    case removed = 6
}

public struct VesperDownloadError: Equatable, Codable {
    public let code: VesperPlayerErrorCode
    public let category: VesperPlayerErrorCategory
    public let retriable: Bool
    public let message: String

    public init(
        code: VesperPlayerErrorCode,
        category: VesperPlayerErrorCategory,
        retriable: Bool,
        message: String
    ) {
        self.code = code
        self.category = category
        self.retriable = retriable
        self.message = message
    }
}

public struct VesperDownloadTaskSnapshot: Equatable, Codable {
    public let taskId: VesperDownloadTaskId
    public let assetId: VesperDownloadAssetId
    public let source: VesperDownloadSource
    public let profile: VesperDownloadProfile
    public let state: VesperDownloadState
    public let progress: VesperDownloadProgressSnapshot
    public let assetIndex: VesperDownloadAssetIndex
    public let error: VesperDownloadError?

    public init(
        taskId: VesperDownloadTaskId,
        assetId: VesperDownloadAssetId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        state: VesperDownloadState,
        progress: VesperDownloadProgressSnapshot,
        assetIndex: VesperDownloadAssetIndex,
        error: VesperDownloadError? = nil
    ) {
        self.taskId = taskId
        self.assetId = assetId
        self.source = source
        self.profile = profile
        self.state = state
        self.progress = progress
        self.assetIndex = assetIndex
        self.error = error
    }
}

public struct VesperDownloadSnapshot: Equatable, Codable {
    public let tasks: [VesperDownloadTaskSnapshot]

    public init(tasks: [VesperDownloadTaskSnapshot]) {
        self.tasks = tasks
    }
}

public struct VesperDownloadTaskStatePatch: Equatable {
    public let taskId: VesperDownloadTaskId
    public let state: VesperDownloadState
    public let progress: VesperDownloadProgressSnapshot
    public let error: VesperDownloadError?
    public let completedPath: String?

    public init(
        taskId: VesperDownloadTaskId,
        state: VesperDownloadState,
        progress: VesperDownloadProgressSnapshot,
        error: VesperDownloadError? = nil,
        completedPath: String? = nil
    ) {
        self.taskId = taskId
        self.state = state
        self.progress = progress
        self.error = error
        self.completedPath = completedPath
    }
}

public struct VesperDownloadTaskProgressPatch: Equatable {
    public let taskId: VesperDownloadTaskId
    public let progress: VesperDownloadProgressSnapshot

    public init(taskId: VesperDownloadTaskId, progress: VesperDownloadProgressSnapshot) {
        self.taskId = taskId
        self.progress = progress
    }
}
