import Foundation

extension VesperForegroundDownloadExecutor {
    func httpBodyStream(for request: URLRequest, sourceURL: URL) async throws -> VesperHTTPBodyStream {
        try rejectInsecureHTTPURL(sourceURL)

        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = true
        let timeoutSeconds = max(TimeInterval(stalledTransferTimeoutMs) / 1_000, 1)
        configuration.timeoutIntervalForRequest = timeoutSeconds
        configuration.timeoutIntervalForResource = max(timeoutSeconds * 4, 60)

        let delegate = VesperURLSessionDataStreamDelegate(
            stalledTransferTimeoutMs: stalledTransferTimeoutMs,
            sourceDescription: sourceURL.absoluteString
        )
        let delegateQueue = OperationQueue()
        delegateQueue.maxConcurrentOperationCount = 1
        let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: delegateQueue)
        let task = session.dataTask(with: request)
        delegate.bind(session: session, task: task)
        task.resume()
        let response = try await delegate.waitForResponse()
        return VesperHTTPBodyStream(
            response: response,
            chunks: delegate.chunks,
            cancel: { delegate.cancel() }
        )
    }

    func httpData(for request: URLRequest, sourceURL: URL) async throws -> (Data, URLResponse) {
        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }

        var data = Data()
        for try await chunk in stream.chunks {
            try Task.checkCancellation()
            data.append(chunk)
        }
        return (data, stream.response)
    }
}
