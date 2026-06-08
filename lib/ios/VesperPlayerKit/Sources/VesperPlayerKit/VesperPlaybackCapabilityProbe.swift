import AVFoundation
import Foundation
import UIKit

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

struct VesperDolbyVisionCodecInfo: Equatable {
    let codec: String
    let profile: Int?
    let level: Int?

    var dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode {
        matrix.dolbyVisionMode
    }

    var diagnostics: [String: String] {
        var diagnostics = [
            "dolbyVisionCodec": codec,
            "dolbyVisionCompatibility": matrix.compatibility,
            "dolbyVisionProfileFamily": matrix.profileFamily,
            "dolbyVisionBaseLayer": matrix.baseLayer,
            "dolbyVisionFallbackTarget": matrix.fallbackTarget,
        ]
        if let profile {
            diagnostics["dolbyVisionProfile"] = String(profile)
        }
        if let level {
            diagnostics["dolbyVisionLevel"] = String(level)
        }
        return diagnostics
    }

    var matrix: VesperDolbyVisionProfileMatrix {
        VesperDolbyVisionProfileMatrix(profile: profile)
    }
}

struct VesperDolbyVisionProfileMatrix: Equatable {
    let dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode
    let compatibility: String
    let profileFamily: String
    let baseLayer: String
    let fallbackTarget: String

