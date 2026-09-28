import Foundation

/// At most one suspected playback stall is retained per native load attempt.
public struct VesperPlaybackStallPolicy: Equatable, Sendable {
    public let enabled: Bool
    public let positionThresholdMs: UInt64
    public let bufferingThresholdMs: UInt64
    public init(enabled: Bool = true, positionThresholdMs: UInt64 = 5_000, bufferingThresholdMs: UInt64 = 15_000) {
        self.enabled = enabled
        self.positionThresholdMs = positionThresholdMs
        self.bufferingThresholdMs = bufferingThresholdMs
    }
}

public struct VesperPlaybackStallKind: RawRepresentable, Equatable, Codable, Sendable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let positionNotAdvancing = Self(rawValue: "positionNotAdvancing")
    public static let bufferingTimeout = Self(rawValue: "bufferingTimeout")
    public init(from decoder: Decoder) throws { rawValue = try decoder.singleValueContainer().decode(String.self) }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

/// Historical media-clock evidence. This does not identify an audio decoder or sink failure.
public struct VesperPlaybackStallObservation: Equatable, Codable, Sendable {
    public let playbackEpoch: UInt64
    public let kind: VesperPlaybackStallKind
    public let stalledForMs: UInt64
    public let elapsedSinceLoadStartMs: UInt64
    public let mediaPositionMs: Int64
    public let audio: VesperAudioPlaybackDiagnostics
}

struct VesperPlaybackStallDetector {
    var policy = VesperPlaybackStallPolicy() { didSet { resetWindow() } }
    private var lastSampleMs: UInt64?
    private var lastPositionMs: Int64?
    private var windowStartMs: UInt64?
    private var windowKind: VesperPlaybackStallKind?
    private var hasProgressed = false
    private var reported = false

    mutating func resetAttempt() { reported = false; resetWindow() }
    mutating func resetWindow() {
        lastSampleMs = nil
        lastPositionMs = nil
        windowStartMs = nil
        windowKind = nil
        hasProgressed = false
    }

    mutating func sample(nowMs: UInt64, positionMs: Int64?, eligible: Bool, buffering: Bool)
        -> (kind: VesperPlaybackStallKind, durationMs: UInt64)? {
        guard policy.enabled, eligible, let positionMs, positionMs >= 0 else {
            resetWindow()
            return nil
        }
        if let lastSampleMs, nowMs < lastSampleMs || nowMs - lastSampleMs > 3_000 {
            resetWindow()
        }
        lastSampleMs = nowMs
        let previousPosition = lastPositionMs
        lastPositionMs = positionMs
        let kind: VesperPlaybackStallKind = buffering ? .bufferingTimeout : .positionNotAdvancing
        guard let previousPosition, positionMs >= previousPosition else {
            hasProgressed = false
            windowStartMs = nowMs
            windowKind = kind
            return nil
        }
        if positionMs > previousPosition {
            hasProgressed = true
            windowStartMs = nowMs
            windowKind = kind
            return nil
        }
        if windowKind != kind { windowKind = kind; windowStartMs = nowMs }
        guard hasProgressed, !reported else { return nil }
        let duration = nowMs - (windowStartMs ?? nowMs)
        let threshold = buffering ? policy.bufferingThresholdMs : policy.positionThresholdMs
        guard duration >= threshold else { return nil }
        reported = true
        return (kind, duration)
    }
}
