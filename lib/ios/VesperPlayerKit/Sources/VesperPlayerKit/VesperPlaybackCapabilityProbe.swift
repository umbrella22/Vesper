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

public struct VesperPlaybackCapabilityProbeRequest: Equatable {
    public let source: VesperPlayerSource?
    public let codec: String?
    public let requiresNativeFrame: Bool
    public let requiresHdrNativeFrame: Bool
    public let sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration
    public let frameProcessorConfiguration: VesperFrameProcessorConfiguration
    public let nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration

    public init(
        source: VesperPlayerSource? = nil,
        codec: String? = nil,
        requiresNativeFrame: Bool = false,
        requiresHdrNativeFrame: Bool = false,
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
        self.requiresHdrNativeFrame = requiresHdrNativeFrame
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
    public let hdrNativeFrameSupported: Bool
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
        hdrNativeFrameSupported: Bool,
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
        self.hdrNativeFrameSupported = hdrNativeFrameSupported
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
            "hdrNativeFrameSupported": hdrNativeFrameSupported,
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
    public static func probe(
        _ request: VesperPlaybackCapabilityProbeRequest
    ) -> VesperPlaybackCapabilityProbeResult {
        let codecFamily = VesperPlaybackCodecFamily(
            candidate: VesperHardwareDecodeCandidateCodec(codecName: request.codec ?? "")
        )
        let effectiveRequiresNativeFrame = request.requiresNativeFrame ||
            request.nativeFramePipelineConfiguration.mode == .preferNativeFrame ||
            request.nativeFramePipelineConfiguration.mode == .requireNativeFrame
        let sourceIsRemote = request.source?.kind == .remote
        let sourceIsLocal = request.source?.kind == .local
        let codecKnown = codecFamily != .unknown
        let hardwareDecodeSupported = request.codec.map {
            VesperCodecSupport.hardwareDecodeSupported(for: $0)
        } ?? false
        let systemPlaybackSupported = sourceIsRemote || sourceIsLocal || codecKnown
        let isDolbyVision = request.codec.map(Self.codecLooksDolbyVision) ?? false
        let nativeFrameRequested = effectiveRequiresNativeFrame
        let rejectsHdrNativeFrame = request.requiresHdrNativeFrame || (isDolbyVision && nativeFrameRequested)
        var missing: [String] = []
        var diagnostics: [String: String] = [
            "probeVersion": "1",
            "sourceKind": request.source?.kind.rawValue ?? "unknown",
            "sourceProtocol": request.source?.`protocol`.rawValue ?? "unknown",
        ]

        if request.codec == nil {
            missing.append("codecMetadata")
        }
        if effectiveRequiresNativeFrame && sourceIsRemote {
            missing.append("hostManagedNetworkProbeNotImplemented")
        }
        if effectiveRequiresNativeFrame && request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty {
            missing.append("nativeFrameDecoderPlugin")
        }
        if rejectsHdrNativeFrame && !request.sourceNormalizerConfiguration.supportsPacketInput {
            missing.append("SourceNormalizerPacketHdrMetadata")
        }
        if rejectsHdrNativeFrame {
            missing.append("hdrProgrammableProcessingNotSupported")
            diagnostics["hdrNativeFramePolicy"] = "systemPlaybackOnly"
            if request.nativeFramePipelineConfiguration.mode == .requireNativeFrame {
                diagnostics["nativeFrameRejectedForHdrProcessing"] = "true"
            } else {
                diagnostics["systemPlaybackSelectedForHdr"] = "true"
            }
        }
        if rejectsHdrNativeFrame && request.frameProcessorConfiguration.mode == .diagnosticsOnly {
            diagnostics["frameProcessorProbe"] = "diagnosticsOnly"
        }
        if effectiveRequiresNativeFrame && !hardwareDecodeSupported {
            missing.append("deviceHardwareDecode")
        }

        let outputFormat: VesperPlaybackCapabilityOutputFormat = rejectsHdrNativeFrame ? .unknown : .nv12
        let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode
        if isDolbyVision && nativeFrameRequested {
            dolbyVisionMode = .unsupported
        } else {
            dolbyVisionMode = .none
        }
        let nativeFrameSupported = effectiveRequiresNativeFrame
            ? hardwareDecodeSupported &&
                !request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty &&
                !sourceIsRemote
            : hardwareDecodeSupported

        let status: VesperPlaybackCapabilityProbeStatus
        if request.codec == nil {
            status = .unknown
        } else if !codecKnown {
            status = .unsupported
        } else if rejectsHdrNativeFrame && request.nativeFramePipelineConfiguration.mode == .requireNativeFrame {
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
            hdrNativeFrameSupported: false,
            outputFormat: outputFormat,
            hdrKind: isDolbyVision ? .dolbyVision : (rejectsHdrNativeFrame ? .unknown : .none),
            dolbyVisionMode: dolbyVisionMode,
            confidence: sourceIsLocal ? .sourceMetadata : .codecOnly,
            missingCapabilities: missing,
            diagnostics: diagnostics
        )
    }

    private static func codecLooksDolbyVision(_ codec: String) -> Bool {
        codec
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            .contains { value in
                let normalized = value.hasPrefix("video/")
                    ? String(value.dropFirst("video/".count))
                    : value
                return normalized.hasPrefix("dvh1") || normalized.hasPrefix("dvhe")
            }
    }
}

private extension VesperPlaybackCodecFamily {
    init(candidate: VesperHardwareDecodeCandidateCodec) {
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
