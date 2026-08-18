@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func templateSegmentURL(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) throws -> URL {
        let template: String
        let number: UInt64?
        let time: UInt64?
        switch segment {
        case .initialization:
            guard let initialization = segmentTemplate.initialization else {
                throw VesperDashBridgeError.unsupportedManifest(
                    "Representation \(playable.representation.id) does not provide SegmentTemplate initialization"
                )
            }
            template = initialization
            number = nil
            time = nil
        case let .media(index):
            let segments = try templateSegments(
                for: playable,
                manifest: manifest,
                segmentTemplate: segmentTemplate
            )
            let selectedSegment: VesperDashTemplateSegment
            if manifest.type == .dynamic {
                guard let matched = segments.first(where: { $0.number == UInt64(index) }) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment number \(index) for rendition \(playable.renditionId)"
                    )
                }
                selectedSegment = matched
            } else {
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(playable.renditionId)"
                    )
                }
                selectedSegment = segments[index]
            }
            template = segmentTemplate.media
            number = selectedSegment.number
            time = selectedSegment.time
        }

        return try expandedTemplateURL(
            playable: playable,
            template: template,
            number: number,
            time: time
        )
    }

    func expandedTemplateURL(
        playable: VesperDashPlayableRepresentation,
        template: String,
        number: UInt64?,
        time: UInt64?
    ) throws -> URL {
        let expanded = try VesperDashTemplateExpander.expand(
            template,
            representation: playable.representation,
            number: number,
            time: time
        )
        let resolved = resolveDashURI(base: playable.representation.baseURL, reference: expanded)
        guard let url = URL(string: resolved) else {
            throw VesperDashBridgeError.invalidManifest(
                "invalid segment URL \(diagnosticURLDescription(resolved))"
            )
        }
        return url
    }
}
