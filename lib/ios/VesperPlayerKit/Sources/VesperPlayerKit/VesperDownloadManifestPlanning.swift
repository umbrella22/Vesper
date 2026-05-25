import Foundation

struct HlsMasterPlaylist {
    let variants: [HlsVariant]
    let audio: [HlsRendition]
}

struct HlsVariant {
    let uri: String
    let attributes: [String: String]
}

struct HlsRendition {
    let uri: String
    let attributes: [String: String]
}

struct HlsMediaPlaylist {
    let targetDuration: String?
    let version: String?
    let maps: [HlsMap]
    let segments: [HlsSegment]
}

struct HlsMap {
    let uri: String
    let byteRange: VesperDownloadByteRange?
}

struct HlsSegment {
    let uri: String
    let duration: String?
    let byteRange: VesperDownloadByteRange?
    let sequence: UInt64
}

struct DashPlannedRepresentation {
    let id: String
    let mediaId: String
    let mimeType: String?
    let codecs: String?
    let bandwidth: String?
    let baseUri: String
    let baseUrl: String?
    let template: DashTemplate?
}

struct DashTemplate {
    let media: String
    let initialization: String?
    let startNumber: UInt64
    let timescale: UInt64
    let duration: UInt64
}

func parseHlsMasterPlaylist(
    manifestUri: String,
    manifestText: String
) -> HlsMasterPlaylist {
    var variants: [HlsVariant] = []
    var audio: [HlsRendition] = []
    var pendingVariant: [String: String]?

    for line in nonEmptyTrimmedLines(manifestText) {
        if let value = valueAfterPrefix("#EXT-X-STREAM-INF:", in: line) {
            pendingVariant = parseHlsAttributes(value)
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-MEDIA:", in: line) {
            let attributes = parseHlsAttributes(value)
            if attributes["TYPE"]?.caseInsensitiveCompare("AUDIO") == .orderedSame,
               let uri = attributes["URI"] {
                audio.append(
                    HlsRendition(
                        uri: resolveRemoteReference(baseUri: manifestUri, reference: uri),
                        attributes: attributes
                    )
                )
            }
            continue
        }
        if line.hasPrefix("#") {
            continue
        }
        if let attributes = pendingVariant {
            variants.append(
                HlsVariant(
                    uri: resolveRemoteReference(baseUri: manifestUri, reference: line),
                    attributes: attributes
                )
            )
            pendingVariant = nil
        }
    }

    return HlsMasterPlaylist(variants: variants, audio: audio)
}

func parseHlsMediaPlaylist(
    playlistUri: String,
    playlistText: String
) throws -> HlsMediaPlaylist {
    var targetDuration: String?
    var version: String?
    var endList = false
    var playlistTypeVod = false
    var pendingDuration: String?
    var pendingByteRange: VesperDownloadByteRange?
    var previousRangeEnd: UInt64 = 0
    var sequence: UInt64 = 0
    var maps: [HlsMap] = []
    var segments: [HlsSegment] = []

    for line in nonEmptyTrimmedLines(playlistText) {
        if let value = valueAfterPrefix("#EXT-X-TARGETDURATION:", in: line) {
            targetDuration = value.trimmingCharacters(in: .whitespacesAndNewlines)
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-VERSION:", in: line) {
            version = value.trimmingCharacters(in: .whitespacesAndNewlines)
            continue
        }
        if line.caseInsensitiveCompare("#EXT-X-ENDLIST") == .orderedSame {
            endList = true
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-PLAYLIST-TYPE:", in: line) {
            playlistTypeVod = value.trimmingCharacters(in: .whitespacesAndNewlines)
                .caseInsensitiveCompare("VOD") == .orderedSame
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-MAP:", in: line) {
            let attributes = parseHlsAttributes(value)
            guard let uri = attributes["URI"] else {
                throw VesperForegroundDownloadPreparationError.invalidSource("HLS EXT-X-MAP was missing URI")
            }
            let byteRange = attributes["BYTERANGE"].flatMap {
                parseHlsByteRange($0, previousRangeEnd: &previousRangeEnd)
            }
            maps.append(
                HlsMap(
                    uri: resolveRemoteReference(baseUri: playlistUri, reference: uri),
                    byteRange: byteRange
                )
            )
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-BYTERANGE:", in: line) {
            pendingByteRange = parseHlsByteRange(value, previousRangeEnd: &previousRangeEnd)
            continue
        }
        if let value = valueAfterPrefix("#EXTINF:", in: line) {
            pendingDuration = value.components(separatedBy: ",").first?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            continue
        }
        if line.hasPrefix("#") {
            continue
        }

        sequence += 1
        segments.append(
            HlsSegment(
                uri: resolveRemoteReference(baseUri: playlistUri, reference: line),
                duration: pendingDuration,
                byteRange: pendingByteRange,
                sequence: sequence
            )
        )
        pendingDuration = nil
        pendingByteRange = nil
    }

    if !endList && !playlistTypeVod {
        throw VesperForegroundDownloadPreparationError.unsupported("HLS download preparation requires a VOD playlist or EXT-X-ENDLIST")
    }
    if segments.isEmpty {
        throw VesperForegroundDownloadPreparationError.invalidSource("HLS media playlist did not contain any segments")
    }

    return HlsMediaPlaylist(
        targetDuration: targetDuration,
        version: version,
        maps: maps,
        segments: segments
    )
}

