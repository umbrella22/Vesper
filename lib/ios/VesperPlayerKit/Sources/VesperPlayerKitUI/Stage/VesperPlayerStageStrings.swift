import Foundation

enum VesperPlayerStageStrings {
    static let auto = "Auto"
    static let quality = "Quality"
    static let qualityButtonCapped = "Capped"
    static let qualityButtonPinned = "Pinned"
    static let qualityButtonLocking = "Locking"
    static let stageVideoOnDemand = "Video on demand"
    static let stageLiveStream = "Live stream"
    static let stageLiveWithDvrWindow = "Live with DVR"
    static let goLive = "Go live"
    static let live = "Live"
    static let buffering = "Buffering"
    static let play = "Play"
    static let pause = "Pause"

    static func liveBehind(_ time: String) -> String {
        "Live -\(time)"
    }

    static func liveEdge(_ time: String) -> String {
        "Live edge \(time)"
    }

    static func bitRateMbps(_ value: Double) -> String {
        String(format: "%.1f Mbps", value)
    }

    static func bitRateKbps(_ value: Double) -> String {
        String(format: "%.0f Kbps", value)
    }

    static func bitRateBps(_ value: Int64) -> String {
        "\(value) bps"
    }

    static func playbackRate(_ value: Double) -> String {
        String(format: "%.1fx", value)
    }
}
