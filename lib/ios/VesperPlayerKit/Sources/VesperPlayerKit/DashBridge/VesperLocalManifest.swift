import Foundation

/// Reads an explicit local manifest off the caller's actor within its remaining budget.
func vesperReadLocalManifest(url: URL, maximumBytes: Int) async throws -> Data {
    guard url.isFileURL, url.user == nil, url.query == nil, url.fragment == nil,
          url.host == nil || url.host == "" || url.host?.lowercased() == "localhost",
          maximumBytes > 0, maximumBytes <= 1024 * 1024 else {
        throw VesperDashStartupError.invalidResponse
    }
    let worker = Task.detached(priority: .utility) {
        try Task.checkCancellation()
        let attributes = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard attributes.isRegularFile == true else { throw VesperDashStartupError.invalidResponse }
        guard let size = attributes.fileSize, size <= maximumBytes else {
            throw VesperDashStartupError.budgetExceeded
        }
        let input = try FileHandle(forReadingFrom: url)
        defer { try? input.close() }
        var data = Data()
        while true {
            try Task.checkCancellation()
            let chunk = try input.read(upToCount: min(16 * 1024, maximumBytes - data.count + 1)) ?? Data()
            if chunk.isEmpty { break }
            guard data.count + chunk.count <= maximumBytes else { throw VesperDashStartupError.budgetExceeded }
            data.append(chunk)
        }
        try Task.checkCancellation()
        guard !data.isEmpty else { throw VesperDashStartupError.invalidResponse }
        return data
    }
    return try await withTaskCancellationHandler {
        let data = try await worker.value
        try Task.checkCancellation()
        return data
    } onCancel: {
        worker.cancel()
    }
}
