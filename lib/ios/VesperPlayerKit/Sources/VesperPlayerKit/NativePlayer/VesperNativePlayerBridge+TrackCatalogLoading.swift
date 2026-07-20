@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func refreshTrackCatalogAndSelection(for item: AVPlayerItem) {
        Task { [weak self, weak item] in
            guard let self, let item else { return }
            guard self.player?.currentItem === item else { return }
            self.publishedSubtitleState = .loading(
                advertisedTrackCount: self.publishedSubtitleState.advertisedTrackCount
            )
            let trackState = await self.loadTrackCatalogState(for: item)
            guard self.player?.currentItem === item else { return }
            self.audioGroup = trackState.audioGroup
            self.subtitleGroup = trackState.subtitleGroup
            self.videoVariantPinsByTrackId = trackState.videoVariantPinsByTrackId
            self.audioOptionsByTrackId = trackState.audioOptionsByTrackId
            self.subtitleOptionsByTrackId = trackState.subtitleOptionsByTrackId
            self.publishedTrackCatalog = trackState.catalog
            self.publishedSubtitleState = trackState.subtitleState
            self.applyDefaultTrackPreferencesIfNeeded(for: item)
            self.enforceSubtitleVisibility(for: item)
            self.applyPendingResilienceRestore(ifNeededFor: item, phase: .trackSelection)
            self.refreshEffectiveVideoTrackObservation(for: item)
        }
    }

    func loadTrackCatalogState(for item: AVPlayerItem) async -> LoadedTrackCatalogState {
        let asset = item.asset
        let audibleGroup = await loadMediaSelectionGroup(for: .audible, asset: asset)
        let legibleGroup = await loadMediaSelectionGroup(for: .legible, asset: asset)
        let dashManifestCatalog = await loadDashManifestTrackCatalogSnapshot()
        let manifestLoadFailure: VesperSubtitleState? =
            currentSource?.protocol == .dash && currentDashSession != nil && dashManifestCatalog == nil
                ? publishedSubtitleState
                : nil
        let videoVariantState: LoadedVideoVariantState
        if let dashManifestCatalog {
            videoVariantState = LoadedVideoVariantState(
                tracks: dashManifestCatalog.videoTracks,
                pinsByTrackId: dashManifestCatalog.videoVariantPinsByTrackId
            )
        } else {
            videoVariantState = await loadVideoVariantState(for: asset)
        }

        var tracks = videoVariantState.tracks
        var audioOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
        var subtitleOptionsByTrackId: [String: AVMediaSelectionOption] = [:]

        if let audibleGroup {
            for (index, option) in audibleGroup.options.enumerated() {
                let trackId = "audio:\(index)"
                let dashAudioMetadata = dashManifestCatalog?.audioMetadata(at: index)
                audioOptionsByTrackId[trackId] = option
                tracks.append(
                    VesperMediaTrack(
                        id: trackId,
                        kind: .audio,
                        label: option.displayName.isEmpty
                            ? dashAudioMetadata?.label
                            : option.displayName,
                        language: option.extendedLanguageTag ?? option.locale?.identifier
                            ?? dashAudioMetadata?.language,
                        codec: dashAudioMetadata?.codec,
                        bitRate: dashAudioMetadata?.bitRate,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: dashAudioMetadata?.channels,
                        sampleRate: dashAudioMetadata?.sampleRate,
                        isDefault: audibleGroup.defaultOption == option,
                        isForced: false
                    )
                )
            }
        } else if let dashManifestCatalog {
            tracks.append(contentsOf: dashManifestCatalog.audioTracks)
        }

        // Subtitle catalog population rules:
        // - When the AV legible group exists AND the source is DASH, each
        //   legible option is matched to a DASH manifest descriptor by
        //   (normalized language, label, forced). The track id is the
        //   descriptor's stable `subtitle:dash:<representation id>` so
        //   catalog ids survive source refresh and track reorder.
        // - When the AV legible group exists but the source is NOT DASH
        //   (HLS / MP4 / progressive with embedded CEA-608 / WebVTT), the
        //   legacy index-based id `subtitle:<index>` is used. These sources
        //   do not carry a manifest descriptor catalog, so AVPlayer's
        //   option list is the only source of truth. Skipping them would
        //   silently drop all non-DASH subtitle selection.
        // - When the AV legible group is absent but the manifest advertised
        //   subtitles, the catalog stays empty and the subtitle state
        //   becomes `failed/subtitle_platform_track_unavailable`. We must
        //   not publish manifest-only descriptors as selectable tracks.
        // - Side-loaded subtitle tracks (external SRT/WebVTT/SSA) are still
        //   appended because they go through a separate overlay renderer
        //   path that does not require an AV legible group.
        var subtitleState = manifestLoadFailure ?? VesperSubtitleState.unavailable()
        let advertisedSubtitleCount = dashManifestCatalog?.subtitleTracks.count ?? 0
        let isDashSource = currentSource?.protocol == .dash
        if let legibleGroup {
            if isDashSource, let descriptors = dashManifestCatalog?.subtitleTracks {
                // DASH path: match each AV option to a manifest descriptor
                // so the catalog id is the stable rendition id.
                var identityAmbiguous = false
                var matched = 0
                // Track which descriptors have already been claimed by a
                // previous option. Two AV options binding to the same
                // descriptor is an identity ambiguity because the reverse
                // mapping must also be unique.
                var claimedDescriptorIds = Set<String>()
                for option in legibleGroup.options {
                    let match = matchedSubtitleDescriptor(
                        for: option,
                        in: descriptors,
                        claimedDescriptorIds: claimedDescriptorIds
                    )
                    switch match {
                    case .none:
                        // No descriptor matches this AV option. Skip it;
                        // the plan does not require us to publish unknown
                        // AV options under synthesized ids.
                        continue
                    case .ambiguous:
                        identityAmbiguous = true
                        continue
                    case let .unique(descriptor):
                        let trackId = descriptor.id
                        claimedDescriptorIds.insert(trackId)
                        subtitleOptionsByTrackId[trackId] = option
                        tracks.append(
                            VesperMediaTrack(
                                id: trackId,
                                kind: .subtitle,
                                label: option.displayName.isEmpty
                                    ? descriptor.label
                                    : option.displayName,
                                language: option.extendedLanguageTag ?? option.locale?.identifier
                                    ?? descriptor.language,
                                codec: descriptor.codec,
                                bitRate: descriptor.bitRate,
                                width: nil,
                                height: nil,
                                frameRate: nil,
                                channels: descriptor.channels,
                                sampleRate: descriptor.sampleRate,
                                isDefault: descriptor.isDefault,
                                isForced: option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
                                    || descriptor.isForced
                            )
                        )
                        matched += 1
                    }
                }
                if identityAmbiguous {
                    subtitleState = .failed(
                        advertisedTrackCount: advertisedSubtitleCount,
                        code: "subtitle_track_identity_ambiguous",
                        phase: .identity,
                        message: "subtitle AV option(s) could not be uniquely matched to manifest descriptors"
                    )
                } else if matched == 0 && advertisedSubtitleCount > 0 {
                    // Asset loaded and group exists, but no option aligned
                    // with any descriptor. Treat as platform discovery
                    // failure.
                    subtitleState = .failed(
                        advertisedTrackCount: advertisedSubtitleCount,
                        code: "subtitle_platform_track_unavailable",
                        phase: .discovery,
                        message: "manifest advertised subtitles but no AV legible option matched any descriptor"
                    )
                } else if matched == 0 {
                    subtitleState = .unavailable()
                } else {
                    subtitleState = .ready(
                        advertisedTrackCount: advertisedSubtitleCount,
                        selectableTrackCount: matched
                    )
                }
            } else if !isDashSource {
                // Non-DASH path (HLS / MP4 / progressive): preserve the
                // legacy index-based id so embedded subtitles remain
                // selectable. Stable manifest identity is scoped to DASH;
                // other source protocols keep their existing behavior.
                for (index, option) in legibleGroup.options.enumerated() {
                    let trackId = "subtitle:\(index)"
                    subtitleOptionsByTrackId[trackId] = option
                    tracks.append(
                        VesperMediaTrack(
                            id: trackId,
                            kind: .subtitle,
                            label: option.displayName,
                            language: option.extendedLanguageTag ?? option.locale?.identifier,
                            codec: nil,
                            bitRate: nil,
                            width: nil,
                            height: nil,
                            frameRate: nil,
                            channels: nil,
                            sampleRate: nil,
                            isDefault: legibleGroup.defaultOption == option,
                            isForced: option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
                        )
                    )
                }
                if legibleGroup.options.isEmpty {
                    subtitleState = .unavailable()
                } else {
                    subtitleState = .ready(
                        advertisedTrackCount: legibleGroup.options.count,
                        selectableTrackCount: legibleGroup.options.count
                    )
                }
            }
        } else if advertisedSubtitleCount > 0 {
            // Do not append `dashManifestCatalog.subtitleTracks` to the public
            // catalog when there is no legible group. The
            // catalog must only contain options that can actually be
            // selected. Surface a structured failure instead.
            subtitleState = .failed(
                advertisedTrackCount: advertisedSubtitleCount,
                code: "subtitle_platform_track_unavailable",
                phase: .discovery,
                message: "manifest advertised subtitles but AVAsset published no legible media selection group"
            )
        }

        if let currentSource {
            for (index, configuration) in currentSource.subtitleConfigurations.enumerated() {
                tracks.append(
                    VesperMediaTrack(
                        id: VesperSubtitleOverlayRenderer.trackId(for: index),
                        kind: .subtitle,
                        label: configuration.label ?? "External Subtitle \(index + 1)",
                        language: configuration.language,
                        codec: configuration.mimeType.rawMime,
                        bitRate: nil,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: nil,
                        sampleRate: nil,
                        isDefault: index == 0,
                        isForced: false
                    )
                )
            }
        }

        return LoadedTrackCatalogState(
            catalog: VesperTrackCatalog(
                tracks: tracks,
                adaptiveVideo: dashManifestCatalog?.adaptiveVideo
                    ?? (videoVariantState.tracks.count > 1),
                adaptiveAudio: dashManifestCatalog?.adaptiveAudio ?? false
            ),
            audioGroup: audibleGroup,
            subtitleGroup: legibleGroup,
            videoVariantPinsByTrackId: videoVariantState.pinsByTrackId,
            audioOptionsByTrackId: audioOptionsByTrackId,
            subtitleOptionsByTrackId: subtitleOptionsByTrackId,
            subtitleState: subtitleState
        )
    }

    /// Result of matching an AV legible option to a DASH manifest subtitle
    /// descriptor.
    private enum SubtitleDescriptorMatch {
        /// Exactly one descriptor matches; use its stable id and metadata.
        case unique(VesperMediaTrack)
        /// Two or more descriptors match the same key (ambiguous identity).
        case ambiguous
        /// No descriptor matches.
        case none
    }

    /// Matches an AV legible option to a DASH manifest subtitle descriptor
    /// using the normalized `(language, label, forced)` key. The catalog id
    /// comes from the descriptor, not the AV option index, because ordering is not
    /// guaranteed to align with the manifest adaptation-set ordering.
    /// Descriptors claimed by a previous option are reported as ambiguous if
    /// another AV option resolves to the same descriptor.
    private func matchedSubtitleDescriptor(
        for option: AVMediaSelectionOption,
        in descriptors: [VesperMediaTrack],
        claimedDescriptorIds: Set<String>
    ) -> SubtitleDescriptorMatch {
        let optionLanguage = subtitleMatchLanguagePrimary(option.extendedLanguageTag ?? option.locale?.identifier)
        let optionForced = option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
        let optionLabel = option.displayName
        var matches: [VesperMediaTrack] = []
        for descriptor in descriptors {
            let descriptorLanguage = subtitleMatchLanguagePrimary(descriptor.language)
            let languageMatches = optionLanguage == descriptorLanguage
            let forcedMatches = descriptor.isForced == optionForced
            let descriptorLabel = descriptor.label?
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased() ?? ""
            let normalizedOptionLabel = optionLabel
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased()
            let labelMatches = descriptorLabel == normalizedOptionLabel
            if languageMatches && forcedMatches && labelMatches {
                matches.append(descriptor)
            }
        }
        switch matches.count {
        case 0:
            return .none
        case 1:
            if claimedDescriptorIds.contains(matches[0].id) {
                return .ambiguous
            }
            return .unique(matches[0])
        default:
            return .ambiguous
        }
    }

    /// Normalizes a language tag without dropping region or script subtags.
    /// An empty language remains distinct from a declared language.
    private func subtitleMatchLanguagePrimary(_ value: String?) -> String {
        guard let value else { return "" }
        return value
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "_", with: "-")
            .lowercased()
    }

    /// Loads the DASH manifest track catalog snapshot and surfaces
    /// structured subtitle failures instead of silently swallowing them.
    /// The previous `try?` shape hid manifest parse errors, identity
    /// ambiguity, and multiple-default-subtitle failures behind a `nil`,
    /// which caused `loadTrackCatalogState` to fall through to an empty
    /// catalog with no `subtitleState` signal. These failures surface as
    /// `subtitle_manifest_parse_failed` /
    /// `subtitle_track_identity_ambiguous` / `subtitle_default_track_ambiguous`.
    func loadDashManifestTrackCatalogSnapshot() async -> VesperDashManifestTrackCatalogSnapshot? {
        guard let source = currentSource,
              source.protocol == .dash,
              let session = currentDashSession
        else {
            return nil
        }
        do {
            let snapshot = try await session.manifestTrackCatalogSnapshot()
            guard currentSource == source, currentDashSession === session else {
                return nil
            }
            return snapshot
        } catch {
            guard currentSource == source, currentDashSession === session else {
                return nil
            }
            let message = (error as? LocalizedError)?.errorDescription
                ?? String(describing: error)
            let code: String
            let phase: VesperSubtitleErrorPhase
            if message.contains("subtitle_track_identity_ambiguous") {
                code = "subtitle_track_identity_ambiguous"
                phase = .identity
            } else if message.contains("subtitle_default_track_ambiguous") {
                code = "subtitle_default_track_ambiguous"
                phase = .identity
            } else {
                code = "subtitle_manifest_parse_failed"
                phase = .manifest
            }
            publishedSubtitleState = .failed(
                advertisedTrackCount: 0,
                code: code,
                phase: phase,
                message: "DASH subtitle manifest loading failed"
            )
            iosHostLog("dashManifestCatalog failed code=\(code) phase=\(phase.rawValue)")
            return nil
        }
    }

    func loadVideoVariantState(for asset: AVAsset) async -> LoadedVideoVariantState {
        guard sourceSupportsVideoVariantCatalog(currentSource) else {
            return .empty
        }
        guard #available(iOS 15.0, *) else {
            return .empty
        }
        guard let urlAsset = asset as? AVURLAsset else {
            return .empty
        }

        let variants = (try? await urlAsset.load(.variants)) ?? []
        guard !variants.isEmpty else {
            return .empty
        }

        let groupedVariants = Dictionary(
            grouping: variants.compactMap(LoadedVideoVariantDescriptor.init)
        ) { descriptor in
            descriptor.deduplicationKey
        }
        let deduplicatedVariants = groupedVariants.values.compactMap { descriptors in
            descriptors.max(by: { left, right in
                LoadedVideoVariantDescriptor.preferredOrdering(
                    left,
                    over: right
                ) == right
            })
        }
        .sorted { left, right in
            if left == right {
                return false
            }
            return LoadedVideoVariantDescriptor.preferredOrdering(left, over: right) == left
        }

        var tracks: [VesperMediaTrack] = []
        var pinsByTrackId: [String: LoadedVideoVariantPin] = [:]
        tracks.reserveCapacity(deduplicatedVariants.count)
        pinsByTrackId.reserveCapacity(deduplicatedVariants.count)

        for (index, descriptor) in deduplicatedVariants.enumerated() {
            let trackId = descriptor.stableTrackId
            tracks.append(
                VesperMediaTrack(
                    id: trackId,
                    kind: .video,
                    label: descriptor.trackLabel,
                    language: nil,
                    codec: descriptor.codec,
                    bitRate: descriptor.peakBitRate,
                    width: descriptor.width,
                    height: descriptor.height,
                    frameRate: descriptor.frameRate,
                    channels: nil,
                    sampleRate: nil,
                    isDefault: index == 0,
                    isForced: false
                )
            )
            pinsByTrackId[trackId] = LoadedVideoVariantPin(
                peakBitRate: descriptor.peakBitRate.map(Double.init),
                maxWidth: descriptor.width,
                maxHeight: descriptor.height
            )
        }

        return LoadedVideoVariantState(
            tracks: tracks,
            pinsByTrackId: pinsByTrackId
        )
    }

    func loadMediaSelectionGroup(
        for characteristic: AVMediaCharacteristic,
        asset: AVAsset
    ) async -> AVMediaSelectionGroup? {
        return try? await asset.loadMediaSelectionGroup(for: characteristic)
    }
}
