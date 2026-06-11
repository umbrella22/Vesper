import Foundation

public enum VesperPlaybackCapabilityProbeStatus: String, Equatable {
    case supported
    case fallbackRequired
    case unsupported
    case unknown
}

public enum VesperPlaybackCodecFamily: String, Equatable {
    case h264
    case hevc
    case av1
    case vvc
    case unknown
}

public enum VesperPlaybackCapabilityOutputFormat: String, Equatable {
    case nv12
    case p010
    case surfaceOpaque
    case unknown
}

public enum VesperPlaybackCapabilityHdrKind: String, Equatable {
    case none
    case hdr10
    case hlg
    case dolbyVision
    case unknown
}

public enum VesperPlaybackCapabilityDolbyVisionMode: String, Equatable {
    case none
    case fullChainCandidate
    case compatibleBaseLayer
    case unsupported
}

public enum VesperPlaybackCapabilityConfidence: String, Equatable {
    case codecOnly
    case sourceMetadata
    case sessionProbe
}

public enum VesperRecommendedPlaybackPath: String, Equatable {
    case nativeFramePipeline
    case systemPlayer
}

public struct VesperPlaybackCapabilitySessionProbeResult: Equatable {
    public let supportedHdrKinds: Set<VesperPlaybackCapabilityHdrKind>
    public let diagnostics: [String: String]

    public init(
        supportedHdrKinds: Set<VesperPlaybackCapabilityHdrKind> = [],
        diagnostics: [String: String] = [:]
    ) {
        self.supportedHdrKinds = supportedHdrKinds
        self.diagnostics = diagnostics
    }
}

struct VesperPlaybackCapabilityAssetProbeResult: Equatable {
    let isPlayable: Bool?
    let videoTrackCount: Int?
    let metadataHdrKind: VesperPlaybackCapabilityHdrKind?
    let diagnostics: [String: String]

    init(
        isPlayable: Bool? = nil,
        videoTrackCount: Int? = nil,
        metadataHdrKind: VesperPlaybackCapabilityHdrKind? = nil,
        diagnostics: [String: String] = [:]
    ) {
        self.isPlayable = isPlayable
        self.videoTrackCount = videoTrackCount
        self.metadataHdrKind = metadataHdrKind
        self.diagnostics = diagnostics
    }
}

public struct VesperPlaybackCapabilityProbeRequest: Equatable {
    public let source: VesperPlayerSource?
    public let codec: String?
    public let width: Int?
    public let height: Int?
    public let frameRate: Double?
    public let requiresNativeFrame: Bool
    public let sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration
    public let frameProcessorConfiguration: VesperFrameProcessorConfiguration
    public let nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration

    public init(
        source: VesperPlayerSource? = nil,
        codec: String? = nil,
        width: Int? = nil,
        height: Int? = nil,
        frameRate: Double? = nil,
        requiresNativeFrame: Bool = false,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration()
    ) {
        self.source = source
        self.codec = codec
        self.width = width
        self.height = height
        self.frameRate = frameRate
        self.requiresNativeFrame = requiresNativeFrame
        self.sourceNormalizerConfiguration = sourceNormalizerConfiguration
        self.frameProcessorConfiguration = frameProcessorConfiguration
        self.nativeFramePipelineConfiguration = nativeFramePipelineConfiguration
    }
}
public struct VesperPlaybackCapabilityProbeResult: Equatable {
    public let status: VesperPlaybackCapabilityProbeStatus
    public let codecFamily: VesperPlaybackCodecFamily
    public let systemPlaybackSupported: Bool
    public let hardwareDecodeSupported: Bool
    public let sdkManagedNativeFrameSupported: Bool
    public let recommendedPlaybackPath: VesperRecommendedPlaybackPath
    public let outputFormat: VesperPlaybackCapabilityOutputFormat
    public let hdrKind: VesperPlaybackCapabilityHdrKind
    public let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode
    public let confidence: VesperPlaybackCapabilityConfidence
    public let missingCapabilities: [String]
    public let diagnostics: [String: String]
    public let hdrMetadata: VesperPlaybackCapabilityHdrMetadata?

    public init(
        status: VesperPlaybackCapabilityProbeStatus,
        codecFamily: VesperPlaybackCodecFamily,
        systemPlaybackSupported: Bool,
        hardwareDecodeSupported: Bool,
        sdkManagedNativeFrameSupported: Bool,
        recommendedPlaybackPath: VesperRecommendedPlaybackPath,
        outputFormat: VesperPlaybackCapabilityOutputFormat,
        hdrKind: VesperPlaybackCapabilityHdrKind,
        dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode,
        confidence: VesperPlaybackCapabilityConfidence,
        missingCapabilities: [String] = [],
        diagnostics: [String: String] = [:],
        hdrMetadata: VesperPlaybackCapabilityHdrMetadata? = nil
    ) {
        self.status = status
        self.codecFamily = codecFamily
        self.systemPlaybackSupported = systemPlaybackSupported
        self.hardwareDecodeSupported = hardwareDecodeSupported
        self.sdkManagedNativeFrameSupported = sdkManagedNativeFrameSupported
        self.recommendedPlaybackPath = recommendedPlaybackPath
        self.outputFormat = outputFormat
        self.hdrKind = hdrKind
        self.dolbyVisionMode = dolbyVisionMode
        self.confidence = confidence
        self.missingCapabilities = missingCapabilities
        self.diagnostics = diagnostics
        self.hdrMetadata = hdrMetadata
    }

    public var wireMap: [String: Any] {
        var map: [String: Any] = [
            "status": status.rawValue,
            "codecFamily": codecFamily.rawValue,
            "systemPlaybackSupported": systemPlaybackSupported,
            "hardwareDecodeSupported": hardwareDecodeSupported,
            "sdkManagedNativeFrameSupported": sdkManagedNativeFrameSupported,
            "recommendedPlaybackPath": recommendedPlaybackPath.rawValue,
            "outputFormat": outputFormat.rawValue,
            "hdrKind": hdrKind.rawValue,
            "dolbyVisionMode": dolbyVisionMode.rawValue,
            "confidence": confidence.rawValue,
            "missingCapabilities": missingCapabilities,
            "diagnostics": diagnostics,
        ]
        map["hdrMetadata"] = hdrMetadata?.wireMap ?? NSNull()
        return map
    }
}
