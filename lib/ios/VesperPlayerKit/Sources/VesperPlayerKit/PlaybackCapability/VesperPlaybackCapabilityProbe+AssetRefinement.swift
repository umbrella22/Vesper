import Foundation
extension VesperPlaybackCapabilityProbe {
    static func withAssetProbeResult(
        _ result: VesperPlaybackCapabilityProbeResult,
        assetProbeResult: VesperPlaybackCapabilityAssetProbeResult
    ) -> VesperPlaybackCapabilityProbeResult {
        var missing = result.missingCapabilities
        var diagnostics = result.diagnostics
        diagnostics.merge(assetProbeResult.diagnostics) { _, new in new }
        applyDolbyVisionProfile8Refinement(to: &diagnostics)
        let metadataHdrKind = assetProbeResult.metadataHdrKind
        let effectiveHdrKind =
            result.hdrKind == .none || result.hdrKind == .unknown
            ? (metadataHdrKind ?? result.hdrKind)
            : result.hdrKind
        let isHdrOrDolbyVision = effectiveHdrKind != .none && effectiveHdrKind != .unknown
        if assetProbeResult.isPlayable == false, !missing.contains("assetPlayable") {
            missing.append("assetPlayable")
        }
        if isHdrOrDolbyVision, !missing.contains("hdrProgrammableProcessingNotSupported") {
            missing.append("hdrProgrammableProcessingNotSupported")
            diagnostics["playbackPathPolicy"] = "hdrSystemPlaybackOnly"
            diagnostics["recommendedPlaybackPathReason"] = "hdrNativeFrameUnsupported"
            if let metadataHdrKind {
                diagnostics["hdrKindSource"] = "assetMetadata"
                diagnostics["assetVideoMetadataHdrKind"] = metadataHdrKind.rawValue
            }
        }

        let status: VesperPlaybackCapabilityProbeStatus
        if result.status == .unsupported || result.status == .unknown {
            status = result.status
        } else if assetProbeResult.isPlayable == false {
            status = .unsupported
        } else if isHdrOrDolbyVision && result.status == .supported {
            status = .fallbackRequired
        } else {
            status = result.status
        }
        let recommendedPlaybackPath: VesperRecommendedPlaybackPath =
            isHdrOrDolbyVision ? .systemPlayer : result.recommendedPlaybackPath
        let outputFormat: VesperPlaybackCapabilityOutputFormat =
            recommendedPlaybackPath == .systemPlayer && isHdrOrDolbyVision
            ? .surfaceOpaque
            : result.outputFormat
        let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode =
            effectiveHdrKind == .dolbyVision && result.dolbyVisionMode == .none
            ? .unsupported
            : result.dolbyVisionMode
        let hdrMetadata = buildHdrMetadata(
            hdrKind: effectiveHdrKind,
            dolbyVisionMode: dolbyVisionMode,
            diagnostics: diagnostics
        )
        let confidence = confidenceAfterAssetProbe(
            baseConfidence: result.confidence,
            metadataHdrKind: metadataHdrKind
        )

        return VesperPlaybackCapabilityProbeResult(
            status: status,
            codecFamily: result.codecFamily,
            systemPlaybackSupported: result.systemPlaybackSupported && assetProbeResult.isPlayable != false,
            hardwareDecodeSupported: result.hardwareDecodeSupported,
            sdkManagedNativeFrameSupported: result.sdkManagedNativeFrameSupported,
            recommendedPlaybackPath: recommendedPlaybackPath,
            outputFormat: outputFormat,
            hdrKind: effectiveHdrKind,
            dolbyVisionMode: dolbyVisionMode,
            confidence: confidence,
            missingCapabilities: missing,
            diagnostics: diagnostics,
            hdrMetadata: hdrMetadata
        )
    }

    private static func confidenceAfterAssetProbe(
        baseConfidence: VesperPlaybackCapabilityConfidence,
        metadataHdrKind: VesperPlaybackCapabilityHdrKind?
    ) -> VesperPlaybackCapabilityConfidence {
        guard baseConfidence != .sessionProbe,
            let metadataHdrKind,
            metadataHdrKind != .none,
            metadataHdrKind != .unknown
        else {
            return baseConfidence
        }
        return .sourceMetadata
    }
}
