import Foundation
extension VesperPlaybackCapabilityProbe {
    public static func buildHdrMetadata(
        hdrKind: VesperPlaybackCapabilityHdrKind,
        dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode,
        diagnostics: [String: String]
    ) -> VesperPlaybackCapabilityHdrMetadata? {
        let refinedDiagnostics = diagnostics.withDolbyVisionProfile8Refinement()
        let metadata = VesperPlaybackCapabilityHdrMetadata(
            hdrKind: hdrKind == .none || hdrKind == .unknown ? nil : hdrKind,
            dolbyVisionMode: dolbyVisionMode == .none ? nil : dolbyVisionMode,
            probe: refinedDiagnostics.firstString(
                "runtimeFormatHdrMetadataProbe",
                "assetVideoHdrMetadataProbe",
                "assetProbe"
            ),
            codec: refinedDiagnostics.firstString("assetVideoCodec", "runtimeFormatCodecs"),
            sampleMimeType: refinedDiagnostics.stringValue("runtimeFormatSampleMimeType"),
            colorPrimaries: refinedDiagnostics.stringValue("assetVideoColorPrimaries"),
            colorSpace: refinedDiagnostics.stringValue("runtimeFormatColorSpace"),
            colorRange: refinedDiagnostics.stringValue("runtimeFormatColorRange"),
            transferFunction: refinedDiagnostics.firstString(
                "assetVideoTransferFunction",
                "runtimeFormatColorTransfer"
            ),
            yCbCrMatrix: refinedDiagnostics.stringValue("assetVideoYCbCrMatrix"),
            alternativeTransferCharacteristics:
                refinedDiagnostics.stringValue("assetVideoAlternativeTransferCharacteristics"),
            lumaBitDepth: refinedDiagnostics.intValue("runtimeFormatLumaBitDepth"),
            chromaBitDepth: refinedDiagnostics.intValue("runtimeFormatChromaBitDepth"),
            hdrStaticInfoPresent: refinedDiagnostics.boolValue("runtimeFormatHdrStaticInfoPresent"),
            hdrStaticInfoByteLength: refinedDiagnostics.intValue("runtimeFormatHdrStaticInfoByteLength"),
            hdrStaticInfoParseError: refinedDiagnostics.stringValue("runtimeFormatHdrStaticInfoParseError"),
            maxContentLightLevelNits: refinedDiagnostics.firstInt(
                "assetVideoMaxContentLightLevelNits",
                "runtimeFormatMaxContentLightLevelNits"
            ),
            maxFrameAverageLightLevelNits: refinedDiagnostics.firstInt(
                "assetVideoMaxFrameAverageLightLevelNits",
                "runtimeFormatMaxFrameAverageLightLevelNits"
            ),
            masteringDisplayColorVolumePresent:
                refinedDiagnostics.boolValue("assetVideoMasteringDisplayColorVolumePresent"),
            masteringDisplayColorVolumeByteLength:
                refinedDiagnostics.intValue("assetVideoMasteringDisplayColorVolumeByteLength"),
            masteringDisplayColorVolumeParseError:
                refinedDiagnostics.stringValue("assetVideoMasteringDisplayColorVolumeParseError"),
            masteringDisplayPrimary0:
                refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary0"),
            masteringDisplayPrimary1:
                refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary1"),
            masteringDisplayPrimary2:
                refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary2"),
            masteringDisplayWhitePoint:
                refinedDiagnostics.chromaticityPoint("assetVideoMasteringDisplayWhitePoint"),
            masteringDisplayMaxLuminanceNits:
                refinedDiagnostics.doubleValue("assetVideoMasteringDisplayMaxLuminanceNits"),
            masteringDisplayMinLuminanceNits:
                refinedDiagnostics.doubleValue("assetVideoMasteringDisplayMinLuminanceNits"),
            dolbyVisionCodec: refinedDiagnostics.stringValue("dolbyVisionCodec"),
            dolbyVisionProfile: refinedDiagnostics.intValue("dolbyVisionProfile"),
            dolbyVisionLevel: refinedDiagnostics.intValue("dolbyVisionLevel"),
            dolbyVisionCompatibility: refinedDiagnostics.stringValue("dolbyVisionCompatibility"),
            dolbyVisionProfileFamily: refinedDiagnostics.stringValue("dolbyVisionProfileFamily"),
            dolbyVisionBaseLayer: refinedDiagnostics.stringValue("dolbyVisionBaseLayer"),
            dolbyVisionFallbackTarget: refinedDiagnostics.stringValue("dolbyVisionFallbackTarget"),
            dolbyVisionBaseLayerEvidence: refinedDiagnostics.stringValue("dolbyVisionBaseLayerEvidence"),
            dolbyVisionBaseLayerTransferFunction: refinedDiagnostics.stringValue("dolbyVisionBaseLayerTransferFunction")
        )
        return metadata.isEmpty ? nil : metadata
    }

    static func applyDolbyVisionProfile8Refinement(to diagnostics: inout [String: String]) {
        guard diagnostics.intValue("dolbyVisionProfile") == 8,
            let evidence = dolbyVisionProfile8BaseLayerEvidence(in: diagnostics)
        else {
            return
        }
        diagnostics["dolbyVisionCompatibility"] = evidence.compatibility
        diagnostics["dolbyVisionProfileFamily"] = "profile8SingleLayerCompatible"
        diagnostics["dolbyVisionBaseLayer"] = evidence.baseLayer
        diagnostics["dolbyVisionFallbackTarget"] = evidence.fallbackTarget
        diagnostics["dolbyVisionBaseLayerEvidence"] = evidence.key
        diagnostics["dolbyVisionBaseLayerTransferFunction"] = evidence.transferFunction
    }

    private static func dolbyVisionProfile8BaseLayerEvidence(
        in diagnostics: [String: String]
    ) -> DolbyVisionProfile8BaseLayerEvidence? {
        for key in [
            "assetVideoTransferFunction",
            "assetVideoAlternativeTransferCharacteristics",
            "runtimeFormatColorTransfer",
        ] {
            if let transferFunction = diagnostics.stringValue(key),
                let evidence = DolbyVisionProfile8BaseLayerEvidence(
                    key: key,
                    transferFunction: transferFunction
                )
            {
                return evidence
            }
        }
        return nil
    }
}
