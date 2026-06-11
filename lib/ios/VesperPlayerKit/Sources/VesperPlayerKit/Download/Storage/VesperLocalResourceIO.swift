@preconcurrency import AVFoundation
import Foundation

let vesperLocalResourceMinReadBufferBytes = 16 * 1024
let vesperLocalResourceMaxReadBufferBytes = 4 * 1024 * 1024
let vesperLocalResourceDefaultReadBufferBytes = 4 * 1024 * 1024

enum VesperLocalResourceOutOfRangeBehavior {
    case fail
    case finishEmpty
}

struct VesperLocalResourceReadPolicy {
    let bufferBytes: Int
    let outOfRangeBehavior: VesperLocalResourceOutOfRangeBehavior

    init(
        bufferBytes: Int = vesperLocalResourceDefaultReadBufferBytes,
        outOfRangeBehavior: VesperLocalResourceOutOfRangeBehavior = .fail
    ) {
        self.bufferBytes = max(
            vesperLocalResourceMinReadBufferBytes,
            min(bufferBytes, vesperLocalResourceMaxReadBufferBytes)
        )
        self.outOfRangeBehavior = outOfRangeBehavior
    }
}

struct VesperGrowingFileReadPolicy {
    let timeoutSeconds: TimeInterval
    let pollSeconds: TimeInterval

    init(timeoutSeconds: TimeInterval, pollSeconds: TimeInterval) {
        self.timeoutSeconds = max(0, timeoutSeconds)
        self.pollSeconds = max(0.001, pollSeconds)
    }
}

enum VesperLocalResourceBody {
    case data(Data, contentType: String)
    case file(
        url: URL,
        offset: UInt64,
        length: UInt64,
        contentType: String,
        removeAfterServing: Bool,
        growingPolicy: VesperGrowingFileReadPolicy?
    )

    var contentType: String {
        switch self {
        case let .data(_, contentType):
            contentType
        case let .file(_, _, _, contentType, _, _):
            contentType
        }
    }

    var contentLength: UInt64 {
        switch self {
        case let .data(data, _):
            UInt64(data.count)
        case let .file(_, _, length, _, _, _):
            length
        }
    }

    func cleanupIfNeeded() {
        if case let .file(url, _, _, _, true, _) = self {
            try? FileManager.default.removeItem(at: url)
        }
    }
}

enum VesperLocalResourceIOError: Error, LocalizedError {
    case negativeOffset
    case offsetOutOfRange
    case contentLengthOverflow
    case fileShorterThanRequested

    var errorDescription: String? {
        switch self {
        case .negativeOffset:
            "Negative local resource byte offset requested."
        case .offsetOutOfRange:
            "Local resource byte offset exceeds response size."
        case .contentLengthOverflow:
            "Local resource response exceeds Int64.max."
        case .fileShorterThanRequested:
            "Local resource file is shorter than requested."
        }
    }
}

