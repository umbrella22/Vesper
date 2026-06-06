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

public struct VesperPlaybackCapabilityProbeRequest: Equatable {
    public let source: VesperPlayerSource?
    public let codec: String?
    public let requiresNativeFrame: Bool
    public let sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration
    public let frameProcessorConfiguration: VesperFrameProcessorConfiguration
    public let nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration

    public init(
        source: VesperPlayerSource? = nil,
        codec: String? = nil,
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
        diagnostics: [String: String] = [:]
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
    }

    public var wireMap: [String: Any] {
        [
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
    }
}

public enum VesperPlaybackCapabilityProbe {
    public typealias SessionProbeProvider =
        (VesperPlaybackCapabilityProbeRequest) -> VesperPlaybackCapabilitySessionProbeResult?

    public static func probe(
        _ request: VesperPlaybackCapabilityProbeRequest,
        sessionProbeProvider: SessionProbeProvider? = nil
    ) -> VesperPlaybackCapabilityProbeResult {
        let codecFamily = VesperPlaybackCodecFamily(
            candidate: VesperHardwareDecodeCandidateCodec(codecName: request.codec ?? "")
        )
        let effectiveRequiresNativeFrame =
            request.requiresNativeFrame
            || request.nativeFramePipelineConfiguration.mode == .preferNativeFrame
            || request.nativeFramePipelineConfiguration.mode == .requireNativeFrame
        let sourceIsRemote = request.source?.kind == .remote
        let sourceIsLocal = request.source?.kind == .local
        let codecKnown = codecFamily != .unknown
        let hardwareDecodeSupported =
            request.codec.map {
                VesperCodecSupport.hardwareDecodeSupported(for: $0)
            } ?? false
        let systemPlaybackSupported = sourceIsRemote || sourceIsLocal || codecKnown
        let hdrKind = request.codec.map(Self.detectHdrKind) ?? .none
        let isHdrOrDolbyVision = hdrKind != .none && hdrKind != .unknown
        let sessionProbeResult = isHdrOrDolbyVision ? sessionProbeProvider?(request) : nil
        var missing: [String] = []
        var diagnostics: [String: String] = [
            "probeVersion": "1",
            "sourceKind": request.source?.kind.rawValue ?? "unknown",
            "sourceProtocol": request.source?.`protocol`.rawValue ?? "unknown",
        ]
        if let sessionProbeResult {
            diagnostics.merge(sessionProbeResult.diagnostics) { _, new in new }
        }

        if request.codec == nil {
            missing.append("codecMetadata")
        }
        if effectiveRequiresNativeFrame && sourceIsRemote {
            missing.append("hostManagedNetworkProbeNotImplemented")
        }
        if effectiveRequiresNativeFrame
            && request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty
        {
            missing.append("nativeFrameDecoderPlugin")
        }
        if isHdrOrDolbyVision {
            missing.append("hdrProgrammableProcessingNotSupported")
            diagnostics["playbackPathPolicy"] = "hdrSystemPlaybackOnly"
            diagnostics["recommendedPlaybackPathReason"] = "hdrNativeFrameUnsupported"
            if let sessionProbeResult, !sessionProbeResult.supportedHdrKinds.contains(hdrKind) {
                missing.append("displayHdrCapability")
                diagnostics["displayHdrSupported"] = "false"
            }
        }
        if isHdrOrDolbyVision && request.frameProcessorConfiguration.mode == .diagnosticsOnly {
            diagnostics["frameProcessorProbe"] = "diagnosticsOnly"
        }
        if effectiveRequiresNativeFrame && !hardwareDecodeSupported {
            missing.append("deviceHardwareDecode")
        }

        let nativeFrameSupported =
            effectiveRequiresNativeFrame
            ? hardwareDecodeSupported
                && !request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty
                && !sourceIsRemote
            : hardwareDecodeSupported
        let recommendedPlaybackPath: VesperRecommendedPlaybackPath
        if isHdrOrDolbyVision {
            recommendedPlaybackPath = .systemPlayer
        } else if nativeFrameSupported && effectiveRequiresNativeFrame {
            recommendedPlaybackPath = .nativeFramePipeline
        } else {
            recommendedPlaybackPath = .systemPlayer
        }
        let outputFormat: VesperPlaybackCapabilityOutputFormat =
            recommendedPlaybackPath == .systemPlayer && isHdrOrDolbyVision ? .surfaceOpaque : .nv12
        let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode
        if hdrKind == .dolbyVision {
            dolbyVisionMode = .unsupported
        } else {
            dolbyVisionMode = .none
        }

        let status: VesperPlaybackCapabilityProbeStatus
        if request.codec == nil {
            status = .unknown
        } else if !codecKnown {
            status = .unsupported
        } else if missing.isEmpty {
            status = .supported
        } else if systemPlaybackSupported {
            status = .fallbackRequired
        } else {
            status = .unsupported
        }

        return VesperPlaybackCapabilityProbeResult(
            status: status,
            codecFamily: codecFamily,
            systemPlaybackSupported: systemPlaybackSupported,
            hardwareDecodeSupported: hardwareDecodeSupported,
            sdkManagedNativeFrameSupported: nativeFrameSupported && missing.isEmpty,
            recommendedPlaybackPath: recommendedPlaybackPath,
            outputFormat: outputFormat,
            hdrKind: hdrKind,
            dolbyVisionMode: dolbyVisionMode,
            confidence: sessionProbeResult != nil
                ? .sessionProbe : (sourceIsLocal ? .sourceMetadata : .codecOnly),
            missingCapabilities: missing,
            diagnostics: diagnostics
        )
    }

    private static func detectHdrKind(_ codec: String) -> VesperPlaybackCapabilityHdrKind {
        let normalizedCodecs =
            codec
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            .map { value in
                let normalized =
                    value.hasPrefix("video/")
                    ? String(value.dropFirst("video/".count))
                    : value
                return normalized
            }
        if normalizedCodecs.contains(where: {
            $0.hasPrefix("dvh1") || $0.hasPrefix("dvhe") || $0 == "dolbyvision"
        }) {
            return .dolbyVision
        }
        if normalizedCodecs.contains(where: { $0 == "hdr10" || $0 == "hdr10+" || $0 == "hdr10plus" }
        ) {
            return .hdr10
        }
        if normalizedCodecs.contains(where: { $0 == "hlg" }) {
            return .hlg
        }
        return .none
    }
}

extension VesperPlaybackCodecFamily {
    fileprivate init(candidate: VesperHardwareDecodeCandidateCodec) {
        switch candidate {
        case .h264:
            self = .h264
        case .hevc:
            self = .hevc
        case .av1:
            self = .av1
        case .vvc:
            self = .vvc
        case .unknown:
            self = .unknown
        }
    }
}
