import Combine
import Foundation
internal import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif
public struct VesperDownloadByteRange: Equatable, Codable {
    public let offset: UInt64
    public let length: UInt64

    public init(offset: UInt64, length: UInt64) {
        self.offset = offset
        self.length = length
    }
}

public struct VesperDownloadResourceRecord: Equatable, Codable {
    public let resourceId: String
    public let uri: String
    public let relativePath: String?
    public let byteRange: VesperDownloadByteRange?
    public let generatedText: String?
    public let sizeBytes: UInt64?
    public let etag: String?
    public let checksum: String?

    public init(
        resourceId: String,
        uri: String,
        relativePath: String? = nil,
        byteRange: VesperDownloadByteRange? = nil,
        generatedText: String? = nil,
        sizeBytes: UInt64? = nil,
        etag: String? = nil,
        checksum: String? = nil
    ) {
        self.resourceId = resourceId
        self.uri = uri
        self.relativePath = relativePath
        self.byteRange = byteRange
        self.generatedText = generatedText
        self.sizeBytes = sizeBytes
        self.etag = etag
        self.checksum = checksum
    }
}

public struct VesperDownloadSegmentRecord: Equatable, Codable {
    public let segmentId: String
    public let uri: String
    public let relativePath: String?
    public let sequence: UInt64?
    public let byteRange: VesperDownloadByteRange?
    public let sizeBytes: UInt64?
    public let checksum: String?

    public init(
        segmentId: String,
        uri: String,
        relativePath: String? = nil,
        sequence: UInt64? = nil,
        byteRange: VesperDownloadByteRange? = nil,
        sizeBytes: UInt64? = nil,
        checksum: String? = nil
    ) {
        self.segmentId = segmentId
        self.uri = uri
        self.relativePath = relativePath
        self.sequence = sequence
        self.byteRange = byteRange
        self.sizeBytes = sizeBytes
        self.checksum = checksum
    }
}

public enum VesperDownloadStreamKind: String, Equatable, Codable {
    case combined
    case video
    case audio
    case secondaryAudio
    case subtitle
    case auxiliary
}

public struct VesperDownloadAssetStream: Equatable, Codable {
    public let streamId: String
    public let kind: VesperDownloadStreamKind
    public let language: String?
    public let codec: String?
    public let label: String?
    public let qualityRank: UInt32?
    public let resourceIds: [String]
    public let segmentIds: [String]
    public let metadata: [String: String]

    public init(
        streamId: String,
        kind: VesperDownloadStreamKind = .combined,
        language: String? = nil,
        codec: String? = nil,
        label: String? = nil,
        qualityRank: UInt32? = nil,
        resourceIds: [String] = [],
        segmentIds: [String] = [],
        metadata: [String: String] = [:]
    ) {
        self.streamId = streamId
        self.kind = kind
        self.language = language
        self.codec = codec
        self.label = label
        self.qualityRank = qualityRank
        self.resourceIds = resourceIds
        self.segmentIds = segmentIds
        self.metadata = metadata
    }
}

public struct VesperDownloadAssetIndex: Equatable, Codable {
    public let contentFormat: VesperDownloadContentFormat
    public let version: String?
    public let etag: String?
    public let checksum: String?
    public let totalSizeBytes: UInt64?
    public let resources: [VesperDownloadResourceRecord]
    public let segments: [VesperDownloadSegmentRecord]
    public let streams: [VesperDownloadAssetStream]
    public let completedPath: String?

    public init(
        contentFormat: VesperDownloadContentFormat = .unknown,
        version: String? = nil,
        etag: String? = nil,
        checksum: String? = nil,
        totalSizeBytes: UInt64? = nil,
        resources: [VesperDownloadResourceRecord] = [],
        segments: [VesperDownloadSegmentRecord] = [],
        streams: [VesperDownloadAssetStream] = [],
        completedPath: String? = nil
    ) {
        self.contentFormat = contentFormat
        self.version = version
        self.etag = etag
        self.checksum = checksum
        self.totalSizeBytes = totalSizeBytes
        self.resources = resources
        self.segments = segments
        self.streams = streams
        self.completedPath = completedPath
    }
}
