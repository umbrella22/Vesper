import Foundation

internal struct VesperSequenceWarmupHTTPResponse: Sendable {
    let statusCode: Int
    let data: Data
}

internal enum VesperSequenceWarmupLoadingError: Error {
    case nonHTTPResponse
}

internal protocol VesperSequenceWarmupLoading: Sendable {
    func load(
        request: URLRequest,
        maximumBytes: Int
    ) async throws -> VesperSequenceWarmupHTTPResponse
}

internal struct VesperSequenceURLSessionWarmupLoader: VesperSequenceWarmupLoading {
    let session: URLSession

    init(session: URLSession = .shared) { self.session = session }

    func load(
        request: URLRequest,
        maximumBytes: Int
    ) async throws -> VesperSequenceWarmupHTTPResponse {
        try Task.checkCancellation()
        let (bytes, response) = try await session.bytes(for: request)
        let dataTask = bytes.task
        defer { dataTask.cancel() }
        try Task.checkCancellation()
        guard let http = response as? HTTPURLResponse else {
            throw VesperSequenceWarmupLoadingError.nonHTTPResponse
        }
        guard (200..<300).contains(http.statusCode), maximumBytes > 0 else {
            return VesperSequenceWarmupHTTPResponse(statusCode: http.statusCode, data: Data())
        }
        return try await withTaskCancellationHandler {
            var bounded = Data()
            for try await byte in bytes {
                try Task.checkCancellation()
                bounded.append(byte)
                if bounded.count >= maximumBytes { break }
            }
            try Task.checkCancellation()
            return VesperSequenceWarmupHTTPResponse(statusCode: http.statusCode, data: bounded)
        } onCancel: {
            dataTask.cancel()
        }
    }
}
