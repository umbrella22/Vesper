import Foundation

func isExpiredHttpStatus(_ statusCode: Int) -> Bool {
    statusCode == 401 || statusCode == 403 || statusCode == 404 || statusCode == 410
}

struct VesperHTTPContentRange: Equatable {
    let start: UInt64?
    let end: UInt64?
    let total: UInt64?

    var isUnsatisfied: Bool {
        start == nil && end == nil
    }

    var length: UInt64? {
        guard let start, let end, end >= start else {
            return nil
        }
        return end - start + 1
    }
}

func parseHttpContentRange(_ contentRange: String?) -> VesperHTTPContentRange? {
    guard let contentRange else {
        return nil
    }
    let fields = contentRange.trimmingCharacters(in: .whitespacesAndNewlines)
        .split(separator: " ", maxSplits: 1)
    guard fields.count == 2,
          fields[0].lowercased() == "bytes"
    else {
        return nil
    }
    let value = fields[1]
    let rangeAndTotal = value.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false)
    guard rangeAndTotal.count == 2 else { return nil }
    let totalText = rangeAndTotal[1].trimmingCharacters(in: .whitespaces)
    let total = totalText == "*" ? nil : UInt64(totalText)
    if value.hasPrefix("*") {
        guard value.hasPrefix("*/") || rangeAndTotal[0] == "*" else { return nil }
        return VesperHTTPContentRange(start: nil, end: nil, total: total)
    }

    let rangeParts = rangeAndTotal[0].split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false)
    guard rangeParts.count == 2,
          let start = UInt64(rangeParts[0].trimmingCharacters(in: .whitespaces)),
          let end = UInt64(rangeParts[1].trimmingCharacters(in: .whitespaces)),
          end >= start
    else {
        return nil
    }
    return VesperHTTPContentRange(start: start, end: end, total: total)
}

func parseHttpContentLength(_ contentLength: String?) -> UInt64? {
    guard let contentLength else { return nil }
    return UInt64(contentLength.trimmingCharacters(in: .whitespacesAndNewlines))
}

@discardableResult
func validateHTTPPartialContentRange(
    contentRangeHeader: String?,
    contentLengthHeader: String?,
    requestedStart: UInt64,
    requestedEndInclusive: UInt64?,
    expectedBodyLength: UInt64?,
    expectedTotalSizeBytes: UInt64?,
    sourceDescription: String
) throws -> VesperHTTPContentRange {
    guard let contentRange = parseHttpContentRange(contentRangeHeader),
          !contentRange.isUnsatisfied,
          contentRange.start == requestedStart,
          let responseEnd = contentRange.end
    else {
        throw VesperForegroundDownloadPreparationError.invalidSource(
            "remote server returned an unexpected Content-Range for \(sourceDescription)"
        )
    }
    if let requestedEndInclusive, responseEnd != requestedEndInclusive {
        throw VesperForegroundDownloadPreparationError.invalidSource(
            "remote server returned a Content-Range outside the requested byte range for \(sourceDescription)"
        )
    }
    if let expectedTotalSizeBytes,
       let total = contentRange.total,
       total != expectedTotalSizeBytes {
        throw VesperForegroundDownloadPreparationError.invalidSource(
            "remote server reported Content-Range total \(total), expected \(expectedTotalSizeBytes) for \(sourceDescription)"
        )
    }
    if let length = contentRange.length {
        if let expectedBodyLength, length != expectedBodyLength {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "remote server returned \(length) range bytes, expected \(expectedBodyLength) for \(sourceDescription)"
            )
        }
        if let contentLength = parseHttpContentLength(contentLengthHeader),
           contentLength != length {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "remote server reported Content-Length \(contentLength), expected \(length) from Content-Range for \(sourceDescription)"
            )
        }
    }
    return contentRange
}

