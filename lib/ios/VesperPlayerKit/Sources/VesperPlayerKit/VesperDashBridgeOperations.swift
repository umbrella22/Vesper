import Foundation
import VesperPlayerKitBridgeShim

private enum VesperDashRustBridge {
    static func execute<Request: Encodable, Response: Decodable>(
        _ request: Request,
        response _: Response.Type = Response.self
    ) throws -> Response {
        let requestData = try JSONEncoder().encode(request)
        guard let requestJson = String(data: requestData, encoding: .utf8) else {
            throw VesperDashBridgeError.invalidManifest("failed to encode DASH bridge request")
        }

        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = requestJson.withCString { requestPointer in
            vesper_dash_bridge_execute_json(requestPointer, &outputPointer, &errorPointer)
        }
        defer {
            if let outputPointer {
                vesper_dash_bridge_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_dash_bridge_string_free(errorPointer)
            }
        }

        guard ok, let outputPointer else {
            let message = errorPointer.map { String(cString: $0) } ?? "Rust DASH bridge call failed"
            throw bridgeError(fromRustMessage: message)
        }

        let responseJson = String(cString: outputPointer)
        guard let responseData = responseJson.data(using: .utf8) else {
            throw VesperDashBridgeError.invalidManifest("failed to decode DASH bridge response")
        }
        do {
            return try JSONDecoder().decode(Response.self, from: responseData)
        } catch {
            throw VesperDashBridgeError.invalidManifest(
                "invalid DASH bridge response: \(error.localizedDescription)"
            )
        }
    }

    private static func bridgeError(fromRustMessage message: String) -> VesperDashBridgeError {
        if message.hasPrefix("unsupported MPD:") {
            return .unsupportedManifest(message)
        }
        if message.hasPrefix("invalid MPD:") {
            return .invalidManifest(message)
        }
        if message.hasPrefix("unsupported MP4:") {
            return .unsupportedMp4(message)
        }
        if message.hasPrefix("invalid MP4:") {
            return .invalidMp4(message)
        }
        return .invalidManifest(message)
    }
}

private struct VesperDashParseManifestRequest: Encodable {
    let operation = "parse_manifest"
    let mpd: String
    let manifestUrl: String
}

private struct VesperDashParseSidxRequest: Encodable {
    let operation = "parse_sidx"
    let data: [UInt8]
}

private struct VesperDashRemoveTopLevelSidxRequest: Encodable {
    let operation = "remove_top_level_sidx"
    let data: [UInt8]
}

private struct VesperDashSelectedPlayableRequest: Encodable {
    let operation = "selected_playable_representations"
    let manifest: VesperDashManifest
    let variantPolicy: VesperDashMasterPlaylistVariantPolicy
    let videoDecodeCapabilities: [VesperDashVideoDecodeCapability]?
}

private struct VesperDashRenditionUrl: Codable, Equatable {
    let renditionId: String
    let url: String
}

private struct VesperDashBuildMasterPlaylistRequest: Encodable {
    let operation = "build_master_playlist"
    let manifest: VesperDashManifest
    let variantPolicy: VesperDashMasterPlaylistVariantPolicy
    let mediaUrls: [VesperDashRenditionUrl]
    let videoDecodeCapabilities: [VesperDashVideoDecodeCapability]?
}

struct VesperDashSelectedPlayableResponse: Codable, Equatable {
    let audio: [VesperDashPlayableRepresentation]
    let video: [VesperDashPlayableRepresentation]
    let subtitles: [VesperDashPlayableRepresentation]
}

private struct VesperDashMasterPlaylistResponse: Codable, Equatable {
    let playlist: String
    let selected: VesperDashSelectedPlayableResponse
}

private struct VesperDashMediaSegmentsRequest: Encodable {
    let operation = "media_segments"
    let segmentBase: VesperDashSegmentBase
    let sidx: VesperDashSidxBox
}

private struct VesperDashTemplateSegmentsRequest: Encodable {
    let operation = "template_segments"
    let manifestType: VesperDashManifestType?
    let durationMs: UInt64?
    let segmentTemplate: VesperDashSegmentTemplate
}

