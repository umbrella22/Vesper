@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func fetchSegmentTemplateData(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> Data {
        let url = try templateSegmentURL(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        let data = try await networkClient.data(for: url)
        // Preserve the original fMP4 segment bytes. This used to strip
        // top-level sidx boxes from media segments, but many DASH encoders
        // write tfhd.base_data_offset as an absolute offset from the segment
        // start. Removing sidx shifts mdat forward, causing AVPlayer to read
        // garbage bytes and report CoreMediaErrorDomain 1718449215 ('frmt').
        // HLS fMP4 allows sidx to remain in segments, and AVPlayer ignores it.
#if DEBUG
        logTopLevelBoxes(
            data: data,
            label: "dashSegmentTemplate",
            renditionId: playable.renditionId,
            segment: segment
        )
#endif
        return data
    }
}
