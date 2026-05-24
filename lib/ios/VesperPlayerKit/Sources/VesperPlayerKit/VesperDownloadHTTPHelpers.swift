import Foundation

func sanitizedDownloadHttpHeaders(_ headers: [String: String]) -> [String: String] {
    var result: [String: String] = [:]
    for (name, value) in headers {
        let sanitizedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if !sanitizedName.isEmpty,
           !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            result[sanitizedName] = value
        }
    }
    return result
}


func rejectInsecureHTTPURL(_ url: URL) throws {
    guard url.scheme?.lowercased() == "http" else {
        return
    }
    throw VesperForegroundDownloadPreparationError.invalidSource(
        "\(vesperDownloadATSFailureMessage) URL: \(url.absoluteString)"
    )
}

func closeDownloadFileHandle(_ handle: FileHandle, context: String) {
    do {
        try handle.close()
    } catch {
        iosHostLog("failed to close \(context) file handle: \(error.localizedDescription)")
    }
}


extension URLRequest {
    mutating func applyDownloadHttpHeaders(_ headers: [String: String]) {
        for (name, value) in sanitizedDownloadHttpHeaders(headers) {
            setValue(value, forHTTPHeaderField: name)
        }
        if value(forHTTPHeaderField: "Accept-Encoding") == nil {
            setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        }
    }
}

