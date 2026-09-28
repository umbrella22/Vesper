import Combine
import Foundation

/// Native video evidence. Neither observation proves application UI visibility.
public struct VesperFirstFrameObservationKind: RawRepresentable, Equatable, Codable, Sendable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let media3RenderedFirstFrame = Self(rawValue: "media3RenderedFirstFrame")
    public static let avPlayerLayerReadyForDisplay = Self(rawValue: "avPlayerLayerReadyForDisplay")
    public init(from decoder: Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

/// Monotonic native load duration, independent of the media timeline position.
public struct VesperFirstFrameObservation: Equatable, Codable, Sendable {
    public let playbackEpoch: UInt64
    public let elapsedSinceLoadStartMs: UInt64
    public let kind: VesperFirstFrameObservationKind
    public let mediaPositionMs: Int64?
}

public struct VesperAudioDiagnosticEvidence: RawRepresentable, Equatable, Codable, Sendable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let unknown = Self(rawValue: "unknown")
    public static let runtimeFormat = Self(rawValue: "runtimeFormat")
    public static let selectedMediaOption = Self(rawValue: "selectedMediaOption")
    public static let manifestMetadata = Self(rawValue: "manifestMetadata")
    public init(from decoder: Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperAudioDiagnosticIssueKind: RawRepresentable, Equatable, Codable, Sendable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let decoderError = Self(rawValue: "decoderError")
    public static let sinkError = Self(rawValue: "sinkError")
    public init(from decoder: Decoder) throws {
        rawValue = try decoder.singleValueContainer().decode(String.self)
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

/// A possibly recoverable platform callback, not a terminal playback failure.
public struct VesperAudioDiagnosticIssue: Equatable, Codable, Sendable {
    public let kind: VesperAudioDiagnosticIssueKind
    public let elapsedSinceLoadStartMs: UInt64
    public let platformCode: String?
    public let message: String?
}

/// Selected input evidence. Unknown fields stay nil; this does not observe speakers.
public struct VesperAudioPlaybackDiagnostics: Equatable, Codable, Sendable {
    public var trackId: String?
    public var formatId: String?
    public var codec: String?
    public var sampleMimeType: String?
    public var decoderName: String?
    public var channels: Int?
    public var sampleRate: Int?
    public var evidence: VesperAudioDiagnosticEvidence = .unknown
    public var lastIssue: VesperAudioDiagnosticIssue?

    public init() {}
}

/// Retained observations for a controller-local load attempt. Zero means no attempt.
public struct VesperPlaybackDiagnosticsSnapshot: Equatable, Codable, Sendable {
    public let playbackEpoch: UInt64
    public var audio: VesperAudioPlaybackDiagnostics
    public var firstFrame: VesperFirstFrameObservation?
    public var lastStall: VesperPlaybackStallObservation?

    init(playbackEpoch: UInt64 = 0) {
        self.playbackEpoch = playbackEpoch
        audio = VesperAudioPlaybackDiagnostics()
    }

    // The native error contract carries string details. Flutter decodes this
    // value to the same nested map as Android at its serialization boundary.
    var errorDetails: [String: String] {
        guard let data = try? JSONEncoder().encode(self),
              let json = String(data: data, encoding: .utf8) else { return [:] }
        return ["playbackDiagnostics": json]
    }
}

public extension VesperPlayerError {
    /// Audio and first-frame evidence captured when the failure was classified.
    var playbackDiagnostics: VesperPlaybackDiagnosticsSnapshot? {
        guard let json = details["playbackDiagnostics"],
              let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(VesperPlaybackDiagnosticsSnapshot.self, from: data)
    }
}

struct VesperPlaybackObservationToken: Equatable {
    let owner: UUID
    let epoch: UInt64
}

@MainActor
final class VesperPlaybackDiagnosticsTracker {
    var stallDetector = VesperPlaybackStallDetector()
    private let owner = UUID()
    private let nowMs: () -> UInt64
    private var epoch: UInt64 = 0
    private var startedAtMs: UInt64?
    private var disposed = false
    private let subject = CurrentValueSubject<VesperPlaybackDiagnosticsSnapshot, Never>(.init())
    private(set) var snapshot = VesperPlaybackDiagnosticsSnapshot()
    var publisher: AnyPublisher<VesperPlaybackDiagnosticsSnapshot, Never> {
        // A subscriber may synchronously replace the load. Other subscribers
        // must not receive the superseded value after that nested publication.
        subject.filter { [weak self] value in
            guard let self else { return false }
            return !self.disposed && self.snapshot == value
        }.eraseToAnyPublisher()
    }

    init(nowMs: @escaping () -> UInt64 = { DispatchTime.now().uptimeNanoseconds / 1_000_000 }) {
        self.nowMs = nowMs
    }

    @discardableResult
    func beginAttempt() -> VesperPlaybackObservationToken {
        guard !disposed else { return capture() }
        stallDetector.resetAttempt()
        epoch &+= 1
        startedAtMs = nowMs()
        let token = capture()
        publish(.init(playbackEpoch: epoch))
        return token
    }

    func invalidate() {
        guard !disposed else { return }
        stallDetector.resetAttempt()
        epoch &+= 1
        startedAtMs = nil
        publish(.init(playbackEpoch: epoch))
    }

    /// Retains failure evidence while rejecting callbacks from released playback.
    func endAttempt() { startedAtMs = nil }

    func capture() -> VesperPlaybackObservationToken {
        VesperPlaybackObservationToken(owner: owner, epoch: epoch)
    }

    func isCurrent(_ token: VesperPlaybackObservationToken) -> Bool {
        !disposed && startedAtMs != nil && token.owner == owner && token.epoch == epoch
    }

    @discardableResult
    func firstFrame(_ token: VesperPlaybackObservationToken, mediaPositionMs: Int64?) -> Bool {
        guard isCurrent(token), snapshot.firstFrame == nil, let startedAtMs else { return false }
        let now = nowMs()
        var value = snapshot
        value.firstFrame = VesperFirstFrameObservation(
            playbackEpoch: token.epoch,
            elapsedSinceLoadStartMs: now >= startedAtMs ? now - startedAtMs : 0,
            kind: .avPlayerLayerReadyForDisplay,
            mediaPositionMs: mediaPositionMs
        )
        publish(value)
        return true
    }

    func audio(_ token: VesperPlaybackObservationToken, value: VesperAudioPlaybackDiagnostics) {
        guard isCurrent(token) else { return }
        var next = snapshot
        next.audio = value
        publish(next)
    }

    func sampleStall(_ token: VesperPlaybackObservationToken, positionMs: Int64?, eligible: Bool, buffering: Bool) {
        guard isCurrent(token), let startedAtMs else { return }
        let now = nowMs()
        guard let evidence = stallDetector.sample(nowMs: now, positionMs: positionMs, eligible: eligible, buffering: buffering),
              let positionMs else { return }
        var value = snapshot
        value.lastStall = VesperPlaybackStallObservation(playbackEpoch: token.epoch, kind: evidence.kind,
            stalledForMs: evidence.durationMs, elapsedSinceLoadStartMs: now >= startedAtMs ? now - startedAtMs : 0,
            mediaPositionMs: positionMs, audio: value.audio)
        publish(value)
    }

    func dispose() {
        guard !disposed else { return }
        disposed = true
        startedAtMs = nil
        subject.send(completion: .finished)
    }

    private func publish(_ value: VesperPlaybackDiagnosticsSnapshot) {
        guard value != snapshot else { return }
        snapshot = value
        subject.send(value)
    }
}
