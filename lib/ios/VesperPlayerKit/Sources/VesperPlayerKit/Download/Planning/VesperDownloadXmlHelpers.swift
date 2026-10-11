import Foundation
func xmlAttr(_ input: String, tag: String, attr: String) -> String? {
    xmlOpenTag(input, tag: tag).flatMap { xmlAttrFromTag($0, attr: attr) }
}

func xmlOpenTag(_ input: String, tag: String) -> String? {
    guard let start = input.range(of: "<\(tag)") else {
        return nil
    }
    guard let end = input[start.lowerBound...].firstIndex(of: ">") else {
        return nil
    }
    return String(input[start.lowerBound...end])
}

func xmlAttrFromTag(_ tag: String, attr: String) -> String? {
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
    return decodeXmlReferences(String(tag[valueStart..<valueEnd]))
}

func xmlBlocks(_ input: String, tag: String) -> [String] {
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

func xmlText(_ input: String, tag: String) -> String? {
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
    return decodeXmlReferences(String(input[bodyStart..<closeStart]).trimmingCharacters(in: .whitespacesAndNewlines))
}

func directXmlText(_ input: String, tag: String, before childTags: [String]) -> String? {
    let upperBound = childTags
        .compactMap { input.range(of: "<\($0)")?.lowerBound }
        .min() ?? input.endIndex
    return xmlText(String(input[..<upperBound]), tag: tag)
}

func prefixBeforeTag(_ input: String, tag: String) -> String {
    guard let end = input.range(of: "<\(tag)")?.lowerBound else {
        return input
    }
    return String(input[..<end])
}

// Decode one XML layer. Replacing entities repeatedly would change signed URLs
// containing a literal encoded ampersand (for example, &amp;amp;).
func decodeXmlReferences(_ value: String) -> String? {
    var result = ""
    var cursor = value.startIndex
    while let amp = value[cursor...].firstIndex(of: "&") {
        result += value[cursor..<amp]
        guard let end = value[amp...].firstIndex(of: ";") else { return nil }
        let entity = String(value[value.index(after: amp)..<end])
        let decoded: String
        switch entity {
        case "amp": decoded = "&"
        case "lt": decoded = "<"
        case "gt": decoded = ">"
        case "apos": decoded = "'"
        case "quot": decoded = "\""
        default:
            let scalarValue: UInt32?
            if entity.hasPrefix("#x") {
                scalarValue = UInt32(entity.dropFirst(2), radix: 16)
            } else if entity.hasPrefix("#") {
                scalarValue = UInt32(entity.dropFirst())
            } else { return nil }
            guard let scalarValue, let scalar = UnicodeScalar(scalarValue),
                  scalarValue == 9 || scalarValue == 10 || scalarValue == 13
                    || (0x20...0xD7FF).contains(scalarValue)
                    || (0xE000...0xFFFD).contains(scalarValue)
                    || (0x10000...0x10FFFF).contains(scalarValue) else { return nil }
            decoded = String(scalar)
        }
        result += decoded
        cursor = value.index(after: end)
    }
    result += value[cursor...]
    return result
}
