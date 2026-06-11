import Foundation

let displayHdrProbeAvailableKey = "displayHdrProbeAvailable"
let displayFrameRateSupportedKey = "displayFrameRateSupported"

extension Dictionary where Key == String, Value == String {
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
    init(candidate: VesperHardwareDecodeCandidateCodec) {
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

func playbackCapabilityFourCharCodeString(_ value: UInt32) -> String {
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
