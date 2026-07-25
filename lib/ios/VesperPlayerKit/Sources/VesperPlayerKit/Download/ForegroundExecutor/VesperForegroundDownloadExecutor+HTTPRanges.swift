import Foundation

extension VesperForegroundDownloadExecutor {
    func fetchKnownSizeHTTPResource(
        _ sourceURL: URL,
        requestHeaders: [String: String],
        expectedSizeBytes: UInt64,
        resumeFromBytes: UInt64,
        rangeChunkBytes: UInt64,
        to destinationURL: URL,
        allowRestartAfterRangeMismatch: Bool,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        let sourceDescription = downloadURLDescriptionForDiagnostics(sourceURL)
        var offset = resumeFromBytes
        if offset >= expectedSizeBytes {
            return expectedSizeBytes
        }
        while offset < expectedSizeBytes {
            let chunkLength = min(rangeChunkBytes, expectedSizeBytes - offset)
            let chunkEnd = offset + chunkLength - 1
            let nextOffset = try await fetchHTTPRangeChunk(
                sourceURL,
                requestHeaders: requestHeaders,
                expectedSizeBytes: expectedSizeBytes,
                rangeStart: offset,
                rangeEndInclusive: chunkEnd,
                rangeChunkBytes: rangeChunkBytes,
                to: destinationURL,
                allowRestartAfterRangeMismatch: allowRestartAfterRangeMismatch,
                onProgress: onProgress
            )
            guard nextOffset > offset else {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "download range transfer did not advance for \(sourceDescription)"
                )
            }
            offset = nextOffset
        }
        return offset
    }

    func fetchHTTPRangeChunk(
        _ sourceURL: URL,
        requestHeaders: [String: String],
        expectedSizeBytes: UInt64,
        rangeStart: UInt64,
        rangeEndInclusive: UInt64,
        rangeChunkBytes: UInt64,
        to destinationURL: URL,
        allowRestartAfterRangeMismatch: Bool,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        let sourceDescription = downloadURLDescriptionForDiagnostics(sourceURL)
        var request = URLRequest(url: sourceURL)
        request.applyDownloadHttpHeaders(requestHeaders)
        request.setValue("bytes=\(rangeStart)-\(rangeEndInclusive)", forHTTPHeaderField: "Range")

        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }
        let response = stream.response
        guard let http = response as? HTTPURLResponse else {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "remote resource did not return an HTTP response for \(sourceDescription)"
            )
        }
        let statusCode = http.statusCode
        let chunkCoversWholeResource = rangeStart == 0 && rangeEndInclusive + 1 >= expectedSizeBytes

        switch statusCode {
        case 206:
            do {
                try validateHTTPPartialContentRange(
                    contentRangeHeader: http.value(forHTTPHeaderField: "Content-Range"),
                    contentLengthHeader: http.value(forHTTPHeaderField: "Content-Length"),
                    requestedStart: rangeStart,
                    requestedEndInclusive: rangeEndInclusive,
                    expectedBodyLength: rangeEndInclusive - rangeStart + 1,
                    expectedTotalSizeBytes: expectedSizeBytes,
                    sourceURL: sourceURL
                )
            } catch {
                throw staleDownloadResource(
                    error.localizedDescription,
                    uri: sourceURL.absoluteString,
                    phase: .download,
                    receivedBytes: rangeStart
                )
            }
        case 200:
            if !chunkCoversWholeResource {
                if rangeStart > 0, allowRestartAfterRangeMismatch {
                    try? fileManager.removeItem(at: destinationURL)
                    await onProgress(0)
                    return try await fetchKnownSizeHTTPResource(
                        sourceURL,
                        requestHeaders: requestHeaders,
                        expectedSizeBytes: expectedSizeBytes,
                        resumeFromBytes: 0,
                        rangeChunkBytes: rangeChunkBytes,
                        to: destinationURL,
                        allowRestartAfterRangeMismatch: false,
                        onProgress: onProgress
                    )
                }
                throw staleDownloadResource(
                    "remote server did not honor the requested byte range for \(sourceDescription)"
                )
            }
            if let contentLength = parseHttpContentLength(http.value(forHTTPHeaderField: "Content-Length")),
               contentLength != expectedSizeBytes {
                throw staleDownloadResource(
                    "remote server reported Content-Length \(contentLength), expected \(expectedSizeBytes) for \(sourceDescription)"
                )
            }
        case 416:
            if rangeStart > 0, allowRestartAfterRangeMismatch {
                try? fileManager.removeItem(at: destinationURL)
                await onProgress(0)
                return try await fetchKnownSizeHTTPResource(
                    sourceURL,
                        requestHeaders: requestHeaders,
                        expectedSizeBytes: expectedSizeBytes,
                        resumeFromBytes: 0,
                        rangeChunkBytes: rangeChunkBytes,
                        to: destinationURL,
                        allowRestartAfterRangeMismatch: false,
                        onProgress: onProgress
                )
            }
            throw staleDownloadResource(
                "remote resource rejected the requested byte range for \(sourceDescription)"
            )
        case 401, 403, 404, 410:
            throw expiredDownloadResource(
                sourceURL: sourceURL,
                statusCode: statusCode,
                phase: .download,
                receivedBytes: rangeStart
            )
        case 200..<300:
            break
        default:
            throw staleDownloadResource(
                "remote resource returned HTTP \(statusCode) for \(sourceDescription)"
            )
        }

        if !fileManager.fileExists(atPath: destinationURL.path) {
            fileManager.createFile(atPath: destinationURL.path, contents: nil)
        }
        let append = statusCode == 206 && rangeStart > 0
        if append {
            let existingBytes = UInt64(
                (try? destinationURL.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            )
            if existingBytes != rangeStart {
                try? fileManager.removeItem(at: destinationURL)
                await onProgress(0)
                return try await fetchKnownSizeHTTPResource(
                    sourceURL,
                    requestHeaders: requestHeaders,
                    expectedSizeBytes: expectedSizeBytes,
                    resumeFromBytes: 0,
                    rangeChunkBytes: rangeChunkBytes,
                    to: destinationURL,
                    allowRestartAfterRangeMismatch: false,
                    onProgress: onProgress
                )
            }
        }
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { closeDownloadFileHandle(output, context: "known-size resource output") }
        if append {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = append ? rangeStart : 0
        var lastCleanFileSize = totalWritten
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
                    try validateHTTPRangeProgress(
                        totalWritten: totalWritten,
                        expectedSizeBytes: expectedSizeBytes,
                        rangeEndInclusive: rangeEndInclusive,
                        isPartialResponse: statusCode == 206,
                        sourceURL: sourceURL,
                        destinationURL: destinationURL
                    )
                    buffer.removeAll(keepingCapacity: true)
                    await onProgress(totalWritten)
                }
            }
            if !buffer.isEmpty {
                try output.write(contentsOf: buffer)
                totalWritten += UInt64(buffer.count)
                lastCleanFileSize = totalWritten
                try validateHTTPRangeProgress(
                    totalWritten: totalWritten,
                    expectedSizeBytes: expectedSizeBytes,
                    rangeEndInclusive: rangeEndInclusive,
                    isPartialResponse: statusCode == 206,
                    sourceURL: sourceURL,
                    destinationURL: destinationURL
                )
                buffer.removeAll(keepingCapacity: true)
                await onProgress(totalWritten)
            }
        } catch {
            try? truncateFile(at: destinationURL, to: lastCleanFileSize)
            throw error
        }

        if statusCode == 206 {
            let expectedNextOffset = rangeEndInclusive + 1
            guard totalWritten == expectedNextOffset else {
                throw staleDownloadResource(
                    "downloaded range ended at \(totalWritten) for \(sourceDescription), expected \(expectedNextOffset)"
                )
            }
            return totalWritten
        }
        guard totalWritten == expectedSizeBytes else {
            throw staleDownloadResource(
                "downloaded \(totalWritten) bytes for \(sourceDescription), expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
    }

    func validateHTTPRangeProgress(
        totalWritten: UInt64,
        expectedSizeBytes: UInt64,
        rangeEndInclusive: UInt64,
        isPartialResponse: Bool,
        sourceURL: URL,
        destinationURL: URL
    ) throws {
        let sourceDescription = downloadURLDescriptionForDiagnostics(sourceURL)
        if totalWritten > expectedSizeBytes {
            try? fileManager.removeItem(at: destinationURL)
            throw staleDownloadResource(
                "remote server sent more bytes than expected for \(sourceDescription)"
            )
        }
        if isPartialResponse, totalWritten > rangeEndInclusive + 1 {
            try? fileManager.removeItem(at: destinationURL)
            throw staleDownloadResource(
                "remote server sent more bytes than the requested byte range for \(sourceDescription)"
            )
        }
    }
}
