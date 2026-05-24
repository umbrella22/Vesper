import Foundation

extension VesperDownloadManager {
    func preparedDownloadOutputURL(
        taskId: VesperDownloadTaskId,
        fileName: String?
    ) throws -> URL {
        let sourceURL = try outputURL(forTask: taskId)
        guard let fileName, !fileName.isEmpty else {
            return sourceURL
        }
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-download-share", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let targetURL = directory.appendingPathComponent(sanitizedOutputFileName(fileName))
        if FileManager.default.fileExists(atPath: targetURL.path) {
            try FileManager.default.removeItem(at: targetURL)
        }
        try FileManager.default.copyItem(at: sourceURL, to: targetURL)
        return targetURL
    }

    func downloadOutputURL(from path: String) -> URL {
        if let url = URL(string: path), url.isFileURL {
            return url
        }
        return URL(fileURLWithPath: path)
    }
}

func sanitizedOutputFileName(_ value: String) -> String {
    let sanitized = value
        .replacingOccurrences(of: "[^A-Za-z0-9._ -]+", with: "_", options: .regularExpression)
        .trimmingCharacters(in: CharacterSet(charactersIn: ". "))
    return sanitized.isEmpty || sanitized == ".." ? "vesper-download" : sanitized
}

func excludeDownloadItemFromBackup(_ url: URL, fileManager: FileManager = .default) {
    guard fileManager.fileExists(atPath: url.path) else {
        return
    }
    var excludedURL = url
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    do {
        try excludedURL.setResourceValues(values)
    } catch {
        iosHostLog("failed to exclude download item from iCloud backup: \(error.localizedDescription)")
    }
}
