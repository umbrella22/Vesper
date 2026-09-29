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
    func load(
        request: URLRequest,
        maximumBytes: Int
    ) async throws -> VesperSequenceWarmupHTTPResponse {
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw VesperSequenceWarmupLoadingError.nonHTTPResponse
        }
        guard (200..<300).contains(http.statusCode), maximumBytes > 0 else {
            return VesperSequenceWarmupHTTPResponse(statusCode: http.statusCode, data: Data())
        }
        var bounded = Data()
        bounded.reserveCapacity(maximumBytes)
        for try await byte in bytes {
            try Task.checkCancellation()
            bounded.append(byte)
            if bounded.count >= maximumBytes { break }
        }
        return VesperSequenceWarmupHTTPResponse(statusCode: http.statusCode, data: bounded)
    }
}
