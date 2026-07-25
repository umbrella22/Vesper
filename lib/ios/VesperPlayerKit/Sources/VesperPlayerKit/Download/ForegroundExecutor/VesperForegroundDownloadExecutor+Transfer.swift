import Foundation

extension VesperForegroundDownloadExecutor {
    func fetch(
        _ sourceURL: URL,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: [String: String],
        expectedSizeBytes: UInt64?,
        resumeFromBytes: UInt64,
        to destinationURL: URL,
        allowRestartAfterRangeMismatch: Bool = true,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        if let expectedSizeBytes, resumeFromBytes >= expectedSizeBytes {
            return expectedSizeBytes
        }

        if sourceURL.isFileURL {
            return try await copyFileURL(
                sourceURL,
                byteRange: byteRange,
                expectedSizeBytes: expectedSizeBytes,
                resumeFromBytes: resumeFromBytes,
                to: destinationURL,
                onProgress: onProgress
            )
        }
        if byteRange == nil, let expectedSizeBytes, expectedSizeBytes > 0, let rangeChunkBytes {
            return try await fetchKnownSizeHTTPResource(
                sourceURL,
                requestHeaders: requestHeaders,
                expectedSizeBytes: expectedSizeBytes,
                resumeFromBytes: resumeFromBytes,
                rangeChunkBytes: rangeChunkBytes,
                to: destinationURL,
                allowRestartAfterRangeMismatch: allowRestartAfterRangeMismatch,
                onProgress: onProgress
            )
        }

        let sourceDescription = downloadURLDescriptionForDiagnostics(sourceURL)
        var request = URLRequest(url: sourceURL)
        request.applyDownloadHttpHeaders(requestHeaders)
        var requestedRangeStart: UInt64?
        var requestedRangeEndInclusive: UInt64?
        var expectedResponseBodyBytes: UInt64?
        if let byteRange {
            guard resumeFromBytes < byteRange.length else {
                return byteRange.length
            }
            let remaining = byteRange.length > resumeFromBytes ? byteRange.length - resumeFromBytes : 0
            let start = byteRange.offset + resumeFromBytes
            let end = remaining == 0 ? start : start + remaining - 1
            request.setValue("bytes=\(start)-\(end)", forHTTPHeaderField: "Range")
            requestedRangeStart = start
            requestedRangeEndInclusive = end
            expectedResponseBodyBytes = remaining
        } else if resumeFromBytes > 0 {
            request.setValue("bytes=\(resumeFromBytes)-", forHTTPHeaderField: "Range")
            requestedRangeStart = resumeFromBytes
            requestedRangeEndInclusive = expectedSizeBytes.flatMap { $0 > 0 ? $0 - 1 : nil }
            expectedResponseBodyBytes = expectedSizeBytes.map { $0 > resumeFromBytes ? $0 - resumeFromBytes : 0 }
        }

        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }
        var expectedFinalBytesAfterResponse: UInt64?
        let response = stream.response
        if let http = response as? HTTPURLResponse {
            switch http.statusCode {
            case 206:
                guard let requestedRangeStart else {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server returned an unexpected Content-Range for \(sourceDescription)"
                    )
                }
                let contentRange = try validateHTTPPartialContentRange(
                    contentRangeHeader: http.value(forHTTPHeaderField: "Content-Range"),
                    contentLengthHeader: http.value(forHTTPHeaderField: "Content-Length"),
                    requestedStart: requestedRangeStart,
                    requestedEndInclusive: requestedRangeEndInclusive,
                    expectedBodyLength: expectedResponseBodyBytes,
                    expectedTotalSizeBytes: byteRange == nil ? expectedSizeBytes : nil,
                    sourceURL: sourceURL
                )
                if let responseBytes = contentRange.length {
                    expectedFinalBytesAfterResponse = resumeFromBytes + responseBytes
                }
            case 200:
                if requestedRangeStart != nil {
                    if byteRange == nil, resumeFromBytes > 0, allowRestartAfterRangeMismatch {
                        try? fileManager.removeItem(at: destinationURL)
                        await onProgress(0)
                        return try await fetch(
                            sourceURL,
                            byteRange: byteRange,
                            requestHeaders: requestHeaders,
                            expectedSizeBytes: expectedSizeBytes,
                            resumeFromBytes: 0,
                            to: destinationURL,
                            allowRestartAfterRangeMismatch: false,
                            onProgress: onProgress
                        )
                    }
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server did not honor the requested byte range for \(sourceDescription)"
                    )
                }
                if let expectedSizeBytes,
                   let contentLength = parseHttpContentLength(http.value(forHTTPHeaderField: "Content-Length")),
                   contentLength != expectedSizeBytes {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server reported Content-Length \(contentLength), expected \(expectedSizeBytes) for \(sourceDescription)"
                    )
                }
            case 416:
                if resumeFromBytes > 0, allowRestartAfterRangeMismatch {
                    try? fileManager.removeItem(at: destinationURL)
                    await onProgress(0)
                    return try await fetch(
                        sourceURL,
                        byteRange: byteRange,
                        requestHeaders: requestHeaders,
                        expectedSizeBytes: expectedSizeBytes,
                        resumeFromBytes: 0,
                        to: destinationURL,
                        allowRestartAfterRangeMismatch: false,
                        onProgress: onProgress
                    )
                }
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "remote resource rejected the requested byte range for \(sourceDescription)"
                )
            case 401, 403, 404, 410:
                throw expiredDownloadResource(
                    sourceURL: sourceURL,
                    statusCode: http.statusCode,
                    phase: .download
                )
            case 200..<300:
                break
            default:
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "remote resource returned HTTP \(http.statusCode) for \(sourceDescription)"
                )
            }
        }

        if !fileManager.fileExists(atPath: destinationURL.path) {
            fileManager.createFile(atPath: destinationURL.path, contents: nil)
        }
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { closeDownloadFileHandle(output, context: "streamed resource output") }
        if resumeFromBytes > 0 {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = resumeFromBytes
        var lastCleanFileSize = resumeFromBytes
        var buffer = Data()
        buffer.reserveCapacity(64 * 1024)

        do {
            for try await data in stream.chunks {
                try Task.checkCancellation()
                buffer.append(data)
                if buffer.count >= 64 * 1024 {
                    try output.write(contentsOf: buffer)
                    totalWritten += UInt64(buffer.count)
                    lastCleanFileSize = totalWritten
                    if let expectedFinalBytesAfterResponse,
                       totalWritten > expectedFinalBytesAfterResponse {
                        try? fileManager.removeItem(at: destinationURL)
                        throw VesperForegroundDownloadPreparationError.invalidSource(
                            "remote server sent more bytes than its Content-Range for \(sourceDescription)"
                        )
                    }
                    if let expectedSizeBytes, totalWritten > expectedSizeBytes {
                        try? fileManager.removeItem(at: destinationURL)
                        throw VesperForegroundDownloadPreparationError.invalidSource(
                            "remote server sent more bytes than expected for \(sourceDescription)"
                        )
                    }
                    buffer.removeAll(keepingCapacity: true)
                    await onProgress(totalWritten)
                }
            }
            if !buffer.isEmpty {
                try output.write(contentsOf: buffer)
                totalWritten += UInt64(buffer.count)
                lastCleanFileSize = totalWritten
                if let expectedFinalBytesAfterResponse,
                   totalWritten > expectedFinalBytesAfterResponse {
                    try? fileManager.removeItem(at: destinationURL)
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server sent more bytes than its Content-Range for \(sourceDescription)"
                    )
                }
                if let expectedSizeBytes, totalWritten > expectedSizeBytes {
                    try? fileManager.removeItem(at: destinationURL)
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server sent more bytes than expected for \(sourceDescription)"
                    )
                }
                buffer.removeAll(keepingCapacity: true)
                await onProgress(totalWritten)
            }
        } catch {
            try? truncateFile(at: destinationURL, to: lastCleanFileSize)
            throw error
        }

        if let expectedFinalBytesAfterResponse,
           totalWritten != expectedFinalBytesAfterResponse {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "downloaded \(totalWritten) bytes after resume, expected \(expectedFinalBytesAfterResponse)"
            )
        }

        if let expectedSizeBytes, totalWritten != expectedSizeBytes {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "downloaded \(totalWritten) bytes, expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
    }
}
