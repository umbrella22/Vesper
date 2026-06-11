import Foundation

extension VesperForegroundDownloadExecutor {
    func fetchText(
        _ sourceUri: String,
        requestHeaders: [String: String]
    ) async throws -> String {
        let sourceURL = try resolveURL(sourceUri)
        let data: Data
        if sourceURL.isFileURL {
            data = try Data(contentsOf: sourceURL)
        } else {
            var request = URLRequest(url: sourceURL)
            request.applyDownloadHttpHeaders(requestHeaders)
            let (responseData, response) = try await httpData(for: request, sourceURL: sourceURL)
            if let http = response as? HTTPURLResponse {
                if isExpiredHttpStatus(http.statusCode) {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
                    )
                }
                if !(200..<300).contains(http.statusCode) {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote resource returned HTTP \(http.statusCode) for \(sourceURL.absoluteString)"
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
        let (_, response) = try await httpData(for: request, sourceURL: sourceURL)
        if let http = response as? HTTPURLResponse,
           isExpiredHttpStatus(http.statusCode) {
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
            )
        }
        if let http = response as? HTTPURLResponse,
           let value = http.value(forHTTPHeaderField: "Content-Length"),
           let size = UInt64(value), size > 0
        {
            return size
        }

        var rangeRequest = URLRequest(url: sourceURL)
        rangeRequest.applyDownloadHttpHeaders(requestHeaders)
        rangeRequest.setValue("bytes=0-0", forHTTPHeaderField: "Range")
        let (_, rangeResponse) = try await httpData(for: rangeRequest, sourceURL: sourceURL)
        if let http = rangeResponse as? HTTPURLResponse,
           isExpiredHttpStatus(http.statusCode) {
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
            )
        }
        if let http = rangeResponse as? HTTPURLResponse,
           let contentRange = parseHttpContentRange(http.value(forHTTPHeaderField: "Content-Range")),
           let size = contentRange.total,
           size > 0
        {
            return size
        }

        throw CocoaError(.fileReadUnknown)
    }

    func inferredFileName(_ uri: String) -> String {
        let name = URL(string: uri)?.lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return name.isEmpty ? "media.bin" : name
    }
}