private struct VesperDashBuildExternalMediaPlaylistRequest: Encodable {
    let operation = "build_external_media_playlist"
    let map: VesperDashHlsMap?
    let segments: [VesperDashHlsSegment]
    let playlistKind: VesperDashHlsPlaylistKind
    let mediaSequence: UInt64?
}

private struct VesperDashExpandTemplateRequest: Encodable {
    let operation = "expand_template"
    let template: String
    let representation: VesperDashRepresentation
    let number: UInt64?
    let time: UInt64?
}

enum VesperDashManifestParser {
    static func parse(data: Data, manifestURL: URL) throws -> VesperDashManifest {
        guard let mpd = String(data: data, encoding: .utf8) else {
            throw VesperDashBridgeError.invalidManifest("MPD is not valid UTF-8")
        }
        if mpd.range(
            of: #"<[^>]*ContentProtection\b"#,
            options: [.regularExpression, .caseInsensitive]
        ) != nil {
            throw VesperDashBridgeError.unsupportedManifest(
                "DASH ContentProtection/DRM is not supported on iOS"
            )
        }
        if mpd.range(
            of: #"<[^>]*SegmentList\b"#,
            options: [.regularExpression, .caseInsensitive]
        ) != nil {
            throw VesperDashBridgeError.unsupportedManifest(
                "DASH SegmentList is not supported on iOS"
            )
        }
        return try VesperDashRustBridge.execute(
            VesperDashParseManifestRequest(
                mpd: mpd,
                manifestUrl: manifestURL.absoluteString
            ),
            response: VesperDashManifest.self
        )
    }
}

enum VesperDashSidxParser {
    static func parse(data: Data) throws -> VesperDashSidxBox {
        try VesperDashRustBridge.execute(
            VesperDashParseSidxRequest(data: [UInt8](data)),
            response: VesperDashSidxBox.self
        )
    }
}

enum VesperDashMp4BoxFilter {
    static func removingTopLevelSidxBoxes(from data: Data) throws -> Data {
        let bytes: [UInt8] = try VesperDashRustBridge.execute(
            VesperDashRemoveTopLevelSidxRequest(data: [UInt8](data)),
            response: [UInt8].self
        )
        return Data(bytes)
    }
}

