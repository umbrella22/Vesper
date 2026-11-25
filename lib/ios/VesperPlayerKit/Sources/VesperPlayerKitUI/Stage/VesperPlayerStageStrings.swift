import Foundation
import SwiftUI

/// User-facing text rendered by `VesperPlayerStage` and its standalone controls.
///
/// The default value, ``localized(locale:)``, is loaded from the
/// `VesperPlayerKitUI` resource bundle and follows the system language. The SDK
/// ships English and Simplified Chinese. Hosts can start from the localized
/// value and override individual fields, or build a complete value with
/// ``init(auto:quality:qualityCapped:qualityPinned:qualityLocking:videoOnDemand:liveStream:liveWithDvrWindow:goLive:live:liveBehindFormat:liveEdgeFormat:buffering:play:pause:playbackPosition:fullscreen:exitFullscreen:navigateBack:more:)``.
///
/// Format fields contain a single `%@` placeholder that is replaced with the
/// formatted time. Strings without a placeholder are rendered unchanged.
public struct VesperPlayerStageStrings: Equatable, Sendable {
    public var auto: String
    public var quality: String
    public var qualityCapped: String
    public var qualityPinned: String
    public var qualityLocking: String
    public var videoOnDemand: String
    public var liveStream: String
    public var liveWithDvrWindow: String
    public var goLive: String
    public var live: String
    /// Live button text when playback is behind the live edge, such as `LIVE -%@`.
    public var liveBehindFormat: String
    /// Timeline summary for live streams with a known edge, such as `LIVE · Edge %@`.
    public var liveEdgeFormat: String
    public var buffering: String
    public var play: String
    public var pause: String
    public var playbackPosition: String
    public var fullscreen: String
    public var exitFullscreen: String
    public var navigateBack: String
    public var more: String

    public init(
        auto: String,
        quality: String,
        qualityCapped: String,
        qualityPinned: String,
        qualityLocking: String,
        videoOnDemand: String,
        liveStream: String,
        liveWithDvrWindow: String,
        goLive: String,
        live: String,
        liveBehindFormat: String,
        liveEdgeFormat: String,
        buffering: String,
        play: String,
        pause: String,
        playbackPosition: String,
        fullscreen: String,
        exitFullscreen: String,
        navigateBack: String,
        more: String
    ) {
        self.auto = auto
        self.quality = quality
        self.qualityCapped = qualityCapped
        self.qualityPinned = qualityPinned
        self.qualityLocking = qualityLocking
        self.videoOnDemand = videoOnDemand
        self.liveStream = liveStream
        self.liveWithDvrWindow = liveWithDvrWindow
        self.goLive = goLive
        self.live = live
        self.liveBehindFormat = liveBehindFormat
        self.liveEdgeFormat = liveEdgeFormat
        self.buffering = buffering
        self.play = play
        self.pause = pause
        self.playbackPosition = playbackPosition
        self.fullscreen = fullscreen
        self.exitFullscreen = exitFullscreen
        self.navigateBack = navigateBack
        self.more = more
    }

    public func liveBehind(_ time: String) -> String {
        Self.fill(liveBehindFormat, time)
    }

    public func liveEdge(_ time: String) -> String {
        Self.fill(liveEdgeFormat, time)
    }

    /// Replaces `%@` without `String(format:)`, so host-provided text with other
    /// format specifiers cannot crash rendering.
    private static func fill(_ format: String, _ value: String) -> String {
        format.replacingOccurrences(of: "%@", with: value)
    }

    // Locale-neutral unit formatting shared by Stage labels.

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
