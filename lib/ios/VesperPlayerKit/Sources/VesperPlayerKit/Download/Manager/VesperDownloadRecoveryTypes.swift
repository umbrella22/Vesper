import Combine
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif
public enum VesperDownloadStaleResourcePhase: String, Equatable, Codable {
    case prepare
    case download
}

public struct VesperDownloadStaleResource: Equatable {
    public let taskId: VesperDownloadTaskId
    public let resourceId: String?
    public let segmentId: String?
    public let uri: String?
    public let phase: VesperDownloadStaleResourcePhase
    public let statusCode: Int?
    public let receivedBytes: UInt64
    public let message: String

    public init(
        taskId: VesperDownloadTaskId,
        resourceId: String? = nil,
        segmentId: String? = nil,
        uri: String? = nil,
        phase: VesperDownloadStaleResourcePhase = .prepare,
        statusCode: Int? = nil,
        receivedBytes: UInt64 = 0,
        message: String
    ) {
        self.taskId = taskId
        self.resourceId = resourceId
        self.segmentId = segmentId
        self.uri = uri
        self.phase = phase
        self.statusCode = statusCode
        self.receivedBytes = receivedBytes
        self.message = message
    }
}

public struct VesperDownloadRecoveredTaskPlan: Equatable {
    public let source: VesperDownloadSource
    public let profile: VesperDownloadProfile
    public let assetIndex: VesperDownloadAssetIndex

    public init(
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) {
        self.source = source
        self.profile = profile
        self.assetIndex = assetIndex
    }
}

@available(*, deprecated, message: "Use VesperDownloadStaleResourcePlanRecoveryHandler to refresh source, profile, and asset index together.")
public typealias VesperDownloadStaleResourceRecoveryHandler =
    @Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadSource?

public typealias VesperDownloadStaleResourcePlanRecoveryHandler =
    @Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadRecoveredTaskPlan?
