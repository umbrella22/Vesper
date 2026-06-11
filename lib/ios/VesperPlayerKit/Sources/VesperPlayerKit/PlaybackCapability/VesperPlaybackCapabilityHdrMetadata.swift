import Foundation
public struct VesperHdrChromaticityPoint: Equatable {
    public let x: Double
    public let y: Double

    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }
}

public struct VesperPlaybackCapabilityHdrMetadata: Equatable {
    public let hdrKind: VesperPlaybackCapabilityHdrKind?
    public let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode?
    public let probe: String?
    public let codec: String?
    public let sampleMimeType: String?
    public let colorPrimaries: String?
    public let colorSpace: String?
    public let colorRange: String?
    public let transferFunction: String?
    public let yCbCrMatrix: String?
    public let alternativeTransferCharacteristics: String?
    public let lumaBitDepth: Int?
    public let chromaBitDepth: Int?
    public let hdrStaticInfoPresent: Bool?
    public let hdrStaticInfoByteLength: Int?
    public let hdrStaticInfoParseError: String?
    public let maxContentLightLevelNits: Int?
    public let maxFrameAverageLightLevelNits: Int?
    public let masteringDisplayColorVolumePresent: Bool?
    public let masteringDisplayColorVolumeByteLength: Int?
    public let masteringDisplayColorVolumeParseError: String?
    public let masteringDisplayPrimary0: VesperHdrChromaticityPoint?
    public let masteringDisplayPrimary1: VesperHdrChromaticityPoint?
    public let masteringDisplayPrimary2: VesperHdrChromaticityPoint?
    public let masteringDisplayWhitePoint: VesperHdrChromaticityPoint?
    public let masteringDisplayMaxLuminanceNits: Double?
    public let masteringDisplayMinLuminanceNits: Double?
    public let dolbyVisionCodec: String?
    public let dolbyVisionProfile: Int?
    public let dolbyVisionLevel: Int?
    public let dolbyVisionCompatibility: String?
    public let dolbyVisionProfileFamily: String?
    public let dolbyVisionBaseLayer: String?
    public let dolbyVisionFallbackTarget: String?
    public let dolbyVisionBaseLayerEvidence: String?
    public let dolbyVisionBaseLayerTransferFunction: String?

