import Foundation
func rewriteDashMpd(
    duration: String?,
    adaptationSets: [String]
) -> String {
    var text = "<MPD type=\"static\""
    if let duration, !duration.isEmpty {
        text += " mediaPresentationDuration=\"\(escapeXml(duration))\""
    }
    text += " xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period>"
    text += adaptationSets.joined()
    text += "</Period></MPD>\n"
    return text
}

func rewriteDashTemplateAdaptationSet(
    representation: DashPlannedRepresentation,
    template: DashTemplate,
    mediaId: String,
    segmentCount: UInt64
) -> String {
    let mime = representation.mimeType.map { " mimeType=\"\(escapeXml($0))\"" } ?? ""
    let codecs = representation.codecs.map { " codecs=\"\(escapeXml($0))\"" } ?? ""
    let bandwidth = representation.bandwidth ?? "1"
    let initialization = template.initialization == nil ? "" : " initialization=\"segments/\(mediaId)-init.mp4\""
    return "<AdaptationSet\(mime)><Representation id=\"\(escapeXml(representation.id))\" bandwidth=\"\(escapeXml(bandwidth))\"\(codecs)><SegmentTemplate timescale=\"\(template.timescale)\" duration=\"\(template.duration)\" startNumber=\"\(template.startNumber)\"\(initialization) media=\"segments/\(mediaId)-$Number$.m4s\" /></Representation></AdaptationSet><!-- plannedSegments=\(segmentCount) -->"
}

func rewriteDashSegmentBaseAdaptationSet(
    representation: DashPlannedRepresentation,
    localName: String
) -> String {
    let mime = representation.mimeType.map { " mimeType=\"\(escapeXml($0))\"" } ?? ""
    let codecs = representation.codecs.map { " codecs=\"\(escapeXml($0))\"" } ?? ""
    let bandwidth = representation.bandwidth ?? "1"
    return "<AdaptationSet\(mime)><Representation id=\"\(escapeXml(representation.id))\" bandwidth=\"\(escapeXml(bandwidth))\"\(codecs)><BaseURL>\(escapeXml(localName))</BaseURL><SegmentBase /></Representation></AdaptationSet>"
}

func expandDashTemplate(
    _ template: String,
    representationId: String,
    number: UInt64
) -> String {
    replaceDashNumberToken(
        template.replacingOccurrences(of: "$RepresentationID$", with: representationId),
        number: number
    )
}

func replaceDashNumberToken(_ value: String, number: UInt64) -> String {
    var output = value.replacingOccurrences(of: "$Number$", with: "\(number)")
    while let start = output.range(of: "$Number%") {
        guard let end = output[start.upperBound...].firstIndex(of: "$") else {
            return output
        }
        let formatSpec = String(output[start.upperBound..<end])
        let width = Int(formatSpec.trimmingCharacters(in: CharacterSet(charactersIn: "d")).dropFirst()) ?? 0
        output.replaceSubrange(start.lowerBound...end, with: padded(number, width: width))
    }
    return output
}
