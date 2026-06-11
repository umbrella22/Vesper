import Foundation
func splitQuoted(_ input: String, delimiter: Character) -> [String] {
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

func valueAfterPrefix(_ prefix: String, in line: String) -> String? {
    guard let range = line.range(of: prefix, options: [.caseInsensitive, .anchored]) else {
        return nil
    }
    return String(line[range.upperBound...])
}

