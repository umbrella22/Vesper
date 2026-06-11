@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func startupRenditionSummary(
        _ playables: [VesperDashPlayableRepresentation]
    ) -> String {
        guard !playables.isEmpty else {
            return "none"
        }
        return playables
            .map(startupRenditionDescription)
            .joined(separator: ";")
    }

    func startupRenditionDescription(
        _ playable: VesperDashPlayableRepresentation
    ) -> String {
        let representation = playable.representation
        let capability = videoDecodeCapabilitiesCache?.first { $0.renditionId == playable.renditionId }
        return [
            "id=\(playable.renditionId)",
            "codec=\(emptyAsNil(representation.codecs))",
            "codecFamily=\(capability?.codecFamily.rawValue ?? "unknown")",
            "hardwareDecodeSupported=\(capability.map { "\($0.hardwareDecodeSupported)" } ?? "unknown")",
            "width=\(representation.width.map(String.init) ?? "nil")",
            "height=\(representation.height.map(String.init) ?? "nil")",
            "bitrate=\(representation.bandwidth.map(String.init) ?? "nil")",
            "frameRate=\(representation.frameRate ?? "nil")",
            "segmentType=\(dashSegmentTypeName(representation))",
        ].joined(separator: ",")
    }

    func masterPlaylistDecodeSelectionAttributes(
        startupSelected: VesperDashSelectedPlayableResponse
    ) -> [String: String] {
        guard let startupVideo = startupSelected.video.first else {
            return [:]
        }
        var attributes: [String: String] = [
            "startupVideoRenditionId": startupVideo.renditionId,
            "startupVideoCodec": startupVideo.representation.codecs,
            "selectionReason": "hardware_decode_startup",
        ]
        if let capability = videoDecodeCapabilitiesCache?.first(where: {
            $0.renditionId == startupVideo.renditionId
        }) {
            attributes["codecFamily"] = capability.codecFamily.rawValue
            attributes["hardwareDecodeSupported"] = "\(capability.hardwareDecodeSupported)"
            if let decoderName = capability.decoderName {
                attributes["decoderName"] = decoderName
            }
        }
        return attributes
    }

    func recordBenchmarkEvent(
        _ eventName: String,
        attributes: [String: String] = [:]
    ) async {
        guard let benchmarkEventRecorder else {
            return
        }
        await benchmarkEventRecorder(eventName, attributes)
    }

    func playlistBenchmarkEndAttributes(
        startedAt: UInt64,
        bytes: Int?,
        cacheHit: Bool,
        error: Error? = nil,
        extra: [String: String] = [:]
    ) -> [String: String] {
        var attributes = extra
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["cacheHit"] = "\(cacheHit)"
        if let bytes {
            attributes["bytes"] = "\(bytes)"
        }
        if let error {
            attributes["error"] = error.localizedDescription
        }
        return attributes
    }

    func segmentBenchmarkEventName(
        _ segment: VesperDashSegmentRequest,
        suffix: String
    ) -> String {
        switch segment {
        case .initialization:
            return "dash_init_segment_request_\(suffix)"
        case .media:
            return "dash_media_segment_request_\(suffix)"
        }
    }

    func segmentBenchmarkStartAttributes(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String
    ) -> [String: String] {
        segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
    }

    func segmentBenchmarkEndAttributes(
        startedAt: UInt64,
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String,
        result: VesperDashSegmentPayloadResult
    ) -> [String: String] {
        var attributes = segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["bytes"] = "\(result.payload.size)"
        attributes["cacheHit"] = "\(result.cacheHit)"
        attributes["coalesced"] = "\(result.coalesced)"
        attributes["segmentType"] = result.segmentType
        attributes["delivery"] = result.delivery
        attributes["contentType"] = result.payload.contentType
        if let byteRange = result.byteRange {
            attributes["byteRange"] = "\(byteRange.start)-\(byteRange.end)"
        }
        return attributes
    }

    func segmentBenchmarkEndAttributes(
        startedAt: UInt64,
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String,
        error: Error
    ) -> [String: String] {
        var attributes = segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["error"] = error.localizedDescription
        return attributes
    }

    func segmentBenchmarkBaseAttributes(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String
    ) -> [String: String] {
        var attributes = [
            "renditionId": renditionId,
            "segmentKind": dashSegmentKindName(segment),
            "requestOrigin": requestOrigin,
        ]
        if case let .media(index) = segment {
            attributes["index"] = "\(index)"
        }
        return attributes
    }

    func elapsedMillisecondsString(since startedAt: UInt64) -> String {
        let now = DispatchTime.now().uptimeNanoseconds
        let elapsedNs = now >= startedAt ? now - startedAt : 0
        return "\(elapsedNs / 1_000_000)"
    }
}
