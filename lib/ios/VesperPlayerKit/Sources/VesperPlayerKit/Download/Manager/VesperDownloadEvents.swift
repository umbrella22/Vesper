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

extension VesperDownloadEvent {
    var isRemovedStatePatch: Bool {
        if case let .stateChanged(patch) = self {
            return patch.state == .removed
        }
        return false
    }
}
