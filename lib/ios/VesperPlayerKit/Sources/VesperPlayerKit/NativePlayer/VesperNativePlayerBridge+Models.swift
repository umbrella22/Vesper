@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

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
