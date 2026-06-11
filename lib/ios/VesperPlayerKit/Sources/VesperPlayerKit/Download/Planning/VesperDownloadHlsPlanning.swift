import Foundation
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
