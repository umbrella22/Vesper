import Foundation

public enum VesperTrackSelectionMode: String, Equatable {
    case auto
    case disabled
    case track
}

public struct VesperTrackSelection: Equatable {
    public let mode: VesperTrackSelectionMode
    public let trackId: String?

    public init(mode: VesperTrackSelectionMode, trackId: String? = nil) {
        self.mode = mode
        self.trackId = trackId
    }

    public static func auto() -> VesperTrackSelection {
        VesperTrackSelection(mode: .auto)
    }

    public static func disabled() -> VesperTrackSelection {
        VesperTrackSelection(mode: .disabled)
    }

    public static func track(_ trackId: String) -> VesperTrackSelection {
        VesperTrackSelection(mode: .track, trackId: trackId)
    }
}

public enum VesperAbrMode: String, Equatable {
    case auto
    case constrained
    case fixedTrack
}

public struct VesperAbrPolicy: Equatable {
    public let mode: VesperAbrMode
    public let trackId: String?
    public let maxBitRate: Int64?
    public let maxWidth: Int?
    public let maxHeight: Int?

    public init(
        mode: VesperAbrMode,
        trackId: String? = nil,
        maxBitRate: Int64? = nil,
        maxWidth: Int? = nil,
        maxHeight: Int? = nil,
    ) {
        self.mode = mode
        self.trackId = trackId
        self.maxBitRate = maxBitRate
        self.maxWidth = maxWidth
        self.maxHeight = maxHeight
    }

    public static func auto() -> VesperAbrPolicy {
        VesperAbrPolicy(mode: .auto)
    }

    public static func constrained(
        maxBitRate: Int64? = nil,
        maxWidth: Int? = nil,
        maxHeight: Int? = nil,
    ) -> VesperAbrPolicy {
        VesperAbrPolicy(
            mode: .constrained,
            maxBitRate: maxBitRate,
            maxWidth: maxWidth,
            maxHeight: maxHeight,
        )
    }

    public static func fixedTrack(_ trackId: String) -> VesperAbrPolicy {
        VesperAbrPolicy(mode: .fixedTrack, trackId: trackId)
    }
}

public struct VesperTrackSelectionSnapshot: Equatable {
    public let video: VesperTrackSelection
    public let audio: VesperTrackSelection
    public let subtitle: VesperTrackSelection
    public let abrPolicy: VesperAbrPolicy

    public init(
        video: VesperTrackSelection = .auto(),
        audio: VesperTrackSelection = .auto(),
        subtitle: VesperTrackSelection = .disabled(),
        abrPolicy: VesperAbrPolicy = .auto(),
    ) {
        self.video = video
        self.audio = audio
        self.subtitle = subtitle
        self.abrPolicy = abrPolicy
    }
}
