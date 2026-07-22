@preconcurrency import AVFoundation
import Foundation
internal import VesperPlayerKitBridgeShim

extension VesperDashSession {
#if DEBUG
    func logTopLevelBoxes(
        data: Data,
        label: String,
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) {
        let bytes = [UInt8](data.prefix(4_096))
        var cursor = 0
        var types: [String] = []
        while cursor < bytes.count, types.count < 8 {
            guard let header = try? VesperMp4BoxHeader.parse(bytes: bytes, start: cursor) else { break }
            let typeString = String(bytes: header.boxType, encoding: .ascii) ?? "????"
            types.append(typeString)
            if header.end <= cursor { break }
            cursor = header.end
        }
        iosHostLog(
            "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(data.count) topBoxes=\(types.joined(separator: ","))"
        )
    }

    func logTopLevelBoxes(
        fileURL: URL,
        totalBytes: UInt64,
        label: String,
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) {
        guard
            let handle = try? FileHandle(forReadingFrom: fileURL),
            let data = try? handle.read(upToCount: 4_096)
        else {
            iosHostLog(
                "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(totalBytes) topBoxes=<unreadable>"
            )
            return
        }
        closeFileHandle(handle, context: "\(label) MP4 box inspection")
        let bytes = [UInt8](data)
        var cursor = 0
        var types: [String] = []
        while cursor < bytes.count, types.count < 8 {
            guard let header = try? VesperMp4BoxHeader.parse(bytes: bytes, start: cursor) else { break }
            let typeString = String(bytes: header.boxType, encoding: .ascii) ?? "????"
            types.append(typeString)
            if header.end <= cursor { break }
            cursor = header.end
        }
        iosHostLog(
            "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(totalBytes) topBoxes=\(types.joined(separator: ","))"
        )
    }
#endif
}
