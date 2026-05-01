import Foundation

enum VesperDashBridgeError: LocalizedError {
    case invalidManifest(String)
    case unsupportedManifest(String)
    case invalidMp4(String)
    case unsupportedMp4(String)
    case network(String)

    var errorDescription: String? {
        switch self {
        case let .invalidManifest(message):
            "Invalid DASH manifest: \(message)"
        case let .unsupportedManifest(message):
            "Unsupported DASH manifest: \(message)"
        case let .invalidMp4(message):
            "Invalid MP4 index: \(message)"
        case let .unsupportedMp4(message):
            "Unsupported MP4 index: \(message)"
        case let .network(message):
            "DASH network request failed: \(message)"
        }
    }
}

struct VesperDashByteRange: Codable, Equatable {
    let start: UInt64
    let end: UInt64

    var length: UInt64 {
        end - start + 1
    }

    init(start: UInt64, end: UInt64) throws {
        guard end >= start else {
            throw VesperDashBridgeError.invalidManifest("byte range end is smaller than start")
        }
        self.start = start
        self.end = end
    }
}

struct VesperDashSegmentBase: Codable, Equatable {
    let initialization: VesperDashByteRange
    let indexRange: VesperDashByteRange
}

struct VesperDashSegmentTemplate: Codable, Equatable {
    let timescale: UInt64
    let duration: UInt64?
    let startNumber: UInt64
    let presentationTimeOffset: UInt64
    let initialization: String?
    let media: String
    let timeline: [VesperDashSegmentTimelineEntry]
}

struct VesperDashSegmentTimelineEntry: Codable, Equatable {
    let startTime: UInt64?
    let duration: UInt64
    let repeatCount: Int
}

enum VesperDashAdaptationKind: String, Codable, Equatable {
    case video
    case audio
    case subtitle
    case unknown
}

struct VesperDashRepresentation: Codable, Equatable {
    let id: String
    let baseURL: String
    let mimeType: String
    let codecs: String
    let bandwidth: UInt64?
    let width: Int?
    let height: Int?
    let frameRate: String?
    let audioSamplingRate: String?
    let segmentBase: VesperDashSegmentBase?
    let segmentTemplate: VesperDashSegmentTemplate?
}

struct VesperDashAdaptationSet: Codable, Equatable {
    let id: String?
    let kind: VesperDashAdaptationKind
    let mimeType: String?
    let language: String?
    let representations: [VesperDashRepresentation]
}

struct VesperDashPeriod: Codable, Equatable {
    let id: String?
    let adaptationSets: [VesperDashAdaptationSet]
}

enum VesperDashManifestType: String, Codable, Equatable {
    case `static`
    case dynamic
}

struct VesperDashManifest: Codable, Equatable {
    let type: VesperDashManifestType
    let durationMs: UInt64?
    let minBufferTimeMs: UInt64?
    let minimumUpdatePeriodMs: UInt64?
    let timeShiftBufferDepthMs: UInt64?
    let periods: [VesperDashPeriod]
}

struct VesperDashPlayableRepresentation: Codable, Equatable {
    let renditionId: String
    let adaptationSet: VesperDashAdaptationSet
    let representation: VesperDashRepresentation
}

enum VesperDashMasterPlaylistVariantPolicy: String, Codable, Equatable, Hashable {
    case all
    case startupSingleVariant
}

struct VesperDashSidxBox: Codable, Equatable {
    let timescale: UInt32
    let earliestPresentationTime: UInt64
    let firstOffset: UInt64
    let references: [VesperDashSidxReference]
}

struct VesperDashSidxReference: Codable, Equatable {
    let referenceType: UInt8
    let referencedSize: UInt32
    let subsegmentDuration: UInt32
    let startsWithSap: Bool
    let sapType: UInt8
    let sapDeltaTime: UInt32
}

struct VesperDashMediaSegment: Codable, Equatable {
    let duration: Double
    let range: VesperDashByteRange
}

struct VesperDashTemplateSegment: Codable, Equatable {
    let duration: Double
    let number: UInt64
    let time: UInt64?
}

struct VesperDashHlsMap: Codable, Equatable {
    let uri: String
    let byteRange: VesperDashByteRange?
}

struct VesperDashHlsSegment: Codable, Equatable {
    let duration: Double
    let uri: String
    let byteRange: VesperDashByteRange?
}

enum VesperDashHlsPlaylistKind: String, Codable, Equatable {
    case vod
    case live
}

enum VesperDashSegmentRequest: Hashable {
    case initialization
    case media(Int)

    var isMedia: Bool {
        if case .media = self {
            return true
        }
        return false
    }
}

enum VesperDashRoute: Equatable {
    case master
    case media(String)
    case segment(String, VesperDashSegmentRequest)
}
