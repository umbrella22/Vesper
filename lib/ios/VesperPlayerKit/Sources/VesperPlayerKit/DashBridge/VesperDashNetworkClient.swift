@preconcurrency import AVFoundation
import Foundation
internal import VesperPlayerKitBridgeShim

class VesperDashNetworkClient {
    private let headers: [String: String]

    init(headers: [String: String] = [:]) {
        self.headers = headers
    }

    func data(for url: URL, byteRange: VesperDashByteRange? = nil) async throws -> Data {
        if url.isFileURL {
            return try readLocalFile(url: url, byteRange: byteRange)
        }
        try rejectInsecureHTTPURL(url)

        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        if let byteRange {
            request.setValue("bytes=\(byteRange.start)-\(byteRange.end)", forHTTPHeaderField: "Range")
        }
        let session = makeSession()
        defer { session.invalidateAndCancel() }
        let (data, response) = try await session.data(for: request)
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            throw VesperDashBridgeError.network("HTTP \(httpResponse.statusCode) for \(url.absoluteString)")
        }
        return data
    }

    func download(
        for url: URL,
        byteRange: VesperDashByteRange? = nil,
        to destinationURL: URL
    ) async throws -> UInt64 {
        if !url.isFileURL {
            try rejectInsecureHTTPURL(url)
        }
        try FileManager.default.createDirectory(
            at: destinationURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        removeFileIfPresent(destinationURL, context: "existing DASH download destination")

        if url.isFileURL {
            return try copyLocalFile(url: url, byteRange: byteRange, to: destinationURL)
        }

        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        if let byteRange {
            request.setValue("bytes=\(byteRange.start)-\(byteRange.end)", forHTTPHeaderField: "Range")
        }
        let session = makeSession()
        defer { session.invalidateAndCancel() }
        let (temporaryURL, response) = try await session.download(for: request)
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            removeFileIfPresent(temporaryURL, context: "failed DASH download temporary file")
            throw VesperDashBridgeError.network("HTTP \(httpResponse.statusCode) for \(url.absoluteString)")
        }
        try FileManager.default.moveItem(at: temporaryURL, to: destinationURL)
        return fileSize(at: destinationURL) ?? 0
    }

    private func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = true
        configuration.timeoutIntervalForRequest = vesperDashNetworkStallTimeoutSeconds
        configuration.timeoutIntervalForResource = vesperDashNetworkResourceTimeoutSeconds
        return URLSession(configuration: configuration)
    }

    private func rejectInsecureHTTPURL(_ url: URL) throws {
        guard url.scheme?.lowercased() == "http" else {
            return
        }
        throw VesperDashBridgeError.network("\(vesperDashATSFailureMessage) URL: \(url.absoluteString)")
    }

    private func readLocalFile(url: URL, byteRange: VesperDashByteRange?) throws -> Data {
        guard let byteRange else {
            return try Data(contentsOf: url)
        }

        let length = try checkedInt(byteRange.length, field: "local file byte range length")
        let handle = try FileHandle(forReadingFrom: url)
        defer { closeFileHandle(handle, context: "local byte range") }
        try handle.seek(toOffset: byteRange.start)
        let data = try handle.read(upToCount: length) ?? Data()
        guard data.count == length else {
            throw VesperDashBridgeError.network("local file byte range is shorter than requested")
        }
        return data
    }

    private func copyLocalFile(
        url: URL,
        byteRange: VesperDashByteRange?,
        to destinationURL: URL
    ) throws -> UInt64 {
        guard let byteRange else {
            try FileManager.default.copyItem(at: url, to: destinationURL)
            return fileSize(at: destinationURL) ?? 0
        }

        let input = try FileHandle(forReadingFrom: url)
        defer { closeFileHandle(input, context: "local copy input") }
        FileManager.default.createFile(atPath: destinationURL.path, contents: nil)
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { closeFileHandle(output, context: "local copy output") }

        try input.seek(toOffset: byteRange.start)
        var remaining = byteRange.length
        while remaining > 0 {
            let readCount = remaining > 256 * 1024 ? 256 * 1024 : Int(remaining)
            let data = try input.read(upToCount: readCount) ?? Data()
            guard !data.isEmpty else {
                throw VesperDashBridgeError.network("local file byte range is shorter than requested")
            }
            try output.write(contentsOf: data)
            remaining = remaining.dashSaturatingSubtract(UInt64(data.count))
        }
        return byteRange.length
    }

    private func fileSize(at url: URL) -> UInt64? {
        guard
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let value = attributes[.size] as? NSNumber
        else {
            return nil
        }
        return value.uint64Value
    }
}