func rewriteHlsMaster(
    variantAttributes: [String: String],
    mediaResourceNames: [String]
) -> String {
    let audioPlaylist = mediaResourceNames.first { $0.hasPrefix("audio") }
    let videoPlaylist = mediaResourceNames.first { $0.hasPrefix("video") }
        ?? mediaResourceNames.first
        ?? "video.m3u8"
    let bandwidth = variantAttributes["BANDWIDTH"] ?? "1"
    var text = "#EXTM3U\n#EXT-X-VERSION:3\n"
    if let audioPlaylist {
        text += "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"audio\",DEFAULT=YES,AUTOSELECT=YES,URI=\"\(audioPlaylist)\"\n"
        text += "#EXT-X-STREAM-INF:BANDWIDTH=\(bandwidth),AUDIO=\"audio\"\n"
    } else {
        text += "#EXT-X-STREAM-INF:BANDWIDTH=\(bandwidth)\n"
    }
    text += "\(videoPlaylist)\n"
    return text
}

func rewriteHlsMedia(
    mediaId: String,
    playlist: HlsMediaPlaylist,
    localMaps: [String: String]
) -> String {
    var text = "#EXTM3U\n"
    text += "#EXT-X-VERSION:\(playlist.version ?? "3")\n"
    text += "#EXT-X-PLAYLIST-TYPE:VOD\n"
    if let targetDuration = playlist.targetDuration {
        text += "#EXT-X-TARGETDURATION:\(targetDuration)\n"
    }
    if let map = playlist.maps.last,
       let path = localMaps[hlsByteRangeKey(uri: map.uri, byteRange: map.byteRange)] {
        text += "#EXT-X-MAP:URI=\"\(path)\"\n"
    }
    for segment in playlist.segments {
        text += "#EXTINF:\(segment.duration ?? "0"),\n"
        text += "segments/\(mediaId)-\(padded(segment.sequence, width: 5)).\(extensionFromUri(segment.uri, fallback: "ts"))\n"
    }
    text += "#EXT-X-ENDLIST\n"
    return text
}

func parseHlsAttributes(_ input: String) -> [String: String] {
    var attributes: [String: String] = [:]
    for pair in splitQuoted(input, delimiter: ",") {
        let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
        guard parts.count == 2 else { continue }
        let key = parts[0].trimmingCharacters(in: .whitespacesAndNewlines)
        let value = parts[1]
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "\""))
        if !key.isEmpty {
            attributes[key] = value
        }
    }
    return attributes
}

func parseHlsByteRange(
    _ value: String,
    previousRangeEnd: inout UInt64
) -> VesperDownloadByteRange? {
    let parts = value.trimmingCharacters(in: .whitespacesAndNewlines)
        .split(separator: "@", maxSplits: 1, omittingEmptySubsequences: false)
    guard let length = UInt64(parts.first?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "") else {
        return nil
    }
    let offset = parts.count > 1
        ? UInt64(parts[1].trimmingCharacters(in: .whitespacesAndNewlines)) ?? previousRangeEnd
        : previousRangeEnd
    previousRangeEnd = offset + length
    return VesperDownloadByteRange(offset: offset, length: length)
}

