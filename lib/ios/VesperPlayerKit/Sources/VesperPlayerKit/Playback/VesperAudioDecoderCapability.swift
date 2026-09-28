import Foundation

public struct VesperAudioDecoderCapabilityRequest: Equatable, Codable, Sendable {
    public let sampleMimeType: String?
    public let codec: String?
    public let channels: Int?
    public let sampleRate: Int?
    public init(sampleMimeType: String? = nil, codec: String? = nil, channels: Int? = nil, sampleRate: Int? = nil) {
        self.sampleMimeType = sampleMimeType
        self.codec = codec
        self.channels = channels
        self.sampleRate = sampleRate
    }
}

public struct VesperAudioDecoderSupport: RawRepresentable, Equatable, Codable, Sendable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let supported = Self(rawValue: "supported")
    public static let unsupported = Self(rawValue: "unsupported")
    public static let unknown = Self(rawValue: "unknown")
    public init(from decoder: Decoder) throws { rawValue = try decoder.singleValueContainer().decode(String.self) }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public struct VesperAudioDecoderCandidate: Equatable, Codable, Sendable {
    public let name: String
    public let hardwareAccelerated: Bool?
    public let status: VesperAudioDecoderSupport
    public let reason: String
}

/// Decoder evidence is separate from container, DRM, route and audible-output support.
public struct VesperAudioDecoderCapabilityResult: Equatable, Codable, Sendable {
    public let request: VesperAudioDecoderCapabilityRequest
    public let status: VesperAudioDecoderSupport
    public let reason: String
    public let resolvedMimeType: String?
    public let candidates: [VesperAudioDecoderCandidate]
    public let evidence: String
}

extension VesperPlayerControllerFactory {
    /// AVPlayer has no public full audio-decoder format query. No codec allowlist
    /// or AudioToolbox enumeration can establish AVPlayer playback support.
    public static func probeAudioDecoderCapability(_ request: VesperAudioDecoderCapabilityRequest) throws -> VesperAudioDecoderCapabilityResult {
        guard request.channels.map({ $0 > 0 }) ?? true, request.sampleRate.map({ $0 > 0 }) ?? true else {
            throw VesperPlayerError(message: "Audio channel count and sample rate must be positive", code: .invalidArgument, category: .input, retriable: false)
        }
        return VesperAudioDecoderCapabilityResult(request: request, status: .unknown,
            reason: "avPlayerAudioDecoderQueryUnavailable", resolvedMimeType: nil, candidates: [], evidence: "unavailable")
    }
}
