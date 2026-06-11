import Foundation

func fourCharCodeString(_ value: UInt32) -> String {
    let scalarValues = [
        UInt8((value >> 24) & 0xFF),
        UInt8((value >> 16) & 0xFF),
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
    let printable = scalarValues.allSatisfy { (0x20 ... 0x7E).contains($0) }
    guard printable else {
        return String(format: "0x%08X", value)
    }
    return String(bytes: scalarValues, encoding: .ascii) ?? String(format: "0x%08X", value)
}
