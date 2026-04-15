import Foundation
import VesperPlayerKitBridgeShim

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

public struct VesperTrackPreferencePolicy: Equatable {
    public let preferredAudioLanguage: String?
    public let preferredSubtitleLanguage: String?
    public let selectSubtitlesByDefault: Bool
    public let selectUndeterminedSubtitleLanguage: Bool
    public let audioSelection: VesperTrackSelection
    public let subtitleSelection: VesperTrackSelection
    public let abrPolicy: VesperAbrPolicy

    public init(
        preferredAudioLanguage: String? = nil,
        preferredSubtitleLanguage: String? = nil,
        selectSubtitlesByDefault: Bool = false,
        selectUndeterminedSubtitleLanguage: Bool = false,
        audioSelection: VesperTrackSelection = .auto(),
        subtitleSelection: VesperTrackSelection = .disabled(),
        abrPolicy: VesperAbrPolicy = .auto()
    ) {
        self.preferredAudioLanguage = preferredAudioLanguage
        self.preferredSubtitleLanguage = preferredSubtitleLanguage
        self.selectSubtitlesByDefault = selectSubtitlesByDefault
        self.selectUndeterminedSubtitleLanguage = selectUndeterminedSubtitleLanguage
        self.audioSelection = audioSelection
        self.subtitleSelection = subtitleSelection
        self.abrPolicy = abrPolicy
    }
}

extension VesperTrackSelection {
    fileprivate static func fromRuntimeBridgePayload(
        _ payload: VesperRuntimeTrackSelection
    ) -> VesperTrackSelection {
        let mode = VesperTrackSelectionMode(runtimeBridgeOrdinal: payload.mode_ordinal)
        let trackId = optionalRuntimeBridgeString(payload.track_id)
        switch mode {
        case .auto:
            return .auto()
        case .disabled:
            return .disabled()
        case .track:
            return trackId.map(VesperTrackSelection.track) ?? .auto()
        }
    }
}

extension VesperTrackSelectionMode {
    fileprivate init(runtimeBridgeOrdinal: Int32) {
        self =
            switch runtimeBridgeOrdinal {
            case 1: .disabled
            case 2: .track
            default: .auto
            }
    }
}

extension VesperAbrMode {
    fileprivate init(runtimeBridgeOrdinal: Int32) {
        self =
            switch runtimeBridgeOrdinal {
            case 1: .constrained
            case 2: .fixedTrack
            default: .auto
            }
    }
}

extension VesperTrackPreferencePolicy {
    func resolvedForRuntime() -> VesperTrackPreferencePolicy {
        VesperRuntimeTrackPreferenceResolver.resolve(self)
    }
}

private enum VesperRuntimeTrackPreferenceResolver {
    static func resolve(
        _ policy: VesperTrackPreferencePolicy
    ) -> VesperTrackPreferencePolicy {
        withRuntimeBridgeTrackPreferencePolicy(policy) { payload in
            var resolved = VesperRuntimeTrackPreferencePolicy()
            let didResolve = vesper_runtime_resolve_track_preferences(payload, &resolved)
            guard didResolve else {
                preconditionFailure(
                    "VesperPlayerKit requires the linked Rust track preference resolver on iOS."
                )
            }
            defer { vesper_runtime_track_preferences_free(&resolved) }
            return VesperTrackPreferencePolicy(
                preferredAudioLanguage: optionalRuntimeBridgeString(
                    resolved.preferred_audio_language
                ),
                preferredSubtitleLanguage: optionalRuntimeBridgeString(
                    resolved.preferred_subtitle_language
                ),
                selectSubtitlesByDefault: resolved.select_subtitles_by_default,
                selectUndeterminedSubtitleLanguage: resolved
                    .select_undetermined_subtitle_language,
                audioSelection: .fromRuntimeBridgePayload(resolved.audio_selection),
                subtitleSelection: .fromRuntimeBridgePayload(resolved.subtitle_selection),
                abrPolicy: VesperAbrPolicy(
                    mode: VesperAbrMode(runtimeBridgeOrdinal: resolved.abr_policy.mode_ordinal),
                    trackId: optionalRuntimeBridgeString(resolved.abr_policy.track_id),
                    maxBitRate: resolved.abr_policy.has_max_bit_rate
                        ? resolved.abr_policy.max_bit_rate
                        : nil,
                    maxWidth: resolved.abr_policy.has_max_width
                        ? Int(resolved.abr_policy.max_width)
                        : nil,
                    maxHeight: resolved.abr_policy.has_max_height
                        ? Int(resolved.abr_policy.max_height)
                        : nil
                )
            )
        }
    }
}

