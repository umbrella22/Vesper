import Foundation
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
struct DolbyVisionProfile8BaseLayerEvidence: Equatable {
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

extension Dictionary where Key == String, Value == String {
    func withDolbyVisionProfile8Refinement() -> [String: String] {
        var values = self
        VesperPlaybackCapabilityProbe.applyDolbyVisionProfile8Refinement(to: &values)
        return values
    }
}
