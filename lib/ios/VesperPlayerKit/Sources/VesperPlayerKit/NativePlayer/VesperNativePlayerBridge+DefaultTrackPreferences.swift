@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func applyDefaultTrackPreferencesIfNeeded(for item: AVPlayerItem) async {
        guard !hasAppliedDefaultTrackPreferences else {
            return
        }

        hasAppliedDefaultTrackPreferences = true
        applyDefaultAudioTrackPreferenceIfPossible(item: item)
        await applyDefaultSubtitleTrackPreferenceIfPossible(item: item)
        try? applyAbrPolicy(
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
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
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
                    confirmedSubtitle: current.confirmedSubtitle,
                    effectiveSubtitleTrackId: current.effectiveSubtitleTrackId,
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }

    func applyDefaultSubtitleTrackPreferenceIfPossible(item: AVPlayerItem) async {
        guard player?.currentItem === item else { return }
        if explicitSubtitleIntentSourceEpoch == subtitleSourceEpoch,
           publishedRequestedSubtitleSelection.mode == .disabled {
            // An explicit disable may have arrived before AVFoundation exposed
            // the legible group. Apply that intent when the group becomes
            // available instead of leaving the platform default selected.
            if let group = subtitleGroup {
                item.select(nil, in: group)
            }
            return
        }
        do {
            try await coordinateSubtitleSelection(
                resolvedTrackPreferencePolicy.subtitleSelection,
                origin: .defaultPolicy
            )
        } catch {
            iosHostLog("default subtitle selection failed: \(error.localizedDescription)")
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
        normalizedSubtitleLanguageIdentifier(value)
    }
}

func resolveAutomaticSubtitleTrackId(
    tracks: [VesperMediaTrack],
    preferredLanguage: String?,
    selectUndeterminedLanguage: Bool,
    allowDefaultCandidate: Bool
) -> String? {
    let candidates = tracks.filter { $0.kind == .subtitle }
    guard !candidates.isEmpty else { return nil }

    if let preferredLanguage = normalizedSubtitleLanguageIdentifier(preferredLanguage),
       let match = preferredAutomaticSubtitleCandidate(
           candidates.filter { track in
               guard let language = normalizedSubtitleLanguageIdentifier(track.language) else {
                   return false
               }
               return language == preferredLanguage
                   || language.hasPrefix(preferredLanguage + "-")
                   || preferredLanguage.hasPrefix(language + "-")
           }
       ) {
        return match.id
    }

    if selectUndeterminedLanguage,
       let match = preferredAutomaticSubtitleCandidate(
           candidates.filter {
               normalizedSubtitleLanguageIdentifier($0.language) == nil
           }
       ) {
        return match.id
    }

    guard allowDefaultCandidate else { return nil }
    return preferredAutomaticSubtitleCandidate(candidates.filter(\.isDefault))?.id
}

func automaticSubtitleSelectionAllowsDefaultCandidate(
    origin: SubtitleSelectionOrigin,
    startupPolicySelectsSubtitlesByDefault: Bool
) -> Bool {
    switch origin {
    case .defaultPolicy:
        return startupPolicySelectsSubtitlesByDefault
    case .explicit, .resilienceRestore, .visibilityRestore:
        return true
    }
}

private func preferredAutomaticSubtitleCandidate(
    _ candidates: [VesperMediaTrack]
) -> VesperMediaTrack? {
    candidates.sorted { lhs, rhs in
        if lhs.isDefault != rhs.isDefault {
            return lhs.isDefault
        }
        if lhs.isForced != rhs.isForced {
            return !lhs.isForced
        }
        return lhs.id < rhs.id
    }.first
}

private func normalizedSubtitleLanguageIdentifier(_ value: String?) -> String? {
    guard let value else { return nil }
    let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
        .replacingOccurrences(of: "_", with: "-")
        .lowercased()
    guard !normalized.isEmpty, normalized != "und" else { return nil }
    return normalized
}
