import Foundation

extension VesperForegroundDownloadExecutor {
    func fetchText(
        _ sourceUri: String,
        requestHeaders: [String: String]
    ) async throws -> String {
        let sourceURL = try resolveURL(sourceUri)
        let sourceDescription = downloadURLDescriptionForDiagnostics(sourceURL)
        let data: Data
        if sourceURL.isFileURL {
            data = try Data(contentsOf: sourceURL)
        } else {
            var request = URLRequest(url: sourceURL)
            request.applyDownloadHttpHeaders(requestHeaders)
            let (responseData, response) = try await httpData(for: request, sourceURL: sourceURL)
            if let http = response as? HTTPURLResponse {
                if isExpiredHttpStatus(http.statusCode) {
                    throw expiredDownloadResource(
                        sourceURL: sourceURL,
                        statusCode: http.statusCode,
                        phase: .prepare
                    )
                }
                if !(200..<300).contains(http.statusCode) {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote resource returned HTTP \(http.statusCode) for \(sourceDescription)"
                    )
                }
            }
            data = responseData
        }
        guard let text = String(data: data, encoding: .utf8) else {
            throw VesperForegroundDownloadPreparationError.invalidSource("remote manifest was not valid UTF-8")
        }
        return text
    }

    func probeRequiredSize(
        _ sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: [String: String]
    ) async throws -> UInt64 {
        if let byteRange {
            return byteRange.length
        }
        return try await probeContentLength(try resolveURL(sourceUri), requestHeaders: requestHeaders)
    }

    func probeContentLength(
        _ sourceURL: URL,
        requestHeaders: [String: String]
    ) async throws -> UInt64 {
        if sourceURL.isFileURL {
            let values = try sourceURL.resourceValues(forKeys: [.fileSizeKey])
            guard let size = values.fileSize, size > 0 else {
                throw CocoaError(.fileReadUnknown)
            }
            return UInt64(size)
        }

        var request = URLRequest(url: sourceURL)
        request.applyDownloadHttpHeaders(requestHeaders)
        request.httpMethod = "HEAD"
        let response = try await httpResponse(for: request, sourceURL: sourceURL)
        if let http = response as? HTTPURLResponse,
           isExpiredHttpStatus(http.statusCode) {
            throw expiredDownloadResource(
                sourceURL: sourceURL,
                statusCode: http.statusCode,
                phase: .prepare
            )
        }
        if let http = response as? HTTPURLResponse, http.statusCode == 200,
           let value = http.value(forHTTPHeaderField: "Content-Length"),
           let size = UInt64(value), size > 0
        {
            return size
        }

        var rangeRequest = URLRequest(url: sourceURL)
        rangeRequest.applyDownloadHttpHeaders(requestHeaders)
        rangeRequest.setValue("bytes=0-0", forHTTPHeaderField: "Range")
        rangeRequest.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        let rangeResponse = try await httpResponse(for: rangeRequest, sourceURL: sourceURL)
        if let http = rangeResponse as? HTTPURLResponse,
           isExpiredHttpStatus(http.statusCode) {
            throw expiredDownloadResource(
                sourceURL: sourceURL,
                statusCode: http.statusCode,
                phase: .prepare
            )
        }
        if let http = rangeResponse as? HTTPURLResponse {
            if http.statusCode == 206 {
                guard http.value(forHTTPHeaderField: "Content-Encoding").map({ $0.lowercased() == "identity" }) ?? true else {
                    throw VesperForegroundDownloadPreparationError.invalidSource("range probe returned encoded content")
                }
                let contentRange = try validateHTTPPartialContentRange(
                    contentRangeHeader: http.value(forHTTPHeaderField: "Content-Range"),
                    contentLengthHeader: http.value(forHTTPHeaderField: "Content-Length"),
                    requestedStart: 0, requestedEndInclusive: 0, expectedBodyLength: 1,
                    expectedTotalSizeBytes: nil, sourceURL: sourceURL
                )
                if let size = contentRange.total, size > 0 { return size }
            } else if http.statusCode == 200,
                      let size = parseHttpContentLength(http.value(forHTTPHeaderField: "Content-Length")), size > 0 {
                return size
            }
        }

        throw CocoaError(.fileReadUnknown)
    }

    func httpResponse(for request: URLRequest, sourceURL: URL) async throws -> URLResponse {
        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }
        try Task.checkCancellation()
        return stream.response
    }

    func inferredFileName(_ uri: String) -> String {
        let name = URL(string: uri)?.lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return name.isEmpty ? "media.bin" : name
    }
}