private func withRuntimeBridgeTrackPreferencePolicy<Result>(
    _ policy: VesperTrackPreferencePolicy,
    _ body: (UnsafePointer<VesperRuntimeTrackPreferencePolicy>) -> Result
) -> Result {
    withOptionalTrackPreferenceCString(policy.preferredAudioLanguage) { preferredAudioLanguage in
        withOptionalTrackPreferenceCString(policy.preferredSubtitleLanguage) {
            preferredSubtitleLanguage in
            withOptionalTrackPreferenceCString(policy.audioSelection.trackId) { audioTrackId in
                withOptionalTrackPreferenceCString(policy.subtitleSelection.trackId) {
                    subtitleTrackId in
                    withOptionalTrackPreferenceCString(policy.abrPolicy.trackId) { abrTrackId in
                        var payload = VesperRuntimeTrackPreferencePolicy(
                            preferred_audio_language: UnsafeMutablePointer(mutating: preferredAudioLanguage),
                            preferred_subtitle_language: UnsafeMutablePointer(
                                mutating: preferredSubtitleLanguage
                            ),
                            select_subtitles_by_default: policy.selectSubtitlesByDefault,
                            select_undetermined_subtitle_language: policy
                                .selectUndeterminedSubtitleLanguage,
                            audio_selection: VesperRuntimeTrackSelection(
                                mode_ordinal: runtimeBridgeOrdinal(policy.audioSelection.mode),
                                track_id: audioTrackId
                            ),
                            subtitle_selection: VesperRuntimeTrackSelection(
                                mode_ordinal: runtimeBridgeOrdinal(policy.subtitleSelection.mode),
                                track_id: subtitleTrackId
                            ),
                            abr_policy: VesperRuntimeAbrPolicy(
                                mode_ordinal: runtimeBridgeOrdinal(policy.abrPolicy.mode),
                                track_id: abrTrackId,
                                has_max_bit_rate: policy.abrPolicy.maxBitRate != nil,
                                max_bit_rate: policy.abrPolicy.maxBitRate ?? 0,
                                has_max_width: policy.abrPolicy.maxWidth != nil,
                                max_width: Int32(policy.abrPolicy.maxWidth ?? 0),
                                has_max_height: policy.abrPolicy.maxHeight != nil,
                                max_height: Int32(policy.abrPolicy.maxHeight ?? 0)
                            )
                        )
                        return withUnsafePointer(to: &payload, body)
                    }
                }
            }
        }
    }
}

private func withOptionalTrackPreferenceCString<Result>(
    _ value: String?,
    _ body: (UnsafePointer<CChar>?) -> Result
) -> Result {
    guard let value else {
        return body(nil)
    }

    return value.withCString(body)
}

private func optionalRuntimeBridgeString(
    _ value: UnsafePointer<CChar>?
) -> String? {
    guard let value else {
        return nil
    }
    return String(cString: value)
}

private func runtimeBridgeOrdinal(_ mode: VesperTrackSelectionMode) -> Int32 {
    switch mode {
    case .auto:
        0
    case .disabled:
        1
    case .track:
        2
    }
}

private func runtimeBridgeOrdinal(_ mode: VesperAbrMode) -> Int32 {
    switch mode {
    case .auto:
        0
    case .constrained:
        1
    case .fixedTrack:
        2
    }
}
