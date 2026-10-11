import Foundation
func selectDashRepresentations(
    manifestText: String,
    manifestUri: String,
    profile: VesperDownloadProfile
) -> [DashPlannedRepresentation] {
    let mpdBase = directXmlText(manifestText, tag: "BaseURL", before: ["Period", "AdaptationSet", "Representation"])
        .map { resolveRemoteReference(baseUri: manifestUri, reference: $0) }
        ?? manifestUri
    var result: [DashPlannedRepresentation] = []
    let periods = xmlBlocks(manifestText, tag: "Period")
    let containers = periods.isEmpty ? [manifestText] : periods
    let adaptationContexts: [(adaptationSet: String, parentBase: String)] = containers.flatMap { container in
        let periodBase = periods.isEmpty ? mpdBase : directXmlText(container, tag: "BaseURL", before: ["AdaptationSet", "Representation"])
            .map { resolveRemoteReference(baseUri: mpdBase, reference: $0) } ?? mpdBase
        return xmlBlocks(container, tag: "AdaptationSet").map { ($0, periodBase) }
    }

    for (index, context) in adaptationContexts.enumerated() {
        let adaptationSet = context.adaptationSet
        let adaptationOpenTag = xmlOpenTag(adaptationSet, tag: "AdaptationSet") ?? ""
        let adaptationMimeType = xmlAttrFromTag(adaptationOpenTag, attr: "mimeType")
        let adaptationContentType = xmlAttrFromTag(adaptationOpenTag, attr: "contentType")
        if let adaptationMimeType,
           !adaptationMimeType.hasPrefix("video/"),
           !adaptationMimeType.hasPrefix("audio/") {
            continue
        }

        let adaptationBase = directXmlText(adaptationSet, tag: "BaseURL", before: ["Representation"])
            .map { resolveRemoteReference(baseUri: context.parentBase, reference: $0) }
            ?? context.parentBase
        let adaptationTemplate = findDashTemplate(prefixBeforeTag(adaptationSet, tag: "Representation"))
        let representations = xmlBlocks(adaptationSet, tag: "Representation")
        guard !representations.isEmpty else {
            continue
        }

        let selectedRepresentation = profile.variantId.flatMap { variantId in
            representations.first { representation in
                xmlAttrFromTag(xmlOpenTag(representation, tag: "Representation") ?? "", attr: "id") == variantId
            }
        } ?? representations.first
        guard let selectedRepresentation else {
            continue
        }

        let representationOpenTag = xmlOpenTag(selectedRepresentation, tag: "Representation") ?? ""
        let id = xmlAttrFromTag(representationOpenTag, attr: "id") ?? "\(index)"
        let representationBase = xmlText(selectedRepresentation, tag: "BaseURL")
        let template = findDashTemplate(selectedRepresentation) ?? adaptationTemplate
        let mimeType = xmlAttrFromTag(representationOpenTag, attr: "mimeType") ?? adaptationMimeType
        let mediaKind: String
        if mimeType?.hasPrefix("audio/") == true || adaptationContentType == "audio" {
            mediaKind = "audio"
        } else if mimeType?.hasPrefix("video/") == true || adaptationContentType == "video" {
            mediaKind = "video"
        } else {
            mediaKind = "media"
        }

        result.append(
            DashPlannedRepresentation(
                id: id,
                mediaId: "\(mediaKind)\(index)",
                mimeType: mimeType,
                codecs: xmlAttrFromTag(representationOpenTag, attr: "codecs"),
                bandwidth: xmlAttrFromTag(representationOpenTag, attr: "bandwidth"),
                baseUri: representationBase.map { resolveRemoteReference(baseUri: adaptationBase, reference: $0) } ?? adaptationBase,
                baseUrl: template == nil ? representationBase.map { resolveRemoteReference(baseUri: adaptationBase, reference: $0) } : nil,
                template: template
            )
        )
    }

    if result.isEmpty,
       let baseURL = directXmlText(manifestText, tag: "BaseURL", before: ["Period", "AdaptationSet", "Representation"]) {
        result.append(
            DashPlannedRepresentation(
                id: "0",
                mediaId: "media0",
                mimeType: nil,
                codecs: nil,
                bandwidth: nil,
                baseUri: resolveRemoteReference(baseUri: manifestUri, reference: baseURL),
                baseUrl: resolveRemoteReference(baseUri: manifestUri, reference: baseURL),
                template: nil
            )
        )
    }

    return result
}

func findDashTemplate(_ input: String) -> DashTemplate? {
    guard
        let tag = xmlOpenTag(input, tag: "SegmentTemplate"),
        let media = xmlAttrFromTag(tag, attr: "media")
    else {
        return nil
    }
    return DashTemplate(
        media: media,
        initialization: xmlAttrFromTag(tag, attr: "initialization"),
        startNumber: xmlAttrFromTag(tag, attr: "startNumber").flatMap(UInt64.init) ?? 1,
        timescale: xmlAttrFromTag(tag, attr: "timescale").flatMap(UInt64.init) ?? 1,
        duration: xmlAttrFromTag(tag, attr: "duration").flatMap(UInt64.init) ?? 0
    )
}

func plannedDashSegmentCount(durationSeconds: Double, segmentSeconds: Double, startNumber: UInt64) throws -> UInt64 {
    guard durationSeconds.isFinite, durationSeconds > 0,
          segmentSeconds.isFinite, segmentSeconds > 0,
          let count = UInt64(exactly: ceil(durationSeconds / segmentSeconds)), count > 0 else {
        throw VesperForegroundDownloadPreparationError.invalidSource("DASH segment count is not representable")
    }
    let (_, overflow) = startNumber.addingReportingOverflow(count - 1)
    guard !overflow else {
        throw VesperForegroundDownloadPreparationError.invalidSource("DASH segment numbering overflows")
    }
    return count
}
