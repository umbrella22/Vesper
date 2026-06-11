@preconcurrency import AVFoundation

extension CMTime {
    init(milliseconds: Int64) {
        self = CMTime(seconds: Double(milliseconds) / 1000.0, preferredTimescale: 600)
    }

    var milliseconds: Int64 {
        guard isValid, isNumeric, seconds.isFinite else {
            return 0
        }
        return max(Int64(seconds * 1000.0), 0)
    }

    var finiteMilliseconds: Int64? {
        guard isValid, isNumeric, seconds.isFinite else {
            return nil
        }
        return max(Int64(seconds * 1000.0), 0)
    }
}
