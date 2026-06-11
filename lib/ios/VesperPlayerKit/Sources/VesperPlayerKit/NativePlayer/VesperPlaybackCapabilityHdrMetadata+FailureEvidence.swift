import Foundation

extension VesperPlaybackCapabilityHdrMetadata {
    var failureEvidenceDetails: [String: String] {
        var values: [String: String] = [:]
        putString(hdrKind?.rawValue, for: "hdrKind", into: &values)
        putString(dolbyVisionMode?.rawValue, for: "dolbyVisionMode", into: &values)
        putString(probe, for: "hdrMetadataProbe", into: &values)
        putString(codec, for: "assetVideoCodec", into: &values)
        putString(sampleMimeType, for: "runtimeFormatSampleMimeType", into: &values)
        putString(colorPrimaries, for: "assetVideoColorPrimaries", into: &values)
        putString(colorSpace, for: "runtimeFormatColorSpace", into: &values)
        putString(colorRange, for: "runtimeFormatColorRange", into: &values)
        putString(transferFunction, for: "assetVideoTransferFunction", into: &values)
        putString(yCbCrMatrix, for: "assetVideoYCbCrMatrix", into: &values)
        putString(
            alternativeTransferCharacteristics,
            for: "assetVideoAlternativeTransferCharacteristics",
            into: &values
        )
        putInt(lumaBitDepth, for: "runtimeFormatLumaBitDepth", into: &values)
        putInt(chromaBitDepth, for: "runtimeFormatChromaBitDepth", into: &values)
        putBool(hdrStaticInfoPresent, for: "runtimeFormatHdrStaticInfoPresent", into: &values)
        putInt(hdrStaticInfoByteLength, for: "runtimeFormatHdrStaticInfoByteLength", into: &values)
        putString(hdrStaticInfoParseError, for: "runtimeFormatHdrStaticInfoParseError", into: &values)
        putInt(maxContentLightLevelNits, for: "assetVideoMaxContentLightLevelNits", into: &values)
        putInt(maxFrameAverageLightLevelNits, for: "assetVideoMaxFrameAverageLightLevelNits", into: &values)
        putBool(
            masteringDisplayColorVolumePresent,
            for: "assetVideoMasteringDisplayColorVolumePresent",
            into: &values
        )
        putInt(
            masteringDisplayColorVolumeByteLength,
            for: "assetVideoMasteringDisplayColorVolumeByteLength",
            into: &values
        )
        putString(
            masteringDisplayColorVolumeParseError,
            for: "assetVideoMasteringDisplayColorVolumeParseError",
            into: &values
        )
        putPoint(masteringDisplayPrimary0, for: "assetVideoMasteringDisplayPrimary0", into: &values)
        putPoint(masteringDisplayPrimary1, for: "assetVideoMasteringDisplayPrimary1", into: &values)
        putPoint(masteringDisplayPrimary2, for: "assetVideoMasteringDisplayPrimary2", into: &values)
        putPoint(masteringDisplayWhitePoint, for: "assetVideoMasteringDisplayWhitePoint", into: &values)
        putDouble(
            masteringDisplayMaxLuminanceNits,
            for: "assetVideoMasteringDisplayMaxLuminanceNits",
            into: &values
        )
        putDouble(
            masteringDisplayMinLuminanceNits,
            for: "assetVideoMasteringDisplayMinLuminanceNits",
            into: &values
        )
        putString(dolbyVisionCodec, for: "dolbyVisionCodec", into: &values)
        putInt(dolbyVisionProfile, for: "dolbyVisionProfile", into: &values)
        putInt(dolbyVisionLevel, for: "dolbyVisionLevel", into: &values)
        putString(dolbyVisionCompatibility, for: "dolbyVisionCompatibility", into: &values)
        putString(dolbyVisionProfileFamily, for: "dolbyVisionProfileFamily", into: &values)
        putString(dolbyVisionBaseLayer, for: "dolbyVisionBaseLayer", into: &values)
        putString(dolbyVisionFallbackTarget, for: "dolbyVisionFallbackTarget", into: &values)
        putString(dolbyVisionBaseLayerEvidence, for: "dolbyVisionBaseLayerEvidence", into: &values)
        putString(dolbyVisionBaseLayerTransferFunction, for: "dolbyVisionBaseLayerTransferFunction", into: &values)
        return values
    }

    private func putString(_ value: String?, for key: String, into values: inout [String: String]) {
        guard let value, !value.isEmpty else {
            return
        }
        values[key] = value
    }

    private func putInt(_ value: Int?, for key: String, into values: inout [String: String]) {
        guard let value else {
            return
        }
        values[key] = String(value)
    }

    private func putBool(_ value: Bool?, for key: String, into values: inout [String: String]) {
        guard let value else {
            return
        }
        values[key] = String(value)
    }

    private func putDouble(_ value: Double?, for key: String, into values: inout [String: String]) {
        guard let value else {
            return
        }
        values[key] = String(value)
    }

    private func putPoint(
        _ point: VesperHdrChromaticityPoint?,
        for key: String,
        into values: inout [String: String]
    ) {
        guard let point else {
            return
        }
        values[key] = "\(point.x),\(point.y)"
    }
}