struct VesperLocalResourceResponder {
    static func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        body: VesperLocalResourceBody,
        readPolicy: VesperLocalResourceReadPolicy = VesperLocalResourceReadPolicy()
    ) {
        defer { body.cleanupIfNeeded() }
        do {
            loadingRequest.contentInformationRequest?.contentType = body.contentType
            loadingRequest.contentInformationRequest?.contentLength = try checkedContentLength(
                body.contentLength
            )
            loadingRequest.contentInformationRequest?.isByteRangeAccessSupported = true
            if let dataRequest = loadingRequest.dataRequest {
                try respond(to: dataRequest, body: body, readPolicy: readPolicy)
            }
            loadingRequest.finishLoading()
        } catch {
            loadingRequest.finishLoading(with: error)
        }
    }

    static func finish(_ loadingRequest: AVAssetResourceLoadingRequest, error: Error) {
        loadingRequest.finishLoading(with: error)
    }

    static func respond(
        to dataRequest: AVAssetResourceLoadingDataRequest,
        body: VesperLocalResourceBody,
        readPolicy: VesperLocalResourceReadPolicy = VesperLocalResourceReadPolicy()
    ) throws {
        switch body {
        case let .data(data, _):
            try respond(to: dataRequest, data: data, readPolicy: readPolicy)
        case let .file(url, offset, length, _, _, growingPolicy):
            try respond(
                to: dataRequest,
                fileURL: url,
                fileOffset: offset,
                resourceLength: length,
                growingPolicy: growingPolicy,
                readPolicy: readPolicy
            )
        }
    }

    static func readDataForTesting(
        body: VesperLocalResourceBody,
        requestedOffset: Int64 = 0,
        currentOffset: Int64 = 0,
        requestedLength: Int = 0,
        readPolicy: VesperLocalResourceReadPolicy = VesperLocalResourceReadPolicy()
    ) throws -> Data {
        switch body {
        case let .data(data, _):
            guard let range = try resolvedRange(
                requestedOffset: requestedOffset,
                currentOffset: currentOffset,
                requestedLength: requestedLength,
                resourceLength: UInt64(data.count),
                outOfRangeBehavior: readPolicy.outOfRangeBehavior
            ) else {
                return Data()
            }
            let start = try checkedInt(range.offset)
            let length = try checkedInt(range.length)
            return data.subdata(in: start..<(start + length))
        case let .file(url, offset, length, _, _, growingPolicy):
            let effectiveLength = effectiveResourceLength(
                url: url,
                fileOffset: offset,
                initialLength: length,
                requestedOffset: requestedOffset,
                currentOffset: currentOffset,
                requestedLength: requestedLength,
                growingPolicy: growingPolicy
            )
            guard let range = try resolvedRange(
                requestedOffset: requestedOffset,
                currentOffset: currentOffset,
                requestedLength: requestedLength,
                resourceLength: effectiveLength,
                outOfRangeBehavior: readPolicy.outOfRangeBehavior
            ) else {
                return Data()
            }
            return try readFileData(
                url: url,
                fileOffset: offset + range.offset,
                length: range.length,
                bufferBytes: readPolicy.bufferBytes
            )
        }
    }

    private static func respond(
        to dataRequest: AVAssetResourceLoadingDataRequest,
        data: Data,
        readPolicy: VesperLocalResourceReadPolicy
    ) throws {
        guard let range = try resolvedRange(
            requestedOffset: dataRequest.requestedOffset,
            currentOffset: dataRequest.currentOffset,
            requestedLength: dataRequest.requestedLength,
            resourceLength: UInt64(data.count),
            outOfRangeBehavior: readPolicy.outOfRangeBehavior
        ) else {
            return
        }
        let start = try checkedInt(range.offset)
        let length = try checkedInt(range.length)
        dataRequest.respond(with: data.subdata(in: start..<(start + length)))
    }

    private static func respond(
        to dataRequest: AVAssetResourceLoadingDataRequest,
        fileURL: URL,
        fileOffset: UInt64,
        resourceLength: UInt64,
        growingPolicy: VesperGrowingFileReadPolicy?,
        readPolicy: VesperLocalResourceReadPolicy
    ) throws {
        let effectiveLength = effectiveResourceLength(
            url: fileURL,
            fileOffset: fileOffset,
            initialLength: resourceLength,
            requestedOffset: dataRequest.requestedOffset,
            currentOffset: dataRequest.currentOffset,
            requestedLength: dataRequest.requestedLength,
            growingPolicy: growingPolicy
        )
        guard let range = try resolvedRange(
            requestedOffset: dataRequest.requestedOffset,
            currentOffset: dataRequest.currentOffset,
            requestedLength: dataRequest.requestedLength,
            resourceLength: effectiveLength,
            outOfRangeBehavior: readPolicy.outOfRangeBehavior
        ) else {
            return
        }
        try respond(
            to: dataRequest,
            fileURL: fileURL,
            fileOffset: fileOffset + range.offset,
            length: range.length,
            bufferBytes: readPolicy.bufferBytes
        )
    }

    private static func respond(
        to dataRequest: AVAssetResourceLoadingDataRequest,
        fileURL: URL,
        fileOffset: UInt64,
        length: UInt64,
        bufferBytes: Int
    ) throws {
        let handle = try FileHandle(forReadingFrom: fileURL)
        defer { try? handle.close() }
        try handle.seek(toOffset: fileOffset)
        var remaining = length
        while remaining > 0 {
            let count = min(try checkedInt(remaining), bufferBytes)
            let data = try handle.read(upToCount: count) ?? Data()
            guard !data.isEmpty else {
                throw VesperLocalResourceIOError.fileShorterThanRequested
            }
            dataRequest.respond(with: data)
            remaining = remaining.saturatingSubtract(UInt64(data.count))
        }
    }

    private static func readFileData(
        url: URL,
        fileOffset: UInt64,
        length: UInt64,
        bufferBytes: Int
    ) throws -> Data {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        try handle.seek(toOffset: fileOffset)
        var output = Data()
        var remaining = length
        while remaining > 0 {
            let count = min(try checkedInt(remaining), bufferBytes)
            let data = try handle.read(upToCount: count) ?? Data()
            guard !data.isEmpty else {
                throw VesperLocalResourceIOError.fileShorterThanRequested
            }
            output.append(data)
            remaining = remaining.saturatingSubtract(UInt64(data.count))
        }
        return output
    }

    private static func resolvedRange(
        requestedOffset: Int64,
        currentOffset: Int64,
        requestedLength: Int,
        resourceLength: UInt64,
        outOfRangeBehavior: VesperLocalResourceOutOfRangeBehavior
    ) throws -> (offset: UInt64, length: UInt64)? {
        let selectedOffset = currentOffset != 0 ? currentOffset : requestedOffset
        guard selectedOffset >= 0 else {
            throw VesperLocalResourceIOError.negativeOffset
        }
        let offset = UInt64(selectedOffset)
        if offset > resourceLength {
            if outOfRangeBehavior == .finishEmpty {
                return nil
            }
            throw VesperLocalResourceIOError.offsetOutOfRange
        }
        if offset == resourceLength {
            return nil
        }
        let remaining = resourceLength - offset
        let length = requestedLength > 0
            ? min(UInt64(requestedLength), remaining)
            : remaining
        guard length > 0 else {
            return nil
        }
        return (offset, length)
    }

    private static func effectiveResourceLength(
        url: URL,
        fileOffset: UInt64,
        initialLength: UInt64,
        requestedOffset: Int64,
        currentOffset: Int64,
        requestedLength: Int,
        growingPolicy: VesperGrowingFileReadPolicy?
    ) -> UInt64 {
        guard let growingPolicy else {
            return initialLength
        }
        let selectedOffset = max(0, currentOffset != 0 ? currentOffset : requestedOffset)
        let requestedResourceEnd = UInt64(selectedOffset).saturatingAdd(
            UInt64(max(1, requestedLength))
        )
        let targetFileLength = fileOffset.saturatingAdd(requestedResourceEnd)
        let fileLength = vesperLocalResourceWaitForFileLength(
            url,
            atLeast: targetFileLength,
            timeoutSeconds: growingPolicy.timeoutSeconds,
            pollSeconds: growingPolicy.pollSeconds
        )
        guard fileLength > fileOffset else {
            return initialLength
        }
        return max(initialLength, fileLength - fileOffset)
    }

    private static func checkedContentLength(_ value: UInt64) throws -> Int64 {
        guard value <= UInt64(Int64.max) else {
            throw VesperLocalResourceIOError.contentLengthOverflow
        }
        return Int64(value)
    }

    private static func checkedInt(_ value: UInt64) throws -> Int {
        guard value <= UInt64(Int.max) else {
            throw VesperLocalResourceIOError.contentLengthOverflow
        }
        return Int(value)
    }
}

