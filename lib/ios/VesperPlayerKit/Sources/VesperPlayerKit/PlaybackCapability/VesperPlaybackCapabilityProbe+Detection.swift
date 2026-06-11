import Foundation
extension VesperPlaybackCapabilityProbe {
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
