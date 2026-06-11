import Foundation

struct HlsMasterPlaylist {
    let variants: [HlsVariant]
    let audio: [HlsRendition]
}

struct HlsVariant {
    let uri: String
    let attributes: [String: String]
}

struct HlsRendition {
    let uri: String
    let attributes: [String: String]
}

struct HlsMediaPlaylist {
    let targetDuration: String?
    let version: String?
    let maps: [HlsMap]
    let segments: [HlsSegment]
}

struct HlsMap {
    let uri: String
    let byteRange: VesperDownloadByteRange?
}

struct HlsSegment {
    let uri: String
    let duration: String?
    let byteRange: VesperDownloadByteRange?
    let sequence: UInt64
}

struct DashPlannedRepresentation {
    let id: String
    let mediaId: String
    let mimeType: String?
    let codecs: String?
    let bandwidth: String?
    let baseUri: String
    let baseUrl: String?
    let template: DashTemplate?
}

struct DashTemplate {
    let media: String
    let initialization: String?
    let startNumber: UInt64
    let timescale: UInt64
    let duration: UInt64
}
