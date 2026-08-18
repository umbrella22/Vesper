@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

let vesperDashATSFailureMessage =
    "iOS DASH playback requires HTTPS media URLs. The SDK does not relax App Transport Security for http:// resources; host apps that need insecure HTTP must fetch those resources outside the SDK and provide local file URLs."
let vesperDashNetworkStallTimeoutSeconds: TimeInterval = 30
let vesperDashNetworkResourceTimeoutSeconds: TimeInterval = 60

struct VesperDashSegmentCacheKey: Hashable {
    let renditionId: String
    let segment: VesperDashSegmentRequest
}

struct VesperDashCachedSegmentFile {
    let url: URL
    let size: UInt64
    var lastAccessedAt: Date

    var isInitialization: Bool {
        segment == .initialization
    }

    private let segment: VesperDashSegmentRequest

    init(url: URL, size: UInt64, segment: VesperDashSegmentRequest, lastAccessedAt: Date) {
        self.url = url
        self.size = size
        self.segment = segment
        self.lastAccessedAt = lastAccessedAt
    }
}

enum VesperDashResourceResponse {
    case resource(VesperLocalResourceBody)
    case redirect(URL)
}

enum VesperDashSegmentPayload {
    case data(Data, contentType: String)
    case file(url: URL, offset: UInt64, size: UInt64, removeAfterServing: Bool, contentType: String)

    var size: UInt64 {
        switch self {
        case let .data(data, _):
            return UInt64(data.count)
        case let .file(_, _, size, _, _):
            return size
        }
    }

    var contentType: String {
        switch self {
        case let .data(_, contentType):
            return contentType
        case let .file(_, _, _, _, contentType):
            return contentType
        }
    }

    var isTemporaryFile: Bool {
        if case .file(_, _, _, true, _) = self {
            return true
        }
        return false
    }

    var localResourceBody: VesperLocalResourceBody {
        switch self {
        case let .data(data, contentType):
            .data(data, contentType: avResourceContentType(forSegmentContentType: contentType))
        case let .file(url, offset, size, removeAfterServing, contentType):
            .file(
                url: url,
                offset: offset,
                length: size,
                contentType: avResourceContentType(forSegmentContentType: contentType),
                removeAfterServing: removeAfterServing,
                growingPolicy: nil
            )
        }
    }

    func readData() throws -> Data {
        switch self {
        case let .data(data, _):
            return data
        case let .file(url, offset, size, removeAfterServing, _):
            defer {
                if removeAfterServing {
                    try? FileManager.default.removeItem(at: url)
                }
            }
            let length = try checkedInt(size, field: "segment payload length")
            let handle = try FileHandle(forReadingFrom: url)
            defer { closeFileHandle(handle, context: "segment payload") }
            try handle.seek(toOffset: offset)
            let data = try handle.read(upToCount: length) ?? Data()
            guard data.count == length else {
                throw VesperDashBridgeError.network("segment file is shorter than requested")
            }
            return data
        }
    }

    func cleanupIfTemporary() {
        if case let .file(url, _, _, true, _) = self {
            try? FileManager.default.removeItem(at: url)
        }
    }
}

func avResourceContentType(forSegmentContentType contentType: String) -> String {
    let normalized = contentType.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    if normalized.contains("vtt") {
        return "public.webvtt"
    }
    if normalized.hasPrefix("public.") {
        return contentType
    }
    return "public.mpeg-4"
}

struct VesperDashSegmentPayloadResult {
    let payload: VesperDashSegmentPayload
    let cacheHit: Bool
    let segmentType: String
    let byteRange: VesperDashByteRange?
    let delivery: String
    let coalesced: Bool

    init(
        payload: VesperDashSegmentPayload,
        cacheHit: Bool,
        segmentType: String,
        byteRange: VesperDashByteRange?,
        delivery: String,
        coalesced: Bool = false
    ) {
        self.payload = payload
        self.cacheHit = cacheHit
        self.segmentType = segmentType
        self.byteRange = byteRange
        self.delivery = delivery
        self.coalesced = coalesced
    }

    func markingCoalesced(
        payload: VesperDashSegmentPayload? = nil,
        cacheHit: Bool? = nil,
        delivery: String? = nil
    ) -> VesperDashSegmentPayloadResult {
        VesperDashSegmentPayloadResult(
            payload: payload ?? self.payload,
            cacheHit: cacheHit ?? self.cacheHit,
            segmentType: segmentType,
            byteRange: byteRange,
            delivery: delivery ?? self.delivery,
            coalesced: true
        )
    }
}