enum VesperDashHlsBuilder {
    /// 构造 HLS master playlist。
    ///
    /// 实现注意：所有 playlist 文本生成都使用 `[String]` lines + `joined("\n")` 模式，
    /// 不再使用多行字符串字面量直接 `+=` 拼接。原因：Swift 多行字面量结尾的 `"""`
    /// 会吞掉前一个换行，曾经导致 `#EXT-X-PLAYLIST-TYPE:VOD` 与后面 `#EXT-X-MAP`
    /// 粘到同一行，HLS 解析器静默忽略未知整行 → AVPlayer 拿不到 init segment
    /// → 报 `'frmt'`。逐行 append 在物理上不可能粘行。
    static func buildMasterPlaylist(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy = .all,
        videoDecodeCapabilities: [VesperDashVideoDecodeCapability]? = nil,
        mediaURL: (String) -> String
    ) throws -> String {
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: variantPolicy,
            videoDecodeCapabilities: videoDecodeCapabilities
        )
        let mediaUrls = (selected.audio + selected.video + selected.subtitles).map {
            VesperDashRenditionUrl(renditionId: $0.renditionId, url: mediaURL($0.renditionId))
        }
        let response: VesperDashMasterPlaylistResponse = try VesperDashRustBridge.execute(
            VesperDashBuildMasterPlaylistRequest(
                manifest: manifest,
                variantPolicy: variantPolicy,
                mediaUrls: mediaUrls,
                videoDecodeCapabilities: videoDecodeCapabilities
            ),
            response: VesperDashMasterPlaylistResponse.self
        )
        return response.playlist
    }

    static func selectedPlayableRepresentations(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy,
        videoDecodeCapabilities: [VesperDashVideoDecodeCapability]? = nil
    ) throws -> (
        audio: [VesperDashPlayableRepresentation],
        video: [VesperDashPlayableRepresentation],
        subtitles: [VesperDashPlayableRepresentation]
    ) {
        let selected: VesperDashSelectedPlayableResponse = try VesperDashRustBridge.execute(
            VesperDashSelectedPlayableRequest(
                manifest: manifest,
                variantPolicy: variantPolicy,
                videoDecodeCapabilities: videoDecodeCapabilities
            ),
            response: VesperDashSelectedPlayableResponse.self
        )
        return (selected.audio, selected.video, selected.subtitles)
    }

    static func buildMediaPlaylist(
        initializationURI: String,
        segments: [VesperDashMediaSegment],
        segmentURI: (Int) -> String
    ) throws -> String {
        try buildExternalMediaPlaylist(
            map: VesperDashHlsMap(uri: initializationURI, byteRange: nil),
            playlistKind: .vod,
            mediaSequence: nil,
            segments: segments.enumerated().map { index, segment in
                VesperDashHlsSegment(
                    duration: segment.duration,
                    uri: segmentURI(index),
                    byteRange: nil
                )
            }
        )
    }

    static func buildMediaPlaylist(
        initializationURI: String,
        segments: [VesperDashTemplateSegment],
        segmentURI: (Int) -> String
    ) throws -> String {
        try buildMediaPlaylist(
            initializationURI: initializationURI,
            segments: segments,
            playlistKind: .vod,
            mediaSequence: nil,
            segmentURI: { index, _ in segmentURI(index) }
        )
    }

    static func buildMediaPlaylist(
        initializationURI: String?,
        segments: [VesperDashTemplateSegment],
        playlistKind: VesperDashHlsPlaylistKind = .vod,
        mediaSequence: UInt64? = nil,
        segmentURI: (Int, VesperDashTemplateSegment) throws -> String
    ) throws -> String {
        try buildExternalMediaPlaylist(
            map: initializationURI.map { VesperDashHlsMap(uri: $0, byteRange: nil) },
            playlistKind: playlistKind,
            mediaSequence: mediaSequence,
            segments: try segments.enumerated().map { index, segment in
                VesperDashHlsSegment(
                    duration: segment.duration,
                    uri: try segmentURI(index, segment),
                    byteRange: nil
                )
            }
        )
    }

    static func buildExternalMediaPlaylist(
        map: VesperDashHlsMap?,
        playlistKind: VesperDashHlsPlaylistKind = .vod,
        mediaSequence: UInt64? = nil,
        segments: [VesperDashHlsSegment]
    ) throws -> String {
        try VesperDashRustBridge.execute(
            VesperDashBuildExternalMediaPlaylistRequest(
                map: map,
                segments: segments,
                playlistKind: playlistKind,
                mediaSequence: mediaSequence
            ),
            response: String.self
        )
    }

    static func mediaSegments(
        segmentBase: VesperDashSegmentBase,
        sidx: VesperDashSidxBox
    ) throws -> [VesperDashMediaSegment] {
        try VesperDashRustBridge.execute(
            VesperDashMediaSegmentsRequest(
                segmentBase: segmentBase,
                sidx: sidx
            ),
            response: [VesperDashMediaSegment].self
        )
    }

    static func templateSegments(
        manifestType: VesperDashManifestType? = nil,
        durationMs: UInt64?,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> [VesperDashTemplateSegment] {
        try VesperDashRustBridge.execute(
            VesperDashTemplateSegmentsRequest(
                manifestType: manifestType,
                durationMs: durationMs,
                segmentTemplate: segmentTemplate
            ),
            response: [VesperDashTemplateSegment].self
        )
    }
}

enum VesperDashTemplateExpander {
    static func expand(
        _ template: String,
        representation: VesperDashRepresentation,
        number: UInt64?,
        time: UInt64? = nil
    ) throws -> String {
        try VesperDashRustBridge.execute(
            VesperDashExpandTemplateRequest(
                template: template,
                representation: representation,
                number: number,
                time: time
            ),
            response: String.self
        )
    }
}
