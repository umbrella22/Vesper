import UIKit
import VesperPlayerKit

extension PlaybackStateUi {
    func toWireName() -> String {
        switch self {
        case .ready:
            "ready"
        case .playing:
            "playing"
        case .paused:
            "paused"
        case .finished:
            "finished"
        }
    }
}

extension VesperSystemPlaybackPermissionStatus {
    func toWireName() -> String {
        switch self {
        case .notRequired:
            "notRequired"
        case .granted:
            "granted"
        case .denied:
            "denied"
        }
    }
}

extension TimelineKindUi {
    func toWireName() -> String {
        switch self {
        case .vod:
            "vod"
        case .live:
            "live"
        case .liveDvr:
            "liveDvr"
        }
    }
}

extension PlayerBridgeBackend {
    func toBackendFamilyWireName() -> String {
        switch self {
        case .fakeDemo:
            "fakeDemo"
        case .rustNativeStub:
            "iosHostKit"
        }
    }
}

extension VesperPlayerSource {
    func validatedForIosBackend() throws -> VesperPlayerSource {
        return self
    }
}

extension VesperMediaTrackKind {
    func toWireName() -> String {
        switch self {
        case .video:
            "video"
        case .audio:
            "audio"
        case .subtitle:
            "subtitle"
        }
    }
}

extension VesperTrackSelectionMode {
    func toWireName() -> String {
        switch self {
        case .auto:
            "auto"
        case .disabled:
            "disabled"
        case .track:
            "track"
        }
    }
}

extension VesperAbrMode {
    func toWireName() -> String {
        switch self {
        case .auto:
            "auto"
        case .constrained:
            "constrained"
        case .fixedTrack:
            "fixedTrack"
        }
    }
}

extension VesperFixedTrackStatus {
    func toWireName() -> String {
        switch self {
        case .pending:
            "pending"
        case .locked:
            "locked"
        case .fallback:
            "fallback"
        }
    }
}

extension VesperBufferingPreset {
    func toWireName() -> String {
        switch self {
        case .default:
            "defaultPreset"
        case .balanced:
            "balanced"
        case .streaming:
            "streaming"
        case .resilient:
            "resilient"
        case .lowLatency:
            "lowLatency"
        }
    }
}

extension VesperRetryBackoff {
    func toWireName() -> String {
        switch self {
        case .fixed:
            "fixed"
        case .linear:
            "linear"
        case .exponential:
            "exponential"
        }
    }
}

extension VesperCachePreset {
    func toWireName() -> String {
        switch self {
        case .default:
            "defaultPreset"
        case .disabled:
            "disabled"
        case .streaming:
            "streaming"
        case .resilient:
            "resilient"
        }
    }
}

extension VesperDownloadState {
    func toWireName() -> String {
        switch self {
        case .queued:
            "queued"
        case .preparing:
            "preparing"
        case .downloading:
            "downloading"
        case .paused:
            "paused"
        case .completed:
            "completed"
        case .failed:
            "failed"
        case .removed:
            "removed"
        }
    }
}

extension VesperDownloadContentFormat {
    func toWireName() -> String {
        switch self {
        case .hlsSegments:
            "hlsSegments"
        case .dashSegments:
            "dashSegments"
        case .flvSegments:
            "flvSegments"
        case .singleFile:
            "singleFile"
        case .unknown:
            "unknown"
        }
    }
}

extension VesperDownloadOutputFormat {
    func toWireName() -> String {
        switch self {
        case .mp4:
            "mp4"
        case .mkv:
            "mkv"
        case .original:
            "original"
        }
    }
}

extension VesperDownloadStreamKind {
    func toWireName() -> String {
        switch self {
        case .combined:
            "combined"
        case .video:
            "video"
        case .audio:
            "audio"
        case .secondaryAudio:
            "secondaryAudio"
        case .subtitle:
            "subtitle"
        case .auxiliary:
            "auxiliary"
        }
    }
}

extension UIColor {
    convenience init(argb: UInt32) {
        let alpha = CGFloat((argb >> 24) & 0xff) / 255.0
        let red = CGFloat((argb >> 16) & 0xff) / 255.0
        let green = CGFloat((argb >> 8) & 0xff) / 255.0
        let blue = CGFloat(argb & 0xff) / 255.0
        self.init(red: red, green: green, blue: blue, alpha: alpha)
    }
}

let methodChannelName = "io.github.ikaros.vesper_player"
let eventChannelName = "io.github.ikaros.vesper_player/events"
let downloadEventChannelName = "io.github.ikaros.vesper_player/download_events"
let playerViewType = "io.github.ikaros.vesper_player/platform_view"
let airPlayRouteButtonViewType = "io.github.ikaros.vesper_player/airplay_route_button"
