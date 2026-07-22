import AVFoundation
import CoreGraphics
import Foundation
import SwiftUI
import UIKit
internal import VesperPlayerKitBridgeShim
public enum VesperBackgroundPlaybackMode: String, Equatable {
    case disabled
    case continueAudio
}

public enum VesperSystemPlaybackPermissionStatus: String, Equatable {
    case notRequired
    case granted
    case denied
}

public enum VesperSystemPlaybackControlKind: String, Equatable {
    case playPause
    case seekBack
    case seekForward
}

public struct VesperSystemPlaybackMetadata: Equatable {
    public let title: String
    public let artist: String?
    public let albumTitle: String?
    public let artworkUri: String?
    public let contentUri: String?
    public let durationMs: Int64?
    public let isLive: Bool

    public init(
        title: String,
        artist: String? = nil,
        albumTitle: String? = nil,
        artworkUri: String? = nil,
        contentUri: String? = nil,
        durationMs: Int64? = nil,
        isLive: Bool = false
    ) {
        self.title = title
        self.artist = artist
        self.albumTitle = albumTitle
        self.artworkUri = artworkUri
        self.contentUri = contentUri
        self.durationMs = durationMs
        self.isLive = isLive
    }
}

public struct VesperSystemPlaybackControlButton: Equatable {
    public let kind: VesperSystemPlaybackControlKind
    public let seekOffsetMs: Int64?

    public init(kind: VesperSystemPlaybackControlKind, seekOffsetMs: Int64? = nil) {
        self.kind = kind
        self.seekOffsetMs = seekOffsetMs
    }

    public static func playPause() -> VesperSystemPlaybackControlButton {
        VesperSystemPlaybackControlButton(kind: .playPause)
    }

    public static func seekBack(_ offsetMs: Int64 = 10_000) -> VesperSystemPlaybackControlButton {
        VesperSystemPlaybackControlButton(kind: .seekBack, seekOffsetMs: offsetMs)
    }

    public static func seekForward(
        _ offsetMs: Int64 = 10_000
    ) -> VesperSystemPlaybackControlButton {
        VesperSystemPlaybackControlButton(kind: .seekForward, seekOffsetMs: offsetMs)
    }

    public func normalized() -> VesperSystemPlaybackControlButton {
        switch kind {
        case .playPause:
            return .playPause()
        case .seekBack:
            return .seekBack(normalizedSeekOffsetMs)
        case .seekForward:
            return .seekForward(normalizedSeekOffsetMs)
        }
    }

    public var normalizedSeekOffsetMs: Int64 {
        min(
            max(
                seekOffsetMs ?? defaultSystemPlaybackSeekOffsetMs,
                minSystemPlaybackSeekOffsetMs
            ),
            maxSystemPlaybackSeekOffsetMs
        )
    }
}

public struct VesperSystemPlaybackControls: Equatable {
    public let compactButtons: [VesperSystemPlaybackControlButton]

    public init(
        compactButtons: [VesperSystemPlaybackControlButton] = [
            .seekBack(),
            .playPause(),
            .seekForward(),
        ]
    ) {
        self.compactButtons = compactButtons
    }

    public static func videoDefault() -> VesperSystemPlaybackControls {
        VesperSystemPlaybackControls(compactButtons: videoDefaultButtons())
    }

    public func normalized(showSeekActions: Bool = true) -> VesperSystemPlaybackControls {
        var buttons = Array(compactButtons.prefix(maxSystemPlaybackCompactButtons))
            .map { $0.normalized() }

        if buttons.isEmpty {
            buttons = Self.videoDefaultButtons().map { $0.normalized() }
        }
        if buttons.count == maxSystemPlaybackCompactButtons, buttons[1].kind != .playPause {
            buttons[1] = .playPause()
        }
        if !buttons.contains(where: { $0.kind == .playPause }) {
            buttons = Self.videoDefaultButtons().map { $0.normalized() }
        }
        if !showSeekActions {
            buttons.removeAll { $0.kind == .seekBack || $0.kind == .seekForward }
            if buttons.isEmpty {
                buttons.append(.playPause())
            }
        }

        return VesperSystemPlaybackControls(compactButtons: buttons)
    }

    public func seekOffsetMs(for kind: VesperSystemPlaybackControlKind) -> Int64? {
        compactButtons.first { $0.kind == kind }?.normalizedSeekOffsetMs
    }

    private static func videoDefaultButtons() -> [VesperSystemPlaybackControlButton] {
        [
            .seekBack(),
            .playPause(),
            .seekForward(),
        ]
    }
}

public struct VesperSystemPlaybackConfiguration: Equatable {
    public let enabled: Bool
    public let backgroundMode: VesperBackgroundPlaybackMode
    public let showSystemControls: Bool
    public let showSeekActions: Bool
    public let metadata: VesperSystemPlaybackMetadata?
    public let controls: VesperSystemPlaybackControls

    public init(
        enabled: Bool = true,
        backgroundMode: VesperBackgroundPlaybackMode = .continueAudio,
        showSystemControls: Bool = true,
        showSeekActions: Bool = true,
        metadata: VesperSystemPlaybackMetadata? = nil,
        controls: VesperSystemPlaybackControls = .videoDefault()
    ) {
        self.enabled = enabled
        self.backgroundMode = backgroundMode
        self.showSystemControls = showSystemControls
        self.showSeekActions = showSeekActions
        self.metadata = metadata
        self.controls = controls
    }
}

private let defaultSystemPlaybackSeekOffsetMs: Int64 = 10_000
private let minSystemPlaybackSeekOffsetMs: Int64 = 1_000
private let maxSystemPlaybackSeekOffsetMs: Int64 = 60_000
private let maxSystemPlaybackCompactButtons = 3