    init(profile: Int?) {
        switch profile {
        case 5:
            dolbyVisionMode = .unsupported
            compatibility = "noCompatibleBaseLayer"
            profileFamily = "profile5SingleLayer"
            baseLayer = "none"
            fallbackTarget = "dolbyVisionSystemPlayer"
        case 7:
            dolbyVisionMode = .compatibleBaseLayer
            compatibility = "dualLayerBaseLayerCandidate"
            profileFamily = "profile7DualLayer"
            baseLayer = "hdr10BaseLayerCandidate"
            fallbackTarget = "hdr10BaseLayerSystemPlayer"
        case 8:
            dolbyVisionMode = .compatibleBaseLayer
            compatibility = "compatibleBaseLayerCandidate"
            profileFamily = "profile8SingleLayerCompatible"
            baseLayer = "compatibleBaseLayerUnknown"
            fallbackTarget = "compatibleBaseLayerSystemPlayer"
        case 9:
            dolbyVisionMode = .unsupported
            compatibility = "unknownProfile"
            profileFamily = "profile9ConservativeUnknown"
            baseLayer = "unknown"
            fallbackTarget = "unknownSystemPlayer"
        case nil:
            dolbyVisionMode = .unsupported
            compatibility = "profileUnknown"
            profileFamily = "profileUnknown"
            baseLayer = "unknown"
            fallbackTarget = "unknownSystemPlayer"
        default:
            dolbyVisionMode = .unsupported
            compatibility = "unknownProfile"
            profileFamily = "unknownProfile"
            baseLayer = "unknown"
            fallbackTarget = "unknownSystemPlayer"
        }
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

private extension VesperPlaybackCapabilityHdrMetadata {
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

private extension VesperHdrChromaticityPoint {
    var wireMap: [String: Double] {
        ["x": x, "y": y]
    }
}

private extension Dictionary where Key == String, Value == Any {
    mutating func put(_ value: Any?, for key: String) {
        guard let value else {
            return
        }
        self[key] = value
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
        if effectiveRequiresNativeFrame
            && request.nativeFramePipelineConfiguration.decoderPluginLibraryPaths.isEmpty
        {
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

    static func detectHdrKind(_ codec: String) -> VesperPlaybackCapabilityHdrKind {
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

    static func detectDolbyVisionCodecInfo(_ codec: String) -> VesperDolbyVisionCodecInfo? {
        let dolbyVisionCodec =
            codec
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            .map { value in
                value.hasPrefix("video/") ? String(value.dropFirst("video/".count)) : value
            }
            .first {
                $0.hasPrefix("dvh1") || $0.hasPrefix("dvhe") || $0 == "dolbyvision"
            }
        guard let dolbyVisionCodec else {
            return nil
        }
        let parts = dolbyVisionCodec.split(separator: ".")
        let profile: Int?
        let level: Int?
        if parts.count >= 2, parts[0] == "dvh1" || parts[0] == "dvhe" {
            profile = Int(parts[1])
        } else {
            profile = nil
        }
        if parts.count >= 3, parts[0] == "dvh1" || parts[0] == "dvhe" {
            level = Int(parts[2])
        } else {
            level = nil
        }
        return VesperDolbyVisionCodecInfo(
            codec: dolbyVisionCodec,
            profile: profile,
            level: level
        )
    }

    static func detectMetadataHdrKind(
        _ diagnostics: [String: String]
    ) -> VesperPlaybackCapabilityHdrKind? {
        if let codec = diagnostics["assetVideoCodec"]?.lowercased(),
            codec.hasPrefix("dvh1") || codec.hasPrefix("dvhe") || codec == "dolbyvision"
        {
            return .dolbyVision
        }
        guard let transferFunction = diagnostics["assetVideoTransferFunction"]?.lowercased() else {
            return nil
        }
        if transferFunction.contains("hlg") ||
            transferFunction.contains("arib") ||
            transferFunction.contains("std-b67") ||
            transferFunction.contains("std_b67")
        {
            return .hlg
        }
        if transferFunction.contains("pq") ||
            transferFunction.contains("2084") ||
            transferFunction.contains("st2084") ||
            transferFunction.contains("st_2084")
        {
            return .hdr10
        }
        return nil
    }
}

private struct DolbyVisionProfile8BaseLayerEvidence: Equatable {
    let key: String
    let transferFunction: String
    let compatibility: String
    let baseLayer: String
    let fallbackTarget: String

    init?(key: String, transferFunction: String) {
        let normalized = transferFunction.lowercased()
        self.key = key
        self.transferFunction = transferFunction
        if normalized.contains("hlg") ||
            normalized.contains("arib") ||
            normalized.contains("std-b67") ||
            normalized.contains("std_b67")
        {
            compatibility = "profile8HlgBaseLayer"
            baseLayer = "hlgBaseLayer"
            fallbackTarget = "hlgBaseLayerSystemPlayer"
        } else if normalized.contains("pq") ||
            normalized.contains("2084") ||
            normalized.contains("st2084") ||
            normalized.contains("st_2084")
        {
            compatibility = "profile8Hdr10BaseLayer"
            baseLayer = "hdr10BaseLayer"
            fallbackTarget = "hdr10BaseLayerSystemPlayer"
        } else if normalized == "sdr" ||
            normalized == "srgb" ||
            normalized.contains("bt709") ||
            normalized.contains("bt.709") ||
            normalized.contains("itu_r_709") ||
            normalized.contains("gamma")
        {
            compatibility = "profile8SdrBaseLayer"
            baseLayer = "sdrBaseLayer"
            fallbackTarget = "sdrBaseLayerSystemPlayer"
        } else {
            return nil
        }
    }
}

private extension Dictionary where Key == String, Value == String {
    func withDolbyVisionProfile8Refinement() -> [String: String] {
        var values = self
        VesperPlaybackCapabilityProbe.applyDolbyVisionProfile8Refinement(to: &values)
        return values
    }
}

struct VesperIOSSessionProbeEnvironment: Equatable {
    enum DisplayGamut: String, Equatable {
        case p3
        case srgb
        case unspecified
        case unknown

        init(_ gamut: UIDisplayGamut) {
            switch gamut {
            case .P3:
                self = .p3
            case .SRGB:
                self = .srgb
            case .unspecified:
                self = .unspecified
            @unknown default:
                self = .unknown
            }
        }
    }

    let displayGamut: DisplayGamut
    let hdrPlaybackEligible: Bool?
    let maximumFramesPerSecond: Int?
    let nativeWidth: Int?
    let nativeHeight: Int?

    @MainActor
    static func current(
        screen: UIScreen? = nil
    ) -> VesperIOSSessionProbeEnvironment {
        let resolvedScreen = screen ?? UIScreen.main
        let nativeBounds = resolvedScreen.nativeBounds
        let hdrPlaybackEligible: Bool?
        if #available(iOS 11.0, *) {
            hdrPlaybackEligible = AVPlayer.eligibleForHDRPlayback
        } else {
            hdrPlaybackEligible = nil
        }
        return VesperIOSSessionProbeEnvironment(
            displayGamut: DisplayGamut(resolvedScreen.traitCollection.displayGamut),
            hdrPlaybackEligible: hdrPlaybackEligible,
            maximumFramesPerSecond: resolvedScreen.maximumFramesPerSecond,
            nativeWidth: Int(nativeBounds.width.rounded()),
            nativeHeight: Int(nativeBounds.height.rounded())
        )
    }
}

enum VesperIOSSessionProbeProvider {
    @MainActor
    static func currentDisplay() -> VesperPlaybackCapabilityProbe.SessionProbeProvider {
        let environment = VesperIOSSessionProbeEnvironment.current()
        return { request in
            probe(request, environment: environment)
        }
    }

    static func probe(
        _ request: VesperPlaybackCapabilityProbeRequest,
        environment: VesperIOSSessionProbeEnvironment
    ) -> VesperPlaybackCapabilitySessionProbeResult {
        let hdrKind = request.codec.map(VesperPlaybackCapabilityProbe.detectHdrKind) ?? .none
        var supportedHdrKinds: Set<VesperPlaybackCapabilityHdrKind> = []
        if environment.hdrPlaybackEligible == true, hdrKind != .none, hdrKind != .unknown {
            supportedHdrKinds.insert(hdrKind)
        }

        var diagnostics: [String: String] = [
            "sessionProbe": "iosDisplayAndPlayerHdrEligibility",
            displayHdrProbeAvailableKey: "true",
            "displayGamut": environment.displayGamut.rawValue,
            "avPlayerEligibleForHDRPlayback": environment.hdrPlaybackEligible.map(String.init)
                ?? "unknown",
        ]
        if environment.hdrPlaybackEligible == true {
            diagnostics["hdrKindSupportBasis"] = "avPlayerEligibleForHDRPlayback"
        }
        if let maximumFramesPerSecond = environment.maximumFramesPerSecond {
            diagnostics["displayMaximumFramesPerSecond"] = String(maximumFramesPerSecond)
            if let frameRate = request.frameRate, frameRate > Double(maximumFramesPerSecond) + 0.01 {
                diagnostics[displayFrameRateSupportedKey] = "false"
            } else if request.frameRate != nil {
                diagnostics[displayFrameRateSupportedKey] = "true"
            }
        }
        if let nativeWidth = environment.nativeWidth {
            diagnostics["displayNativeWidth"] = String(nativeWidth)
        }
        if let nativeHeight = environment.nativeHeight {
            diagnostics["displayNativeHeight"] = String(nativeHeight)
        }
        if let width = request.width {
            diagnostics["requestedWidth"] = String(width)
        }
        if let height = request.height {
            diagnostics["requestedHeight"] = String(height)
        }
        if let frameRate = request.frameRate {
            diagnostics["requestedFrameRate"] = String(frameRate)
        }

        return VesperPlaybackCapabilitySessionProbeResult(
            supportedHdrKinds: supportedHdrKinds,
            diagnostics: diagnostics
        )
    }
}

enum VesperIOSAssetProbeProvider {
    static func probe(
        _ request: VesperPlaybackCapabilityProbeRequest
    ) async -> VesperPlaybackCapabilityAssetProbeResult? {
        guard let source = request.source,
            source.protocol == .file || source.protocol == .progressive || source.protocol == .hls
        else {
            return nil
        }
        guard let url = URL(string: source.uri) else {
            return VesperPlaybackCapabilityAssetProbeResult(
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetProbeError": "invalidSourceUrl",
                ]
            )
        }

        let asset = AVURLAsset(url: url)
        return await probe(asset)
    }

    static func probe(_ asset: AVAsset) async -> VesperPlaybackCapabilityAssetProbeResult {
        var diagnostics: [String: String] = [
            "assetProbe": "iosAVAsset",
            "assetProbeAvailable": "true",
        ]

        do {
            let isPlayable = try await asset.load(.isPlayable)
            diagnostics["assetPlayable"] = String(isPlayable)

            let videoTracks = try await asset.loadTracks(withMediaType: .video)
            diagnostics["assetVideoTrackCount"] = String(videoTracks.count)
            if let firstVideoTrack = videoTracks.first {
                diagnostics.merge(await videoDiagnostics(for: firstVideoTrack)) { _, new in new }
            }

            return VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: isPlayable,
                videoTrackCount: videoTracks.count,
                metadataHdrKind: VesperPlaybackCapabilityProbe.detectMetadataHdrKind(diagnostics),
                diagnostics: diagnostics
            )
        } catch {
            diagnostics["assetProbeError"] = String(describing: type(of: error))
            diagnostics["assetProbeErrorMessage"] = error.localizedDescription
            return VesperPlaybackCapabilityAssetProbeResult(diagnostics: diagnostics)
        }
    }

    private static func videoDiagnostics(for track: AVAssetTrack) async -> [String: String] {
        var diagnostics: [String: String] = [:]

        if let naturalSize = try? await track.load(.naturalSize) {
            let width = abs(Int(naturalSize.width.rounded()))
            let height = abs(Int(naturalSize.height.rounded()))
            if width > 0 {
                diagnostics["assetVideoWidth"] = String(width)
            }
            if height > 0 {
                diagnostics["assetVideoHeight"] = String(height)
            }
        }

        if let nominalFrameRate = try? await track.load(.nominalFrameRate),
            nominalFrameRate.isFinite,
            nominalFrameRate > 0
        {
            diagnostics["assetVideoFrameRate"] = String(Double(nominalFrameRate))
        }

        if let estimatedDataRate = try? await track.load(.estimatedDataRate),
            estimatedDataRate.isFinite,
            estimatedDataRate > 0
        {
            diagnostics["assetVideoEstimatedDataRate"] = String(Int(estimatedDataRate.rounded()))
        }

        if let formatDescription = (try? await track.load(.formatDescriptions))?.first {
            let mediaSubtype = CMFormatDescriptionGetMediaSubType(formatDescription)
            diagnostics["assetVideoCodec"] = playbackCapabilityFourCharCodeString(mediaSubtype)
            diagnostics.merge(formatDescriptionColorDiagnostics(formatDescription)) { _, new in new }
        }

        return diagnostics
    }

    private static func formatDescriptionColorDiagnostics(
        _ formatDescription: CMFormatDescription
    ) -> [String: String] {
        guard let extensions = CMFormatDescriptionGetExtensions(formatDescription) as? [String: Any]
        else {
            return [:]
        }

        var diagnostics: [String: String] = [:]
        copyExtension(
            kCMFormatDescriptionExtension_ColorPrimaries,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoColorPrimaries"
        )
        copyExtension(
            kCMFormatDescriptionExtension_TransferFunction,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoTransferFunction"
        )
        copyExtension(
            kCMFormatDescriptionExtension_YCbCrMatrix,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoYCbCrMatrix"
        )
        diagnostics.merge(VesperIOSHdrStaticMetadataDiagnostics.diagnostics(from: extensions)) { _, new in
            new
        }
        if diagnostics["assetVideoTransferFunction"] != nil ||
            diagnostics["assetVideoAlternativeTransferCharacteristics"] != nil ||
            diagnostics["assetVideoMasteringDisplayColorVolumePresent"] == "true" ||
            diagnostics["assetVideoContentLightLevelInfoPresent"] == "true"
        {
            diagnostics["assetVideoHdrMetadataProbe"] = "formatDescription"
        }
        return diagnostics
    }

    private static func copyExtension(
        _ key: CFString,
        from extensions: [String: Any],
        into diagnostics: inout [String: String],
        diagnosticKey: String
    ) {
        guard let value = extensions[key as String] else {
            return
        }
        diagnostics[diagnosticKey] = String(describing: value)
    }
}

enum VesperIOSHdrStaticMetadataDiagnostics {
    static func diagnostics(from extensions: [String: Any]) -> [String: String] {
        var diagnostics: [String: String] = [:]
        appendAlternativeTransferCharacteristics(from: extensions, into: &diagnostics)
        appendMasteringDisplayColorVolume(from: extensions, into: &diagnostics)
        appendContentLightLevelInfo(from: extensions, into: &diagnostics)
        return diagnostics
    }

    private static func appendAlternativeTransferCharacteristics(
        from extensions: [String: Any],
        into diagnostics: inout [String: String]
    ) {
        guard let value = extensions[kCMFormatDescriptionExtension_AlternativeTransferCharacteristics as String] else {
            return
        }
        diagnostics["assetVideoAlternativeTransferCharacteristics"] = String(describing: value)
    }

    private static func appendMasteringDisplayColorVolume(
        from extensions: [String: Any],
        into diagnostics: inout [String: String]
    ) {
        guard let data = dataValue(
            extensions[kCMFormatDescriptionExtension_MasteringDisplayColorVolume as String]
        ) else {
            return
        }
        diagnostics["assetVideoMasteringDisplayColorVolumePresent"] = "true"
        diagnostics["assetVideoMasteringDisplayColorVolumeByteLength"] = String(data.count)
        guard data.count >= 24 else {
            diagnostics["assetVideoMasteringDisplayColorVolumeParseError"] = "tooShort"
            return
        }

        let primary0X = readUInt16(data, offset: 0)
        let primary0Y = readUInt16(data, offset: 2)
        let primary1X = readUInt16(data, offset: 4)
        let primary1Y = readUInt16(data, offset: 6)
        let primary2X = readUInt16(data, offset: 8)
        let primary2Y = readUInt16(data, offset: 10)
        let whitePointX = readUInt16(data, offset: 12)
        let whitePointY = readUInt16(data, offset: 14)
        let maxLuminance = readUInt32(data, offset: 16)
        let minLuminance = readUInt32(data, offset: 20)

        diagnostics["assetVideoMasteringDisplayPrimary0"] = chromaticityPair(primary0X, primary0Y)
        diagnostics["assetVideoMasteringDisplayPrimary1"] = chromaticityPair(primary1X, primary1Y)
        diagnostics["assetVideoMasteringDisplayPrimary2"] = chromaticityPair(primary2X, primary2Y)
        diagnostics["assetVideoMasteringDisplayWhitePoint"] = chromaticityPair(whitePointX, whitePointY)
        diagnostics["assetVideoMasteringDisplayMaxLuminanceNits"] = String(maxLuminance)
        diagnostics["assetVideoMasteringDisplayMinLuminanceNits"] = decimalString(
            Double(minLuminance) / 10_000,
            digits: 4
        )
    }

    private static func appendContentLightLevelInfo(
        from extensions: [String: Any],
        into diagnostics: inout [String: String]
    ) {
        guard let data = dataValue(
            extensions[kCMFormatDescriptionExtension_ContentLightLevelInfo as String]
        ) else {
            return
        }
        diagnostics["assetVideoContentLightLevelInfoPresent"] = "true"
        diagnostics["assetVideoContentLightLevelInfoByteLength"] = String(data.count)
        guard data.count >= 4 else {
            diagnostics["assetVideoContentLightLevelInfoParseError"] = "tooShort"
            return
        }

        diagnostics["assetVideoMaxContentLightLevelNits"] = String(readUInt16(data, offset: 0))
        diagnostics["assetVideoMaxFrameAverageLightLevelNits"] = String(readUInt16(data, offset: 2))
    }

    private static func dataValue(_ value: Any?) -> Data? {
        if let data = value as? Data {
            return data
        }
        return (value as? NSData).map(Data.init)
    }

    private static func readUInt16(_ data: Data, offset: Int) -> UInt16 {
        (UInt16(data[offset]) << 8) | UInt16(data[offset + 1])
    }

    private static func readUInt32(_ data: Data, offset: Int) -> UInt32 {
        (UInt32(data[offset]) << 24) |
            (UInt32(data[offset + 1]) << 16) |
            (UInt32(data[offset + 2]) << 8) |
            UInt32(data[offset + 3])
    }

    private static func chromaticityPair(_ x: UInt16, _ y: UInt16) -> String {
        "\(decimalString(Double(x) / 50_000, digits: 5)),\(decimalString(Double(y) / 50_000, digits: 5))"
    }

    private static func decimalString(_ value: Double, digits: Int) -> String {
        String(format: "%.\(digits)f", locale: Locale(identifier: "en_US_POSIX"), value)
    }
}

private let displayHdrProbeAvailableKey = "displayHdrProbeAvailable"
private let displayFrameRateSupportedKey = "displayFrameRateSupported"

private extension Dictionary where Key == String, Value == String {
    func firstString(_ keys: String...) -> String? {
        keys.compactMap { stringValue($0) }.first
    }

    func firstInt(_ keys: String...) -> Int? {
        keys.compactMap { intValue($0) }.first
    }

    func stringValue(_ key: String) -> String? {
        guard let value = self[key], !value.isEmpty else {
            return nil
        }
        return value
    }

    func boolValue(_ key: String) -> Bool? {
        guard let value = stringValue(key) else {
            return nil
        }
        switch value {
        case "true":
            return true
        case "false":
            return false
        default:
            return nil
        }
    }

    func intValue(_ key: String) -> Int? {
        guard let value = stringValue(key) else {
            return nil
        }
        return Int(value)
    }

    func doubleValue(_ key: String) -> Double? {
        guard let value = stringValue(key), let parsed = Double(value), parsed.isFinite else {
            return nil
        }
        return parsed
    }

    func chromaticityPoint(_ key: String) -> VesperHdrChromaticityPoint? {
        guard let value = stringValue(key) else {
            return nil
        }
        let parts = value.split(separator: ",")
        guard parts.count == 2,
              let x = Double(parts[0].trimmingCharacters(in: .whitespacesAndNewlines)),
              let y = Double(parts[1].trimmingCharacters(in: .whitespacesAndNewlines))
        else {
            return nil
        }
        return VesperHdrChromaticityPoint(x: x, y: y)
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

private func playbackCapabilityFourCharCodeString(_ value: UInt32) -> String {
    let scalarValues = [
        UInt8((value >> 24) & 0xFF),
        UInt8((value >> 16) & 0xFF),
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
    let printable = scalarValues.allSatisfy { (0x20 ... 0x7E).contains($0) }
    guard printable else {
        return String(format: "0x%08X", value)
    }
    return String(bytes: scalarValues, encoding: .ascii) ?? String(format: "0x%08X", value)
}
