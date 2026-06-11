import AVFoundation
import Foundation

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