func selectDashRepresentations(
    manifestText: String,
    manifestUri: String,
    profile: VesperDownloadProfile
) -> [DashPlannedRepresentation] {
    let mpdBase = directXmlText(manifestText, tag: "BaseURL", before: ["Period", "AdaptationSet", "Representation"])
        .map { resolveRemoteReference(baseUri: manifestUri, reference: $0) }
        ?? manifestUri
    var result: [DashPlannedRepresentation] = []
    let adaptationSets = xmlBlocks(manifestText, tag: "AdaptationSet")

    for (index, adaptationSet) in adaptationSets.enumerated() {
        let adaptationOpenTag = xmlOpenTag(adaptationSet, tag: "AdaptationSet") ?? ""
        let adaptationMimeType = xmlAttrFromTag(adaptationOpenTag, attr: "mimeType")
        let adaptationContentType = xmlAttrFromTag(adaptationOpenTag, attr: "contentType")
        if let adaptationMimeType,
           !adaptationMimeType.hasPrefix("video/"),
           !adaptationMimeType.hasPrefix("audio/") {
            continue
        }

        let adaptationBase = directXmlText(adaptationSet, tag: "BaseURL", before: ["Representation"])
            .map { resolveRemoteReference(baseUri: mpdBase, reference: $0) }
            ?? mpdBase
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
                baseUrl: template == nil ? representationBase : nil,
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
                baseUri: manifestUri,
                baseUrl: baseURL,
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

func parseIso8601DurationSeconds(_ value: String?) -> Double? {
    guard let value, value.hasPrefix("PT") else {
        return nil
    }
    var number = ""
    var total = 0.0
    for character in value.dropFirst(2) {
        if character.isNumber || character == "." {
            number.append(character)
            continue
        }
        guard let parsed = Double(number) else {
            return nil
        }
        number = ""
        switch character {
        case "H":
            total += parsed * 3600
        case "M":
            total += parsed * 60
        case "S":
            total += parsed
        default:
            return nil
        }
    }
    return total > 0 ? total : nil
}

func xmlAttr(_ input: String, tag: String, attr: String) -> String? {
    xmlOpenTag(input, tag: tag).flatMap { xmlAttrFromTag($0, attr: attr) }
}

private func xmlOpenTag(_ input: String, tag: String) -> String? {
    guard let start = input.range(of: "<\(tag)") else {
        return nil
    }
    guard let end = input[start.lowerBound...].firstIndex(of: ">") else {
        return nil
    }
    return String(input[start.lowerBound...end])
}

private func xmlAttrFromTag(_ tag: String, attr: String) -> String? {
    guard let attrRange = tag.range(of: "\(attr)=") else {
        return nil
    }
    let valueStartCandidate = attrRange.upperBound
    guard valueStartCandidate < tag.endIndex else {
        return nil
    }
    let quote = tag[valueStartCandidate]
    guard quote == "\"" || quote == "'" else {
        return nil
    }
    let valueStart = tag.index(after: valueStartCandidate)
    guard let valueEnd = tag[valueStart...].firstIndex(of: quote) else {
        return nil
    }
    return String(tag[valueStart..<valueEnd])
}

private func xmlBlocks(_ input: String, tag: String) -> [String] {
    var blocks: [String] = []
    var searchStart = input.startIndex
    let open = "<\(tag)"
    let close = "</\(tag)>"
    while let start = input[searchStart...].range(of: open)?.lowerBound {
        let candidate = input[start...]
        if let closeRange = candidate.range(of: close) {
            blocks.append(String(input[start..<closeRange.upperBound]))
            searchStart = closeRange.upperBound
        } else if let selfCloseRange = candidate.range(of: "/>") {
            blocks.append(String(input[start..<selfCloseRange.upperBound]))
            searchStart = selfCloseRange.upperBound
        } else {
            break
        }
    }
    return blocks
}

private func xmlText(_ input: String, tag: String) -> String? {
    guard let openStart = input.range(of: "<\(tag)")?.lowerBound else {
        return nil
    }
    guard let openEnd = input[openStart...].firstIndex(of: ">") else {
        return nil
    }
    let bodyStart = input.index(after: openEnd)
    guard let closeStart = input[bodyStart...].range(of: "</\(tag)>")?.lowerBound else {
        return nil
    }
    return String(input[bodyStart..<closeStart]).trimmingCharacters(in: .whitespacesAndNewlines)
}

private func directXmlText(_ input: String, tag: String, before childTags: [String]) -> String? {
    let upperBound = childTags
        .compactMap { input.range(of: "<\($0)")?.lowerBound }
        .min() ?? input.endIndex
    return xmlText(String(input[..<upperBound]), tag: tag)
}

private func prefixBeforeTag(_ input: String, tag: String) -> String {
    guard let end = input.range(of: "<\(tag)")?.lowerBound else {
        return input
    }
    return String(input[..<end])
}

func parseFlvClipManifest(baseUri: String, manifestText: String) -> [String] {
    nonEmptyTrimmedLines(manifestText).compactMap { line in
        if line.hasPrefix("#") || line.caseInsensitiveCompare("ffconcat version 1.0") == .orderedSame {
            return nil
        }
        let rawUri: String
        if valueAfterPrefix("file ", in: line) != nil {
            rawUri = line.dropFirst("file ".count)
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
        } else {
            rawUri = line
        }
        return rawUri.isEmpty ? nil : resolveRemoteReference(baseUri: baseUri, reference: rawUri)
    }
}

func resolveRemoteReference(baseUri: String, reference: String) -> String {
    let trimmedReference = reference.trimmingCharacters(in: .whitespacesAndNewlines)
    if let url = URL(string: trimmedReference), url.scheme != nil {
        return url.absoluteString
    }
    if let baseURL = URL(string: baseUri),
       let resolved = URL(string: trimmedReference, relativeTo: baseURL)?.absoluteURL {
        return resolved.absoluteString
    }
    return trimmedReference
}

func extensionFromUri(_ uri: String, fallback: String) -> String {
    let withoutFragment = uri.components(separatedBy: "#").first ?? uri
    let path = withoutFragment.components(separatedBy: "?").first ?? withoutFragment
    let name = path.components(separatedBy: "/").last ?? ""
    let parts = name.split(separator: ".", omittingEmptySubsequences: false)
    guard
        parts.count > 1,
        let rawExtension = parts.last,
        !rawExtension.isEmpty,
        rawExtension.allSatisfy({ $0.isLetter || $0.isNumber })
    else {
        return fallback
    }
    return String(rawExtension)
}

private func escapeXml(_ value: String) -> String {
    value
        .replacingOccurrences(of: "&", with: "&amp;")
        .replacingOccurrences(of: "\"", with: "&quot;")
        .replacingOccurrences(of: "<", with: "&lt;")
        .replacingOccurrences(of: ">", with: "&gt;")
}

func escapeFfconcatPath(_ path: String) -> String {
    path.replacingOccurrences(of: "'", with: "'\\''")
}

private func splitQuoted(_ input: String, delimiter: Character) -> [String] {
    var result: [String] = []
    var start = input.startIndex
    var index = input.startIndex
    var inQuotes = false
    while index < input.endIndex {
        let character = input[index]
        if character == "\"" {
            inQuotes.toggle()
        } else if character == delimiter, !inQuotes {
            result.append(String(input[start..<index]).trimmingCharacters(in: .whitespacesAndNewlines))
            start = input.index(after: index)
        }
        index = input.index(after: index)
    }
    result.append(String(input[start...]).trimmingCharacters(in: .whitespacesAndNewlines))
    return result
}

private func valueAfterPrefix(_ prefix: String, in line: String) -> String? {
    guard let range = line.range(of: prefix, options: [.caseInsensitive, .anchored]) else {
        return nil
    }
    return String(line[range.upperBound...])
}

func nonEmptyTrimmedLines(_ text: String) -> [String] {
    text.components(separatedBy: .newlines)
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
}

func hlsByteRangeKey(uri: String, byteRange: VesperDownloadByteRange?) -> String {
    guard let byteRange else {
        return "\(uri):none"
    }
    return "\(uri):\(byteRange.offset):\(byteRange.length)"
}

func padded(_ value: UInt64, width: Int) -> String {
    let text = String(value)
    guard text.count < width else {
        return text
    }
    return String(repeating: "0", count: width - text.count) + text
}
