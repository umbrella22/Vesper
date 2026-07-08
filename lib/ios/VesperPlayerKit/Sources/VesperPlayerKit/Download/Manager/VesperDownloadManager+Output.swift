import Foundation
#if canImport(UIKit)
import UIKit
#endif

extension VesperDownloadManager {
    public func exportTaskOutput(
        taskId: VesperDownloadTaskId,
        outputPath: String,
        onProgress: @escaping (Float) -> Void = { _ in },
        isCancelled: @escaping () -> Bool = { false }
    ) async throws {
        guard sessionHandle != 0 else {
            throw DownloadExportBridgeError("native download session handle must not be zero")
        }

        let bindings = self.bindings
        let sessionHandle = self.sessionHandle
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                do {
                    try bindings.exportDownloadTask(
                        sessionHandle: sessionHandle,
                        taskId: taskId,
                        outputPath: outputPath,
                        onProgress: onProgress,
                        isCancelled: isCancelled
                    )
                    continuation.resume(returning: ())
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func outputURL(forTask taskId: VesperDownloadTaskId) throws -> URL {
        guard let task = task(taskId) else {
            throw DownloadExportBridgeError("download task \(taskId) was not found")
        }
        guard task.state == .completed else {
            throw DownloadExportBridgeError("download task \(taskId) must be completed before sharing or saving")
        }
        guard let completedPath = task.assetIndex.completedPath, !completedPath.isEmpty else {
            throw DownloadExportBridgeError("download task \(taskId) does not have an output file")
        }
        let url = downloadOutputURL(from: completedPath)
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw DownloadExportBridgeError("download task output file does not exist")
        }
        return url
    }

    #if canImport(UIKit)
    public func prepareTaskOutputURL(
        taskId: VesperDownloadTaskId,
        fileName: String? = nil
    ) async throws -> URL {
        let sourceURL = try outputURL(forTask: taskId)
        return try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                do {
                    let url = try prepareDownloadOutputURLFromSource(
                        sourceURL: sourceURL,
                        fileName: fileName
                    )
                    continuation.resume(returning: url)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func shareTaskOutput(
        taskId: VesperDownloadTaskId,
        fileName: String? = nil,
        mimeType: String? = nil,
        from presenter: UIViewController
    ) throws {
        _ = mimeType
        let url = try preparedDownloadOutputURL(taskId: taskId, fileName: fileName)
        sharePreparedTaskOutput(url, mimeType: mimeType, from: presenter)
    }

    public func sharePreparedTaskOutput(
        _ url: URL,
        mimeType: String? = nil,
        from presenter: UIViewController
    ) {
        _ = mimeType
        let controller = UIActivityViewController(activityItems: [url], applicationActivities: nil)
        if let popover = controller.popoverPresentationController {
            popover.sourceView = presenter.view
            popover.sourceRect = CGRect(
                x: presenter.view.bounds.midX,
                y: presenter.view.bounds.midY,
                width: 1,
                height: 1
            )
            popover.permittedArrowDirections = []
        }
        presenter.present(controller, animated: true)
    }

    @discardableResult
    public func saveTaskOutput(
        taskId: VesperDownloadTaskId,
        fileName: String? = nil,
        from presenter: UIViewController
    ) throws -> URL {
        let url = try preparedDownloadOutputURL(taskId: taskId, fileName: fileName)
        return savePreparedTaskOutput(url, from: presenter)
    }

    @discardableResult
    public func savePreparedTaskOutput(
        _ url: URL,
        from presenter: UIViewController
    ) -> URL {
        let picker = UIDocumentPickerViewController(forExporting: [url], asCopy: true)
        presenter.present(picker, animated: true)
        return url
    }
    #endif
}
