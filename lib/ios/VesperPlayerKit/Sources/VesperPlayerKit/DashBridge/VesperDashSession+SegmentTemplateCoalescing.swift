@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func coalescedSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayloadResult {
        if let inFlightTask = segmentDownloadTasksByKey[key] {
            let result = try await inFlightTask.value
            if let cached = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType) {
                return result.markingCoalesced(
                    payload: cached,
                    cacheHit: true,
                    delivery: "coalescedCacheFile"
                )
            }
            guard !result.payload.isTemporaryFile else {
                return try await fetchSegmentTemplatePayload(
                    manifest: manifest,
                    playable: playable,
                    segmentTemplate: segmentTemplate,
                    segment: segment,
                    cacheURL: cacheURL,
                    key: key,
                    allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
                    contentType: contentType
                )
            }
            return result.markingCoalesced()
        }

        let downloadTask = Task {
            try await self.fetchSegmentTemplatePayload(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
                contentType: contentType
            )
        }
        segmentDownloadTasksByKey[key] = downloadTask
        do {
            let result = try await downloadTask.value
            segmentDownloadTasksByKey[key] = nil
            return result
        } catch {
            segmentDownloadTasksByKey[key] = nil
            throw error
        }
    }
}
