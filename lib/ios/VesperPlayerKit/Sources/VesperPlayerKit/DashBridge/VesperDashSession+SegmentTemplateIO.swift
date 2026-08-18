@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func cachedSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayloadResult {
        let contentType = dashSegmentContentType(for: playable, segment: segment)
        let key = VesperDashSegmentCacheKey(
            renditionId: playable.renditionId,
            segment: segment
        )
        let cacheURL = segmentCacheURL(
            renditionId: playable.renditionId,
            segment: segment
        )
        if let cached = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType) {
            return VesperDashSegmentPayloadResult(
                payload: cached,
                cacheHit: true,
                segmentType: "template",
                byteRange: nil,
                delivery: "cacheFile"
            )
        }

        if shouldCoalesceSegmentTemplateDownload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment,
            allowSkippingLargeMediaEntry: true
        ) {
            return try await coalescedSegmentTemplatePayload(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: true,
                contentType: contentType
            )
        }

        return try await fetchSegmentTemplatePayload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment,
            cacheURL: cacheURL,
            key: key,
            allowSkippingLargeMediaEntry: true,
            contentType: contentType
        )
    }
}
