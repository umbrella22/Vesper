import Combine
import Foundation

/// Observed display output. Capability and decoder metadata cannot confirm this state.
public enum VesperHdrOutputState: String, Sendable {
    case unknown, sdr, hdr
}

public enum VesperHdrOutputFormat: String, Sendable {
    case unknown, hdr10, hlg, dolbyVision
}

/// Output evidence and its native lifecycle context. Revisions are local to one
/// controller; sourceRevision counts activations, including repeated URLs.
/// iOS currently reports unknown because no per-player display observer is enabled.
public struct VesperHdrOutputSnapshot: Equatable, Sendable {
    public internal(set) var state: VesperHdrOutputState = .unknown
    public internal(set) var format: VesperHdrOutputFormat = .unknown
    public internal(set) var sourceRevision: Int64 = 0
    public internal(set) var outputGeneration: Int64 = 0
    public internal(set) var effectiveVideoTrackId: String?
    public internal(set) var catalogRevision: Int64?
    public internal(set) var displayId: String?
    public internal(set) var evidence: String?
    public internal(set) var reason: String? = "outputObservationUnavailable"
}

struct VesperHdrOutputObservationToken {
    fileprivate let owner: UUID
    let sourceRevision: Int64
    let outputGeneration: Int64
}

struct VesperHdrOutputObservation {
    let state: VesperHdrOutputState
    var format: VesperHdrOutputFormat = .unknown
    var evidence: String?
    var reason: String?
}

@MainActor
final class VesperHdrOutputTracker {
    private let owner = UUID()
    private var disposed = false
    @Published private(set) var snapshot = VesperHdrOutputSnapshot()

    func capture() -> VesperHdrOutputObservationToken {
        VesperHdrOutputObservationToken(
            owner: owner,
            sourceRevision: snapshot.sourceRevision,
            outputGeneration: snapshot.outputGeneration
        )
    }

    func sourceChanged() {
        guard !disposed else { return }
        var context = snapshot
        context.sourceRevision += 1
        context.effectiveVideoTrackId = nil
        context.catalogRevision = nil
        invalidate(context)
    }

    func videoTrackChanged(_ trackId: String?, catalogRevision: Int64?) {
        guard snapshot.effectiveVideoTrackId != trackId || snapshot.catalogRevision != catalogRevision else { return }
        var context = snapshot
        context.effectiveVideoTrackId = trackId
        context.catalogRevision = catalogRevision
        invalidate(context)
    }

    func outputPathChanged() {
        invalidate(snapshot)
    }

    private func invalidate(_ context: VesperHdrOutputSnapshot) {
        guard !disposed else { return }
        var next = context
        next.outputGeneration = snapshot.outputGeneration + 1
        next.state = .unknown
        next.format = .unknown
        next.evidence = nil
        next.reason = "outputObservationUnavailable"
        snapshot = next
    }

    @discardableResult
    func apply(_ token: VesperHdrOutputObservationToken, observation: VesperHdrOutputObservation) -> Bool {
        guard !disposed, token.owner == owner,
              token.sourceRevision == snapshot.sourceRevision,
              token.outputGeneration == snapshot.outputGeneration else { return false }
        guard observation.state == .unknown || observation.evidence?.isEmpty == false else { return false }
        var next = snapshot
        next.state = observation.state
        next.format = observation.state == .hdr ? observation.format : .unknown
        next.evidence = observation.state == .unknown ? nil : observation.evidence
        next.reason = observation.reason
        snapshot = next
        return true
    }

    func dispose() {
        guard !disposed else { return }
        outputPathChanged()
        disposed = true
    }
}
