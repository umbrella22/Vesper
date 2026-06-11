import Foundation
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
