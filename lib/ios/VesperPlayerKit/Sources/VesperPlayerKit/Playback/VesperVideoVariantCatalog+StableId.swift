@preconcurrency import AVFoundation
import CoreGraphics
import Foundation

func stableVideoVariantTrackId(
    codec: String?,
    peakBitRate: Int64?,
    width: Int?,
    height: Int?,
    frameRate: Double?
) -> String {
    let frameRateBucket = frameRate.flatMap { value -> Int? in
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int((value * 100).rounded())
    }

    let components = [
        "c\(sanitizedStableVideoVariantTrackIdComponent(codec))",
        "b\(peakBitRate.map(String.init) ?? "na")",
        "w\(width.map(String.init) ?? "na")",
        "h\(height.map(String.init) ?? "na")",
        "f\(frameRateBucket.map(String.init) ?? "na")",
    ]
    return "video:hls:" + components.joined(separator: ":")
}

func sanitizedStableVideoVariantTrackIdComponent(_ value: String?) -> String {
    let rawValue = value?.lowercased() ?? "na"
    let sanitizedScalars = rawValue.unicodeScalars.map { scalar -> UnicodeScalar in
        if CharacterSet.alphanumerics.contains(scalar) {
            return scalar
        }
        return "_"
    }
    let sanitized = String(String.UnicodeScalarView(sanitizedScalars))
        .replacingOccurrences(of: "_+", with: "_", options: .regularExpression)
        .trimmingCharacters(in: CharacterSet(charactersIn: "_"))
    return sanitized.isEmpty ? "na" : sanitized
}

struct StableVideoVariantFingerprint {
    let codecComponent: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRateBucket: Int?

    init?(trackId: String) {
        let components = trackId.split(separator: ":")
        guard components.count >= 7, components[0] == "video", components[1] == "hls" else {
            return nil
        }

        var codecComponent: String?
        var peakBitRate: Int64?
        var width: Int?
        var height: Int?
        var frameRateBucket: Int?

        for component in components.dropFirst(2) {
            guard let prefix = component.first else {
                continue
            }
            let rawValue = String(component.dropFirst())
            switch prefix {
            case "c":
                codecComponent = rawValue == "na" ? nil : rawValue
            case "b":
                peakBitRate = rawValue == "na" ? nil : Int64(rawValue)
            case "w":
                width = rawValue == "na" ? nil : Int(rawValue)
            case "h":
                height = rawValue == "na" ? nil : Int(rawValue)
            case "f":
                frameRateBucket = rawValue == "na" ? nil : Int(rawValue)
            default:
                continue
            }
        }

        self.codecComponent = codecComponent
        self.peakBitRate = peakBitRate
        self.width = width
        self.height = height
        self.frameRateBucket = frameRateBucket
    }

    init(track: VesperMediaTrack) {
        codecComponent = track.codec.map(sanitizedStableVideoVariantTrackIdComponent)
        peakBitRate = track.bitRate
        width = track.width
        height = track.height
        frameRateBucket = track.frameRate.flatMap { value in
            guard value.isFinite, value > 0 else {
                return nil
            }
            return Int((Double(value) * 100).rounded())
        }
    }

    var hasComparableFields: Bool {
        codecComponent != nil ||
            peakBitRate != nil ||
            width != nil ||
            height != nil ||
            frameRateBucket != nil
    }
}