    public init(
        hdrKind: VesperPlaybackCapabilityHdrKind? = nil,
        dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode? = nil,
        probe: String? = nil,
        codec: String? = nil,
        sampleMimeType: String? = nil,
        colorPrimaries: String? = nil,
        colorSpace: String? = nil,
        colorRange: String? = nil,
        transferFunction: String? = nil,
        yCbCrMatrix: String? = nil,
        alternativeTransferCharacteristics: String? = nil,
        lumaBitDepth: Int? = nil,
        chromaBitDepth: Int? = nil,
        hdrStaticInfoPresent: Bool? = nil,
        hdrStaticInfoByteLength: Int? = nil,
        hdrStaticInfoParseError: String? = nil,
        maxContentLightLevelNits: Int? = nil,
        maxFrameAverageLightLevelNits: Int? = nil,
        masteringDisplayColorVolumePresent: Bool? = nil,
        masteringDisplayColorVolumeByteLength: Int? = nil,
        masteringDisplayColorVolumeParseError: String? = nil,
        masteringDisplayPrimary0: VesperHdrChromaticityPoint? = nil,
        masteringDisplayPrimary1: VesperHdrChromaticityPoint? = nil,
        masteringDisplayPrimary2: VesperHdrChromaticityPoint? = nil,
        masteringDisplayWhitePoint: VesperHdrChromaticityPoint? = nil,
        masteringDisplayMaxLuminanceNits: Double? = nil,
        masteringDisplayMinLuminanceNits: Double? = nil,
        dolbyVisionCodec: String? = nil,
        dolbyVisionProfile: Int? = nil,
        dolbyVisionLevel: Int? = nil,
        dolbyVisionCompatibility: String? = nil,
        dolbyVisionProfileFamily: String? = nil,
        dolbyVisionBaseLayer: String? = nil,
        dolbyVisionFallbackTarget: String? = nil,
        dolbyVisionBaseLayerEvidence: String? = nil,
        dolbyVisionBaseLayerTransferFunction: String? = nil
    ) {
        self.hdrKind = hdrKind
        self.dolbyVisionMode = dolbyVisionMode
        self.probe = probe
        self.codec = codec
        self.sampleMimeType = sampleMimeType
        self.colorPrimaries = colorPrimaries
        self.colorSpace = colorSpace
        self.colorRange = colorRange
        self.transferFunction = transferFunction
        self.yCbCrMatrix = yCbCrMatrix
        self.alternativeTransferCharacteristics = alternativeTransferCharacteristics
        self.lumaBitDepth = lumaBitDepth
        self.chromaBitDepth = chromaBitDepth
        self.hdrStaticInfoPresent = hdrStaticInfoPresent
        self.hdrStaticInfoByteLength = hdrStaticInfoByteLength
        self.hdrStaticInfoParseError = hdrStaticInfoParseError
        self.maxContentLightLevelNits = maxContentLightLevelNits
        self.maxFrameAverageLightLevelNits = maxFrameAverageLightLevelNits
        self.masteringDisplayColorVolumePresent = masteringDisplayColorVolumePresent
        self.masteringDisplayColorVolumeByteLength = masteringDisplayColorVolumeByteLength
        self.masteringDisplayColorVolumeParseError = masteringDisplayColorVolumeParseError
        self.masteringDisplayPrimary0 = masteringDisplayPrimary0
        self.masteringDisplayPrimary1 = masteringDisplayPrimary1
        self.masteringDisplayPrimary2 = masteringDisplayPrimary2
        self.masteringDisplayWhitePoint = masteringDisplayWhitePoint
        self.masteringDisplayMaxLuminanceNits = masteringDisplayMaxLuminanceNits
        self.masteringDisplayMinLuminanceNits = masteringDisplayMinLuminanceNits
        self.dolbyVisionCodec = dolbyVisionCodec
        self.dolbyVisionProfile = dolbyVisionProfile
        self.dolbyVisionLevel = dolbyVisionLevel
        self.dolbyVisionCompatibility = dolbyVisionCompatibility
        self.dolbyVisionProfileFamily = dolbyVisionProfileFamily
        self.dolbyVisionBaseLayer = dolbyVisionBaseLayer
        self.dolbyVisionFallbackTarget = dolbyVisionFallbackTarget
        self.dolbyVisionBaseLayerEvidence = dolbyVisionBaseLayerEvidence
        self.dolbyVisionBaseLayerTransferFunction = dolbyVisionBaseLayerTransferFunction
    }

