import Foundation
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

func escapeXml(_ value: String) -> String {
    value
        .replacingOccurrences(of: "&", with: "&amp;")
        .replacingOccurrences(of: "\"", with: "&quot;")
        .replacingOccurrences(of: "<", with: "&lt;")
        .replacingOccurrences(of: ">", with: "&gt;")
}

func escapeFfconcatPath(_ path: String) -> String {
    path.replacingOccurrences(of: "'", with: "'\\''")
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
