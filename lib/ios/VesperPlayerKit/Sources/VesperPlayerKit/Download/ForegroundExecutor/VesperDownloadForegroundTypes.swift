import Foundation

struct ForegroundDownloadEntry {
    let url: URL
    let resourceId: String?
    let segmentId: String?
    let relativePath: String?
    let byteRange: VesperDownloadByteRange?
    let generatedText: String?
    let expectedSizeBytes: UInt64?
    let fallbackName: String
    let isSegment: Bool
}

enum VesperForegroundDownloadPreparationError: LocalizedError {
    case invalidSource(String)
    case unsupported(String)

    var errorDescription: String? {
        switch self {
        case let .invalidSource(message), let .unsupported(message):
            return message
        }
    }
}

struct VesperStaleDownloadResourceError: LocalizedError {
    let message: String
    let resourceId: String?
    let segmentId: String?
    let uri: String?
    let phase: VesperDownloadStaleResourcePhase?
    let statusCode: Int?
    let receivedBytes: UInt64

    var errorDescription: String? {
        message
    }

    func staleResource(
        taskId: VesperDownloadTaskId,
        fallbackResourceId: String? = nil,
        fallbackSegmentId: String? = nil,
        fallbackUri: String? = nil,
        phase fallbackPhase: VesperDownloadStaleResourcePhase,
        receivedBytes fallbackReceivedBytes: UInt64 = 0
    ) -> VesperDownloadStaleResource {
        VesperDownloadStaleResource(
            taskId: taskId,
            resourceId: resourceId ?? fallbackResourceId,
            segmentId: segmentId ?? fallbackSegmentId,
            uri: uri ?? fallbackUri,
            phase: phase ?? fallbackPhase,
            statusCode: statusCode,
            receivedBytes: receivedBytes > 0 ? receivedBytes : fallbackReceivedBytes,
            message: message
        )
    }
}

func staleDownloadResource(
    _ message: String,
    resourceId: String? = nil,
    segmentId: String? = nil,
    uri: String? = nil,
    phase: VesperDownloadStaleResourcePhase? = nil,
    statusCode: Int? = nil,
    receivedBytes: UInt64 = 0
) -> VesperStaleDownloadResourceError {
    VesperStaleDownloadResourceError(
        message: message,
        resourceId: resourceId,
        segmentId: segmentId,
        uri: uri,
        phase: phase,
        statusCode: statusCode,
        receivedBytes: receivedBytes
    )
}

struct DownloadProgressThrottle {
    private let minProgressBytes: UInt64
    private let minProgressIntervalNs: UInt64
    private var lastReportedBytes: UInt64 = 0
    private var lastReportedTimeNs: UInt64 = 0

    init(minProgressBytes: UInt64, minProgressIntervalMs: UInt64) {
        self.minProgressBytes = max(minProgressBytes, 1)
        self.minProgressIntervalNs = minProgressIntervalMs * 1_000_000
    }

    mutating func shouldReport(receivedBytes: UInt64, force: Bool) -> Bool {
        if force || receivedBytes < lastReportedBytes {
            markReported(receivedBytes: receivedBytes)
            return true
        }
        if receivedBytes - lastReportedBytes < minProgressBytes {
            return false
        }
        let now = DispatchTime.now().uptimeNanoseconds
        if lastReportedTimeNs != 0, now - lastReportedTimeNs < minProgressIntervalNs {
            return false
        }
        markReported(receivedBytes: receivedBytes, now: now)
        return true
    }

    mutating func markReported(receivedBytes: UInt64) {
        markReported(receivedBytes: receivedBytes, now: DispatchTime.now().uptimeNanoseconds)
    }

    private mutating func markReported(receivedBytes: UInt64, now: UInt64) {
        lastReportedBytes = receivedBytes
        lastReportedTimeNs = now
    }
}
