import Foundation

extension VesperForegroundDownloadExecutor {
    func truncateFile(at url: URL, to size: UInt64) throws {
        guard fileManager.fileExists(atPath: url.path) else {
            return
        }
        let output = try FileHandle(forWritingTo: url)
        defer { closeDownloadFileHandle(output, context: "download file truncation") }
        try output.truncate(atOffset: size)
    }

    func copyFileURL(
        _ sourceURL: URL,
        byteRange: VesperDownloadByteRange?,
        expectedSizeBytes: UInt64?,
        resumeFromBytes: UInt64,
        to destinationURL: URL,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        if !fileManager.fileExists(atPath: destinationURL.path) {
            fileManager.createFile(atPath: destinationURL.path, contents: nil)
        }

        let input = try FileHandle(forReadingFrom: sourceURL)
        let output = try FileHandle(forWritingTo: destinationURL)
        defer {
            closeDownloadFileHandle(input, context: "local file input")
            closeDownloadFileHandle(output, context: "local file output")
        }

        try input.seek(toOffset: (byteRange?.offset ?? 0) + resumeFromBytes)
        if resumeFromBytes > 0 {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = resumeFromBytes
        var lastCleanFileSize = resumeFromBytes
        var remaining = byteRange.map { $0.length > resumeFromBytes ? $0.length - resumeFromBytes : 0 }
        do {
            while remaining == nil || remaining! > 0 {
                try Task.checkCancellation()
                let chunkSize = Int(min(UInt64(64 * 1024), remaining ?? UInt64(64 * 1024)))
                let data = try input.read(upToCount: chunkSize) ?? Data()
                if data.isEmpty {
                    break
                }
                try output.write(contentsOf: data)
                let count = UInt64(data.count)
                totalWritten += count
                lastCleanFileSize = totalWritten
                if let currentRemaining = remaining {
                    remaining = currentRemaining > count ? currentRemaining - count : 0
                }
                await onProgress(totalWritten)
            }
        } catch {
            try? truncateFile(at: destinationURL, to: lastCleanFileSize)
            throw error
        }

        if let expectedSizeBytes, totalWritten != expectedSizeBytes {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "copied \(totalWritten) bytes, expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
    }

    func resumableExistingBytes(
        at destinationURL: URL,
        expectedSizeBytes: UInt64?
    ) -> UInt64 {
        guard fileManager.fileExists(atPath: destinationURL.path) else {
            return 0
        }
        guard resumePartialDownloads else {
            try? fileManager.removeItem(at: destinationURL)
            return 0
        }
        guard let expectedSizeBytes else {
            try? fileManager.removeItem(at: destinationURL)
            return 0
        }

        let existingBytes = (try? destinationURL.resourceValues(forKeys: [.fileSizeKey]).fileSize)
            .map { UInt64(max($0, 0)) } ?? 0
        if existingBytes == expectedSizeBytes {
            return existingBytes
        }
        if expectedSizeBytes > 1 && existingBytes > 0 && existingBytes < expectedSizeBytes {
            return existingBytes
        }
        try? fileManager.removeItem(at: destinationURL)
        return 0
    }
}
