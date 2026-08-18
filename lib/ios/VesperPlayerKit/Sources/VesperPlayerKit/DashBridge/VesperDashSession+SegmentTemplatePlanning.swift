@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func shouldCoalesceSegmentTemplateDownload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        allowSkippingLargeMediaEntry: Bool
    ) -> Bool {
        guard allowSkippingLargeMediaEntry else {
            return true
        }
        guard case let .media(index) = segment else {
            return true
        }
        guard
            let bandwidth = playable.representation.bandwidth,
            bandwidth > 0,
            let templateSegment = try? templateSegmentForRequest(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                index: index
            )
        else {
            return false
        }
        let estimatedBytes = templateSegment.duration * Double(bandwidth) / 8
        return estimatedBytes.isFinite
            && estimatedBytes <= Double(Self.segmentCacheMaxSingleMediaBytes)
    }

    func templateSegmentForRequest(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        index: Int
    ) throws -> VesperDashTemplateSegment {
        let segments = try templateSegments(
            for: playable,
            manifest: manifest,
            segmentTemplate: segmentTemplate
        )
        if manifest.type == .dynamic {
            guard let matched = segments.first(where: { $0.number == UInt64(index) }) else {
                throw VesperDashBridgeError.invalidManifest(
                    "missing media segment number \(index) for rendition \(playable.renditionId)"
                )
            }
            return matched
        }
        guard segments.indices.contains(index) else {
            throw VesperDashBridgeError.invalidManifest(
                "missing media segment \(index) for rendition \(playable.renditionId)"
            )
        }
        return segments[index]
    }
}
