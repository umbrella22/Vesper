@preconcurrency import AVFoundation
import Foundation
internal import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func isSubtitleRendition(renditionId: String) async -> Bool {
        guard let playable = try? await playableRepresentation(renditionId: renditionId) else {
            return false
        }
        return playable.adaptationSet.kind == .subtitle
    }

    func dashSegmentContentType(
        for playable: VesperDashPlayableRepresentation,
        segment: VesperDashSegmentRequest
    ) -> String {
        // Initialization segments historically returned `video/mp4`
        // unconditionally. Subtitle adaptation sets typically have no
        // init segment, but if one exists (e.g. an MP4-wrapped wvtt init),
        // return the rendition MIME type so AVPlayer does not mislabel it.
        // The body of `dashSegmentContentType` for `.media` segments already
        // routes by representation MIME type, so this brings the
        // initialization branch in line.
        let mimeType = playable.representation.mimeType
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        if mimeType == "text/vtt" || mimeType == "text/webvtt" || mimeType.contains("vtt") {
            return "text/vtt"
        }
        return "video/mp4"
    }

    /// Returns `"vtt"` when the rendition is a WebVTT subtitle (so segment
    /// URLs use a MIME-aware extension), otherwise `nil` so the caller keeps
    /// the default `.m4s`/`init.mp4` audio/video naming. Used by
    /// `buildMediaPlaylistData` to emit subtitle-aware HLS media playlist
    /// URIs.
    func subtitleSegmentFileExtension(
        for playable: VesperDashPlayableRepresentation
    ) -> String? {
        guard playable.adaptationSet.kind == .subtitle else { return nil }
        let mimeType = playable.representation.mimeType
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        if mimeType == "text/vtt" || mimeType == "text/webvtt" || mimeType.contains("vtt") {
            return "vtt"
        }
        return nil
    }

    func segmentData(renditionId: String, segment: VesperDashSegmentRequest) async throws -> Data {
        try await segmentPayload(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: "resourceLoader"
        ).readData()
    }

    func segmentResourcePayload(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayload {
        try await segmentPayload(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: "resourceLoader"
        )
    }

    func segmentPayload(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String = "playback"
    ) async throws -> VesperDashSegmentPayload {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent(
            segmentBenchmarkEventName(segment, suffix: "start"),
            attributes: segmentBenchmarkStartAttributes(
                renditionId: renditionId,
                segment: segment,
                requestOrigin: requestOrigin
            )
        )
        do {
            let result = try await resolveSegmentPayload(renditionId: renditionId, segment: segment)
            await recordBenchmarkEvent(
                segmentBenchmarkEventName(segment, suffix: "end"),
                attributes: segmentBenchmarkEndAttributes(
                    startedAt: startedAt,
                    renditionId: renditionId,
                    segment: segment,
                    requestOrigin: requestOrigin,
                    result: result
                )
            )
            return result.payload
        } catch {
            await recordBenchmarkEvent(
                segmentBenchmarkEventName(segment, suffix: "end"),
                attributes: segmentBenchmarkEndAttributes(
                    startedAt: startedAt,
                    renditionId: renditionId,
                    segment: segment,
                    requestOrigin: requestOrigin,
                    error: error
                )
            )
            throw error
        }
    }

    func resolveSegmentPayload(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayloadResult {
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
            if manifest.type == .dynamic {
                throw VesperDashBridgeError.unsupportedManifest(
                    "dynamic DASH SegmentBase is not supported on iOS"
                )
            }
            guard let mediaURL = URL(string: playable.representation.baseURL) else {
                throw VesperDashBridgeError.invalidManifest(
                    "invalid media URL \(playable.representation.baseURL)"
                )
            }

            let byteRange: VesperDashByteRange
            switch segment {
            case .initialization:
                byteRange = segmentBase.initialization
            case let .media(index):
                let segments = try await mediaSegments(for: playable, segmentBase: segmentBase)
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(renditionId)"
                    )
                }
                byteRange = segments[index].range
            }

            if mediaURL.isFileURL {
                let payload = VesperDashSegmentPayload.file(
                    url: mediaURL,
                    offset: byteRange.start,
                    size: byteRange.length,
                    removeAfterServing: false,
                    contentType: dashSegmentContentType(for: playable, segment: segment)
                )
                return VesperDashSegmentPayloadResult(
                    payload: payload,
                    cacheHit: false,
                    segmentType: "base",
                    byteRange: byteRange,
                    delivery: "localFile"
                )
            }
            let data = try await networkClient.data(for: mediaURL, byteRange: byteRange)
            return VesperDashSegmentPayloadResult(
                payload: .data(
                    data,
                    contentType: dashSegmentContentType(for: playable, segment: segment)
                ),
                cacheHit: false,
                segmentType: "base",
                byteRange: byteRange,
                delivery: "networkData"
            )
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        return try await cachedSegmentTemplatePayload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
    }
}
