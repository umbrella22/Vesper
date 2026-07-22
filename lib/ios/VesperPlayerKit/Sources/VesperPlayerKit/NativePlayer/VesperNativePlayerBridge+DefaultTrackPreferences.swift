@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func applyDefaultTrackPreferencesIfNeeded(for item: AVPlayerItem) {
        guard !hasAppliedDefaultTrackPreferences else {
            return
        }

        hasAppliedDefaultTrackPreferences = true
        applyDefaultAudioTrackPreferenceIfPossible(item: item)
        applyDefaultSubtitleTrackPreferenceIfPossible(item: item)
        applyAbrPolicy(
            resolvedTrackPreferencePolicy.abrPolicy,
            origin: .defaultPolicy,
            clearLastReportedError: false
        )
    }

    func applyDefaultAudioTrackPreferenceIfPossible(item: AVPlayerItem) {
        guard let group = audioGroup else {
            return
        }

        let policy = resolvedTrackPreferencePolicy
        switch policy.audioSelection.mode {
        case .disabled:
            item.select(nil, in: group)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: .disabled(),
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            }
        case .track:
            try? applyTrackSelection(
                policy.audioSelection,
                kind: .audio,
                group: group,
                optionsByTrackId: audioOptionsByTrackId,
                item: item
            )
        case .auto:
            if
                let match = matchingMediaOption(
                    language: policy.preferredAudioLanguage,
                    optionsByTrackId: audioOptionsByTrackId
                )
            {
                item.select(match.option, in: group)
            } else {
                item.selectMediaOptionAutomatically(in: group)
            }
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: .auto(),
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }

    func applyDefaultSubtitleTrackPreferenceIfPossible(item: AVPlayerItem) {
        let policy = resolvedTrackPreferencePolicy
        if subtitleOverlayRenderer.hasTracks {
            let sideLoadTrackId: String?
            switch policy.subtitleSelection.mode {
            case .disabled:
                sideLoadTrackId = nil
            case .track:
                sideLoadTrackId = policy.subtitleSelection.trackId.flatMap { trackId in
                    subtitleOverlayRenderer.containsTrack(trackId) ? trackId : nil
                }
                if sideLoadTrackId == nil, subtitleGroup != nil {
                    break
                }
            case .auto:
                let preferredLanguage = policy.preferredSubtitleLanguage?.lowercased()
                let preferredIndex = currentSource?.subtitleConfigurations.firstIndex { configuration in
                    configuration.language?.lowercased() == preferredLanguage
                }
                if let preferredIndex {
                    sideLoadTrackId = VesperSubtitleOverlayRenderer.trackId(for: preferredIndex)
                } else if policy.selectSubtitlesByDefault {
                    sideLoadTrackId = subtitleOverlayRenderer.firstTrackId()
                } else {
                    sideLoadTrackId = nil
                }
            }
            if policy.subtitleSelection.mode != .track || sideLoadTrackId != nil || subtitleGroup == nil {
                if let group = subtitleGroup {
                    item.select(nil, in: group)
                }
                _ = subtitleOverlayRenderer.select(trackId: sideLoadTrackId)
                updateTrackSelection { current in
                    VesperTrackSelectionSnapshot(
                        video: current.video,
                        audio: current.audio,
                        subtitle: sideLoadTrackId == nil ? .disabled() : policy.subtitleSelection,
                        abrPolicy: current.abrPolicy
                    )
                }
                return
            }
        }

        guard let group = subtitleGroup else {
            return
        }
        _ = subtitleOverlayRenderer.select(trackId: nil)

        switch policy.subtitleSelection.mode {
        case .disabled:
            item.select(nil, in: group)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: .disabled(),
                    abrPolicy: current.abrPolicy
                )
            }
        case .track:
            try? applyTrackSelection(
                policy.subtitleSelection,
                kind: .subtitle,
                group: group,
                optionsByTrackId: subtitleOptionsByTrackId,
                item: item
            )
        case .auto:
            let option = automaticSubtitleOption(
                in: group,
                optionsByTrackId: subtitleOptionsByTrackId
            )
            item.select(option, in: group)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: option == nil ? .disabled() : .auto(),
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }

    func automaticSubtitleOption(
        in group: AVMediaSelectionGroup,
        optionsByTrackId: [String: AVMediaSelectionOption]
    ) -> AVMediaSelectionOption? {
        let policy = resolvedTrackPreferencePolicy
        return matchingMediaOption(
            language: policy.preferredSubtitleLanguage,
            optionsByTrackId: optionsByTrackId
        )?.option
            ?? (policy.selectUndeterminedSubtitleLanguage
                ? firstUndeterminedMediaOption(optionsByTrackId: optionsByTrackId)
                : nil)
            ?? (policy.selectSubtitlesByDefault ? group.defaultOption : nil)
    }

    func matchingMediaOption(
        language: String?,
        optionsByTrackId: [String: AVMediaSelectionOption]
    ) -> (trackId: String, option: AVMediaSelectionOption)? {
        guard let normalizedLanguage = normalizedLanguageIdentifier(language) else {
            return nil
        }

        return optionsByTrackId.first { _, option in
            let candidates = [
                option.extendedLanguageTag,
                option.locale?.identifier,
            ]
            return candidates.contains { candidate in
                guard let normalizedCandidate = normalizedLanguageIdentifier(candidate) else {
                    return false
                }
                return normalizedCandidate == normalizedLanguage ||
                    normalizedCandidate.hasPrefix(normalizedLanguage + "-") ||
                    normalizedLanguage.hasPrefix(normalizedCandidate + "-")
            }
        }.map { (trackId: $0.key, option: $0.value) }
    }

    func firstUndeterminedMediaOption(
        optionsByTrackId: [String: AVMediaSelectionOption]
    ) -> AVMediaSelectionOption? {
        optionsByTrackId.values.first { option in
            normalizedLanguageIdentifier(option.extendedLanguageTag) == nil &&
                normalizedLanguageIdentifier(option.locale?.identifier) == nil
        }
    }

    func normalizedLanguageIdentifier(_ value: String?) -> String? {
        guard let value else {
            return nil
        }

        let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "_", with: "-")
            .lowercased()
        guard !normalized.isEmpty, normalized != "und" else {
            return nil
        }
        return normalized
    }
}
