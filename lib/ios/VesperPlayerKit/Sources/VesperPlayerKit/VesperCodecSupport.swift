import CoreMedia
import Foundation
import VideoToolbox

public enum VesperCodecSupport {
    public static func hardwareDecodeSupported(for codec: String) -> Bool {
        guard let codecType = VesperHardwareDecodeCandidateCodec(codecName: codec).videoCodecType else {
            return false
        }
        return VTIsHardwareDecodeSupported(codecType)
    }
}

enum VesperHardwareDecodeCandidateCodec: Equatable {
    case vvc
    case av1
    case h264
    case hevc
    case unknown

    init(codecName: String) {
        let normalizedCodecs = codecName
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
        self = normalizedCodecs.lazy.compactMap(Self.videoCodecFamily).first ?? .unknown
    }

    private static func videoCodecFamily(_ value: String) -> VesperHardwareDecodeCandidateCodec? {
        let codec = value.hasPrefix("video/")
            ? String(value.dropFirst("video/".count))
            : value
        if codec.hasPrefix("vvc1") || codec.hasPrefix("vvi1") || codec == "vvc" || codec == "h266" {
            return .vvc
        }
        if codec.hasPrefix("av01") || codec == "av1" {
            return .av1
        }
        if codec.hasPrefix("hvc1") || codec.hasPrefix("hev1") || codec == "hevc" || codec == "h265" {
            return .hevc
        }
        if codec.hasPrefix("avc1") || codec.hasPrefix("avc3") || codec == "avc" || codec == "h264" {
            return .h264
        }
        return nil
    }

    var videoCodecType: CMVideoCodecType? {
        switch self {
        case .vvc:
            return nil
        case .av1:
            return CMVideoCodecType(kCMVideoCodecType_AV1)
        case .h264:
            return CMVideoCodecType(kCMVideoCodecType_H264)
        case .hevc:
            return CMVideoCodecType(kCMVideoCodecType_HEVC)
        case .unknown:
            return nil
        }
    }

    var dashCodecFamily: VesperDashVideoCodecFamily {
        switch self {
        case .vvc:
            return .vvc
        case .av1:
            return .av1
        case .hevc:
            return .hevc
        case .h264:
            return .avc
        case .unknown:
            return .unknown
        }
    }
}
