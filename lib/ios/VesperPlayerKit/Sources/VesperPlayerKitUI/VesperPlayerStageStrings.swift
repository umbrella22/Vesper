import Foundation

enum VesperPlayerStageStrings {
    static let auto = "自动"
    static let quality = "画质"
    static let qualityButtonCapped = "受限"
    static let qualityButtonPinned = "锁定"
    static let qualityButtonLocking = "锁定中"
    static let stageVideoOnDemand = "点播视频"
    static let stageLiveStream = "直播流"
    static let stageLiveWithDvrWindow = "带 DVR 窗口的直播"
    static let goLive = "回到直播"
    static let live = "直播"
    static let buffering = "缓冲中"
    static let play = "播放"
    static let pause = "暂停"

    static func liveBehind(_ time: String) -> String {
        "直播 -\(time)"
    }

    static func liveEdge(_ time: String) -> String {
        "直播 · 实时点 \(time)"
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