func vesperLocalResourceIsContained(_ candidate: URL, in directory: URL) -> Bool {
    let candidatePath = candidate.standardizedFileURL.path
    let directoryPath = directory.standardizedFileURL.path
    return candidatePath.hasPrefix(directoryPath + "/")
}

func vesperLocalResourceFileSize(_ url: URL) throws -> UInt64 {
    let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
    if let size = attributes[.size] as? NSNumber {
        return size.uint64Value
    }
    return 0
}

func vesperLocalResourceWaitForFileLength(
    _ url: URL,
    atLeast targetLength: UInt64,
    timeoutSeconds: TimeInterval,
    pollSeconds: TimeInterval
) -> UInt64 {
    // This blocks with Thread.sleep while a growing media file catches up.
    // Call it only from AVAssetResourceLoader/background resource queues, never
    // from the main queue.
    let deadline = Date().addingTimeInterval(max(0, timeoutSeconds))
    var currentLength = (try? vesperLocalResourceFileSize(url)) ?? 0
    while currentLength < targetLength && Date() < deadline {
        Thread.sleep(forTimeInterval: max(0.001, pollSeconds))
        currentLength = (try? vesperLocalResourceFileSize(url)) ?? currentLength
    }
    return currentLength
}

private extension UInt64 {
    func saturatingAdd(_ other: UInt64) -> UInt64 {
        let (value, overflow) = addingReportingOverflow(other)
        return overflow ? UInt64.max : value
    }

    func saturatingSubtract(_ other: UInt64) -> UInt64 {
        other > self ? 0 : self - other
    }
}
