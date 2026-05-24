import Foundation
import VesperPlayerKitBridgeShim

final class DownloadExportProgressBridge: @unchecked Sendable {
    let onProgress: (Float) -> Void
    let isCancelled: () -> Bool

    init(
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) {
        self.onProgress = onProgress
        self.isCancelled = isCancelled
    }

    func retainContext() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque())
    }

    static func releaseContext(_ context: UnsafeMutableRawPointer?) {
        guard let context else {
            return
        }
        Unmanaged<DownloadExportProgressBridge>.fromOpaque(context).release()
    }

    static func fromContext(_ context: UnsafeMutableRawPointer?) -> DownloadExportProgressBridge? {
        guard let context else {
            return nil
        }
        return Unmanaged<DownloadExportProgressBridge>.fromOpaque(context).takeUnretainedValue()
    }
}

struct DownloadExportBridgeError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}
