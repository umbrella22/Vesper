@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

enum AbrPolicyOrigin: String {
    case manual
    case defaultPolicy
    case resilienceRestore
    case recoveredFallback
}

struct FixedTrackConvergenceState {
    let requestedTrackId: String
    let origin: AbrPolicyOrigin
    var lockedStartedAt: Date?
    var mismatchSignature: FixedTrackMismatchSignature?
    var mismatchStartedAt: Date?
    var hasHandledPersistentMismatch = false

    mutating func resetLocked() {
        lockedStartedAt = nil
    }

    mutating func resetMismatch() {
        mismatchSignature = nil
        mismatchStartedAt = nil
        hasHandledPersistentMismatch = false
    }
}

struct FixedTrackMismatchSignature: Equatable {
    let effectiveVideoTrackId: String?
    let bitRate: Int64?
    let width: Int?
    let height: Int?

    init(
        effectiveVideoTrackId: String?,
        observation: VesperVideoVariantObservation?
    ) {
        self.effectiveVideoTrackId = effectiveVideoTrackId
        bitRate = observation?.bitRate
        width = observation?.width
        height = observation?.height
    }
}

enum SubtitleSelectionOrigin {
    case explicit
    case defaultPolicy
    case resilienceRestore
    case visibilityRestore

    func canSupersede(_ pendingOrigin: SubtitleSelectionOrigin) -> Bool {
        switch self {
        case .explicit:
            return true
        case .resilienceRestore:
            return pendingOrigin != .explicit
        case .defaultPolicy:
            return pendingOrigin == .defaultPolicy || pendingOrigin == .visibilityRestore
        case .visibilityRestore:
            return pendingOrigin == .visibilityRestore
        }
    }
}

struct PendingSubtitleSelection {
    let commandId: UInt64
    let sourceEpoch: UInt64
    let playbackEpoch: UInt64
    let item: AVPlayerItem
    let selection: VesperTrackSelection
    let origin: SubtitleSelectionOrigin
}

struct VesperSubtitleSelectionWaitPolicy: Equatable, Sendable {
    let timeout: Duration
    let pollInterval: Duration

    static let production = VesperSubtitleSelectionWaitPolicy(
        timeout: .seconds(3),
        pollInterval: .milliseconds(50)
    )
}
