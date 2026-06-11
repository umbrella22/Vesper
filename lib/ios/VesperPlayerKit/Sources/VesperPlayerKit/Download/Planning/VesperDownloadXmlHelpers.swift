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
    return String(tag[valueStart..<valueEnd])
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
    return String(input[bodyStart..<closeStart]).trimmingCharacters(in: .whitespacesAndNewlines)
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
