import Foundation

func encodeRuntimeUInt32(_ value: Int?, field: String) -> UInt32 {
    guard let value else { return 0 }
    let encoded = UInt32(clamping: value)
    if Int(encoded) != value {
        iosHostLog("runtime bridge clamped \(field) from \(value) to \(encoded)")
    }
    return encoded
}

func encodeRuntimeInt32(_ value: Int?, field: String) -> Int32 {
    guard let value else { return 0 }
    let encoded = Int32(clamping: max(0, value))
    if Int(encoded) != value {
        iosHostLog("runtime bridge clamped \(field) from \(value) to \(encoded)")
    }
    return encoded
}