    var isEmpty: Bool {
        hdrKind == nil &&
            dolbyVisionMode == nil &&
            probe == nil &&
            codec == nil &&
            sampleMimeType == nil &&
            colorPrimaries == nil &&
            colorSpace == nil &&
            colorRange == nil &&
            transferFunction == nil &&
            yCbCrMatrix == nil &&
            alternativeTransferCharacteristics == nil &&
            lumaBitDepth == nil &&
            chromaBitDepth == nil &&
            hdrStaticInfoPresent == nil &&
            hdrStaticInfoByteLength == nil &&
            hdrStaticInfoParseError == nil &&
            maxContentLightLevelNits == nil &&
            maxFrameAverageLightLevelNits == nil &&
            masteringDisplayColorVolumePresent == nil &&
            masteringDisplayColorVolumeByteLength == nil &&
            masteringDisplayColorVolumeParseError == nil &&
            masteringDisplayPrimary0 == nil &&
            masteringDisplayPrimary1 == nil &&
            masteringDisplayPrimary2 == nil &&
            masteringDisplayWhitePoint == nil &&
            masteringDisplayMaxLuminanceNits == nil &&
            masteringDisplayMinLuminanceNits == nil &&
            dolbyVisionCodec == nil &&
            dolbyVisionProfile == nil &&
            dolbyVisionLevel == nil &&
            dolbyVisionCompatibility == nil &&
            dolbyVisionProfileFamily == nil &&
            dolbyVisionBaseLayer == nil &&
            dolbyVisionFallbackTarget == nil &&
            dolbyVisionBaseLayerEvidence == nil &&
            dolbyVisionBaseLayerTransferFunction == nil
    }
}
extension VesperPlaybackCapabilityHdrMetadata {
    var wireMap: [String: Any] {
        var values: [String: Any] = [:]
        values.put(hdrKind?.rawValue, for: "hdrKind")
        values.put(dolbyVisionMode?.rawValue, for: "dolbyVisionMode")
        values.put(probe, for: "probe")
        values.put(codec, for: "codec")
        values.put(sampleMimeType, for: "sampleMimeType")
        values.put(colorPrimaries, for: "colorPrimaries")
        values.put(colorSpace, for: "colorSpace")
        values.put(colorRange, for: "colorRange")
        values.put(transferFunction, for: "transferFunction")
        values.put(yCbCrMatrix, for: "yCbCrMatrix")
        values.put(
            alternativeTransferCharacteristics,
            for: "alternativeTransferCharacteristics"
        )
        values.put(lumaBitDepth, for: "lumaBitDepth")
        values.put(chromaBitDepth, for: "chromaBitDepth")
        values.put(hdrStaticInfoPresent, for: "hdrStaticInfoPresent")
        values.put(hdrStaticInfoByteLength, for: "hdrStaticInfoByteLength")
        values.put(hdrStaticInfoParseError, for: "hdrStaticInfoParseError")
        values.put(maxContentLightLevelNits, for: "maxContentLightLevelNits")
        values.put(maxFrameAverageLightLevelNits, for: "maxFrameAverageLightLevelNits")
        values.put(
            masteringDisplayColorVolumePresent,
            for: "masteringDisplayColorVolumePresent"
        )
        values.put(
            masteringDisplayColorVolumeByteLength,
            for: "masteringDisplayColorVolumeByteLength"
        )
        values.put(
            masteringDisplayColorVolumeParseError,
            for: "masteringDisplayColorVolumeParseError"
        )
        values.put(masteringDisplayPrimary0?.wireMap, for: "masteringDisplayPrimary0")
        values.put(masteringDisplayPrimary1?.wireMap, for: "masteringDisplayPrimary1")
        values.put(masteringDisplayPrimary2?.wireMap, for: "masteringDisplayPrimary2")
        values.put(masteringDisplayWhitePoint?.wireMap, for: "masteringDisplayWhitePoint")
        values.put(masteringDisplayMaxLuminanceNits, for: "masteringDisplayMaxLuminanceNits")
        values.put(masteringDisplayMinLuminanceNits, for: "masteringDisplayMinLuminanceNits")
        values.put(dolbyVisionCodec, for: "dolbyVisionCodec")
        values.put(dolbyVisionProfile, for: "dolbyVisionProfile")
        values.put(dolbyVisionLevel, for: "dolbyVisionLevel")
        values.put(dolbyVisionCompatibility, for: "dolbyVisionCompatibility")
        values.put(dolbyVisionProfileFamily, for: "dolbyVisionProfileFamily")
        values.put(dolbyVisionBaseLayer, for: "dolbyVisionBaseLayer")
        values.put(dolbyVisionFallbackTarget, for: "dolbyVisionFallbackTarget")
        values.put(dolbyVisionBaseLayerEvidence, for: "dolbyVisionBaseLayerEvidence")
        values.put(dolbyVisionBaseLayerTransferFunction, for: "dolbyVisionBaseLayerTransferFunction")
        return values
    }
}

extension VesperHdrChromaticityPoint {
    var wireMap: [String: Double] {
        ["x": x, "y": y]
    }
}

extension Dictionary where Key == String, Value == Any {
    mutating func put(_ value: Any?, for key: String) {
        guard let value else {
            return
        }
        self[key] = value
    }
}
