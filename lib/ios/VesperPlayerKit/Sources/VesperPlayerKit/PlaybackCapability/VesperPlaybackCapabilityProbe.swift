import Foundation
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
        let dolbyVisionCodecInfo = request.codec.flatMap(Self.detectDolbyVisionCodecInfo)
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
        if let dolbyVisionCodecInfo {
            diagnostics.merge(dolbyVisionCodecInfo.diagnostics) { _, new in new }
        }
        applyDolbyVisionProfile8Refinement(to: &diagnostics)

        if request.codec == nil {
            missing.append("codecMetadata")
        }
        if effectiveRequiresNativeFrame && sourceIsRemote {
            missing.append("hostManagedNetworkProbeNotImplemented")
        }
        let hasNativeFrameDecoder =
            request.nativeFramePipelineConfiguration.decoderPluginReferences.contains { reference in
                VesperBundledPluginResolver.isRegisteredNativeReference(
                    reference,
                    pluginId: VesperBundledPluginReferences.decoderVideoToolbox.pluginId
                )
            }
        if effectiveRequiresNativeFrame && !hasNativeFrameDecoder {
            missing.append("nativeFrameDecoderPlugin")
        }
        if isHdrOrDolbyVision {
            missing.append("hdrProgrammableProcessingNotSupported")
            diagnostics["playbackPathPolicy"] = "hdrSystemPlaybackOnly"
            diagnostics["recommendedPlaybackPathReason"] = "hdrNativeFrameUnsupported"
            let displayHdrProbeAvailable =
                sessionProbeResult != nil
                && (sessionProbeResult?.diagnostics[displayHdrProbeAvailableKey] == "true"
                    || !(sessionProbeResult?.supportedHdrKinds.isEmpty ?? true))
            if let sessionProbeResult, displayHdrProbeAvailable,
                !sessionProbeResult.supportedHdrKinds.contains(hdrKind)
            {
                missing.append("displayHdrCapability")
                diagnostics["displayHdrSupported"] = "false"
            }
            if sessionProbeResult?.diagnostics[displayFrameRateSupportedKey] == "false" {
                missing.append("displayFrameRate")
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
                && hasNativeFrameDecoder
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
        if let dolbyVisionCodecInfo {
            dolbyVisionMode = dolbyVisionCodecInfo.dolbyVisionMode
        } else if hdrKind == .dolbyVision {
            dolbyVisionMode = .unsupported
        } else {
            dolbyVisionMode = .none
        }
        let confidence: VesperPlaybackCapabilityConfidence =
            sessionProbeResult != nil ? .sessionProbe : (sourceIsLocal ? .sourceMetadata : .codecOnly)
        let hdrMetadata = buildHdrMetadata(
            hdrKind: hdrKind,
            dolbyVisionMode: dolbyVisionMode,
            diagnostics: diagnostics
        )

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
            confidence: confidence,
            missingCapabilities: missing,
            diagnostics: diagnostics,
            hdrMetadata: hdrMetadata
        )
    }
}
