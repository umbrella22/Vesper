@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

struct VesperMp4BoxHeader {
    let boxType: [UInt8]
    let end: Int

    static func parse(bytes: [UInt8], start: Int) throws -> VesperMp4BoxHeader {
        let remaining = bytes.count - start
        guard remaining >= 8 else {
            throw VesperDashBridgeError.invalidMp4("truncated MP4 box header")
        }
        let size32 = try readBigEndianUInt32(bytes, offset: start, field: "MP4 box size")
        let boxType = Array(bytes[(start + 4)..<(start + 8)])
        let boxSize: Int
        let headerSize: Int
        if size32 == 0 {
            boxSize = remaining
            headerSize = 8
        } else if size32 == 1 {
            guard remaining >= 16 else {
                throw VesperDashBridgeError.invalidMp4("truncated extended MP4 box header")
            }
            let size64 = try readBigEndianUInt64(bytes, offset: start + 8, field: "extended MP4 box size")
            boxSize = try checkedInt(size64, field: "extended MP4 box size")
            headerSize = 16
        } else {
            boxSize = Int(size32)
            headerSize = 8
        }
        guard boxSize >= headerSize else {
            throw VesperDashBridgeError.invalidMp4("MP4 box size is smaller than its header")
        }
        guard boxSize <= remaining else {
            throw VesperDashBridgeError.invalidMp4("MP4 box exceeds input data")
        }
        return VesperMp4BoxHeader(
            boxType: boxType,
            end: start + boxSize
        )
    }
}

func resolveDashURI(base: String, reference: String) -> String {
    let reference = reference.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !reference.isEmpty else { return base }
    if URL(string: reference)?.scheme != nil {
        return reference
    }
    guard let baseURL = URL(string: base),
          let resolved = URL(string: reference, relativeTo: baseURL)?.absoluteURL
    else {
        return reference
    }
    return resolved.absoluteString
}

func dashSegmentTypeName(_ representation: VesperDashRepresentation) -> String {
    if representation.segmentTemplate != nil {
        return "template"
    }
    if representation.segmentBase != nil {
        return "base"
    }
    return "unknown"
}

func emptyAsNil(_ value: String) -> String {
    value.isEmpty ? "nil" : value
}

func dashSegmentKindName(_ segment: VesperDashSegmentRequest) -> String {
    switch segment {
    case .initialization:
        return "initialization"
    case .media:
        return "media"
    }
}

func applyHttpHeaders(_ headers: [String: String], to request: inout URLRequest) {
    for (field, value) in headers where !field.isEmpty {
        request.setValue(value, forHTTPHeaderField: field)
    }
    if request.value(forHTTPHeaderField: "Accept-Encoding") == nil {
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
    }
}

let vesperAVURLAssetHTTPHeaderFieldsKey = "AVURLAssetHTTPHeaderFieldsKey"

let dashPathComponentAllowedCharacters: CharacterSet = {
    var characters = CharacterSet.urlPathAllowed
    characters.remove(charactersIn: "/")
    return characters
}()

func checkedInt(_ value: UInt64, field: String) throws -> Int {
    guard value <= UInt64(Int.max) else {
        throw VesperDashBridgeError.invalidMp4("\(field) exceeds Int.max")
    }
    return Int(value)
}

func closeFileHandle(_ handle: FileHandle, context: String) {
    do {
        try handle.close()
    } catch {
        iosHostLog("failed to close \(context) file handle: \(error.localizedDescription)")
    }
}

func removeFileIfPresent(_ url: URL, context: String) {
    guard FileManager.default.fileExists(atPath: url.path) else {
        return
    }
    do {
        try FileManager.default.removeItem(at: url)
    } catch {
        iosHostLog("failed to remove \(context): \(error.localizedDescription)")
    }
}

func startupPrefetchSegmentIndices(count: Int) -> [Int] {
    guard count > 0 else {
        return []
    }
    let candidates = [
        0,
        min(1, count - 1),
        min((count + 2) / 3, count - 1),
        min(((count * 2) + 2) / 3, count - 1),
    ]
    return Array(Set(candidates)).sorted()
}

func backgroundPrefetchRequests(
    count: Int,
    includeMediaSegments: Bool = true
) -> [VesperDashSegmentRequest] {
    guard includeMediaSegments, count > 0 else {
        return [.initialization]
    }
    let prioritized = startupPrefetchSegmentIndices(count: count)
    let orderedIndices = prioritized + (0..<count).filter { !prioritized.contains($0) }
    return [.initialization] + orderedIndices.map(VesperDashSegmentRequest.media)
}

func readBigEndianUInt32(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt32 {
    guard offset >= 0, offset + 4 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    return (UInt32(bytes[offset]) << 24)
        | (UInt32(bytes[offset + 1]) << 16)
        | (UInt32(bytes[offset + 2]) << 8)
        | UInt32(bytes[offset + 3])
}

func readBigEndianUInt64(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt64 {
    guard offset >= 0, offset + 8 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    var value: UInt64 = 0
    for byte in bytes[offset..<(offset + 8)] {
        value = (value << 8) | UInt64(byte)
    }
    return value
}

extension UInt64 {
    func dashSaturatingAdd(_ rhs: UInt64) -> UInt64 {
        let (value, overflow) = addingReportingOverflow(rhs)
        return overflow ? UInt64.max : value
    }

    func dashSaturatingSubtract(_ rhs: UInt64) -> UInt64 {
        let (value, overflow) = subtractingReportingOverflow(rhs)
        return overflow ? 0 : value
    }
}
