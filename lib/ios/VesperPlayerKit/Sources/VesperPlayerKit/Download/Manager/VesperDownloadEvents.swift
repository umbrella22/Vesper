import Combine
import Foundation
internal import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif
public enum VesperDownloadEvent: Equatable {
    case created(VesperDownloadTaskSnapshot)
    case stateChanged(VesperDownloadTaskStatePatch)
    case assetIndexUpdated(VesperDownloadTaskSnapshot)
    case progressUpdated(VesperDownloadTaskProgressPatch)
}

public struct VesperDownloadEventBatch: Equatable {
    public let events: [VesperDownloadEvent]
    public let droppedEvents: UInt64
    public let requiresSnapshotResync: Bool
    public let snapshotIsAuthoritative: Bool

    public init(
        events: [VesperDownloadEvent],
        droppedEvents: UInt64,
        requiresSnapshotResync: Bool = false,
        snapshotIsAuthoritative: Bool = true
    ) {
        self.events = events
        self.droppedEvents = droppedEvents
        self.requiresSnapshotResync = requiresSnapshotResync
        self.snapshotIsAuthoritative = snapshotIsAuthoritative
    }
}

extension VesperDownloadEvent {
    var isRemovedStatePatch: Bool {
        if case let .stateChanged(patch) = self {
            return patch.state == .removed
        }
        return false
    }
}
