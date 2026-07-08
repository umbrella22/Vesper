import Foundation

extension VesperDownloadManager {
    func preparedDownloadOutputURL(
        taskId: VesperDownloadTaskId,
        fileName: String?
    ) throws -> URL {
        try prepareDownloadOutputURLFromSource(
            sourceURL: outputURL(forTask: taskId),
            fileName: fileName
        )
    }

    func downloadOutputURL(from path: String) -> URL {
        if let url = URL(string: path), url.isFileURL {
            return url
        }
        return URL(fileURLWithPath: path)
    }
}

func prepareDownloadOutputURLFromSource(
    sourceURL: URL,
    fileName: String?
) throws -> URL {
    guard let fileName, !fileName.isEmpty else {
        return sourceURL
    }
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("vesper-download-share", isDirectory: true)
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let targetURL = directory.appendingPathComponent(sanitizedOutputFileName(fileName))
    try FileManager.default.copyItem(at: sourceURL, to: targetURL)
    return targetURL
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
