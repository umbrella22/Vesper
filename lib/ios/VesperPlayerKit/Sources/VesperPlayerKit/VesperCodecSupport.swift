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
    case h264
    case hevc
    case unknown

    init(codecName: String) {
        switch codecName.trimmingCharacters(in: .whitespacesAndNewlines).uppercased() {
        case "H264", "AVC", "AVC1":
            self = .h264
        case "HEVC", "H265", "HVC1", "HEV1":
            self = .hevc
        default:
            self = .unknown
        }
    }

    var videoCodecType: CMVideoCodecType? {
        switch self {
        case .h264:
            return CMVideoCodecType(kCMVideoCodecType_H264)
        case .hevc:
            return CMVideoCodecType(kCMVideoCodecType_HEVC)
        case .unknown:
            return nil
        }
    }
}
