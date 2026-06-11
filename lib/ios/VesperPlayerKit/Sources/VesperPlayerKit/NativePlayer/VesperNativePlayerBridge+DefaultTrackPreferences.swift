@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

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
            applyTrackSelection(
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
        guard let group = subtitleGroup else {
            return
        }

        let policy = resolvedTrackPreferencePolicy
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
            applyTrackSelection(
                policy.subtitleSelection,
                kind: .subtitle,
                group: group,
                optionsByTrackId: subtitleOptionsByTrackId,
                item: item
            )
        case .auto:
            let option =
                matchingMediaOption(
                    language: policy.preferredSubtitleLanguage,
                    optionsByTrackId: subtitleOptionsByTrackId
                )?.option
                ?? (policy.selectUndeterminedSubtitleLanguage
                    ? firstUndeterminedMediaOption(optionsByTrackId: subtitleOptionsByTrackId)
                    : nil)
                ?? (policy.selectSubtitlesByDefault ? group.defaultOption : nil)
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
