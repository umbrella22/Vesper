@preconcurrency import AVFoundation
import Foundation
import CryptoKit
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func refreshTrackCatalogAndSelection(for item: AVPlayerItem) {
        trackCatalogLoadGeneration &+= 1
        let capturedGeneration = trackCatalogLoadGeneration
        let capturedSourceEpoch = subtitleSourceEpoch
        let capturedPlaybackEpoch = currentPlaybackEpoch()
        Task { [weak self, weak item] in
            guard let self, let item else { return }
            guard self.isCurrentSubtitleCatalogLoad(
                item: item,
                sourceEpoch: capturedSourceEpoch,
                playbackEpoch: capturedPlaybackEpoch,
                generation: capturedGeneration
            ) else { return }
            self.publishedSubtitleState = self.publishedSubtitleState.replacingCatalog(
                with: .loading(
                    advertisedTrackCount: self.publishedSubtitleState.advertisedTrackCount
                )
            )
            let trackState = await self.loadTrackCatalogState(
                for: item,
                sourceEpoch: capturedSourceEpoch
            )
            guard self.isCurrentSubtitleCatalogLoad(
                item: item,
                sourceEpoch: capturedSourceEpoch,
                playbackEpoch: capturedPlaybackEpoch,
                generation: capturedGeneration
            ) else { return }
            self.audioGroup = trackState.audioGroup
            self.subtitleGroup = trackState.subtitleGroup
            self.videoVariantPinsByTrackId = trackState.videoVariantPinsByTrackId
            self.audioOptionsByTrackId = trackState.audioOptionsByTrackId
            self.subtitleOptionsByTrackId = trackState.subtitleOptionsByTrackId
            self.publishedTrackCatalog = trackState.catalog
            self.publishedSubtitleState = self.publishedSubtitleState.replacingCatalog(
                with: trackState.subtitleState
            )
            await self.applyDefaultTrackPreferencesIfNeeded(for: item)
            self.enforceSubtitleVisibility(for: item)
            await self.applyPendingResilienceRestore(ifNeededFor: item, phase: .trackSelection)
            self.refreshEffectiveVideoTrackObservation(for: item)
        }
    }

    private func isCurrentSubtitleCatalogLoad(
        item: AVPlayerItem,
        sourceEpoch: UInt64,
        playbackEpoch: UInt64,
        generation: UInt64
    ) -> Bool {
        subtitleSourceEpoch == sourceEpoch
            && currentPlaybackEpoch() == playbackEpoch
            && player?.currentItem === item
            && trackCatalogLoadGeneration == generation
    }

    func loadTrackCatalogState(
        for item: AVPlayerItem,
        sourceEpoch: UInt64? = nil
    ) async -> LoadedTrackCatalogState {
        let asset = item.asset
        let audibleGroup = await loadMediaSelectionGroup(for: .audible, asset: asset)
        let legibleGroup = await loadMediaSelectionGroup(for: .legible, asset: asset)
        let dashManifestCatalog = await loadDashManifestTrackCatalogSnapshot(
            sourceEpoch: sourceEpoch
        )
        let hlsManifestLoad = await loadHlsSubtitleManifestState(sourceEpoch: sourceEpoch)
        let hlsManifestCatalog = hlsManifestLoad.snapshot
        let manifestLoadFailure: VesperSubtitleState? = {
            if currentSource?.protocol == .dash,
               currentDashSession != nil,
               dashManifestCatalog == nil {
                return publishedSubtitleState
            }
            if let error = hlsManifestLoad.error {
                return VesperSubtitleState(
                    catalogState: .failed,
                    selectionState: .idle,
                    advertisedTrackCount: 0,
                    selectableTrackCount: 0,
                    catalogError: error,
                    selectionError: nil
                )
            }
            return nil
        }()
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
        //   option's canonical metadata provides a stable opaque id. An
        //   identity collision fails discovery instead of falling back to
        //   an order-dependent position.
        // - When the AV legible group is absent but the manifest advertised
        //   subtitles, the catalog stays empty and the subtitle state
        //   becomes `failed/subtitle_platform_track_unavailable`. We must
        //   not publish manifest-only descriptors as selectable tracks.
        // - Side-loaded subtitle tracks (external SRT/WebVTT/SSA) are still
        //   appended because they go through a separate overlay renderer
        //   path that does not require an AV legible group.
        var subtitleState = manifestLoadFailure ?? VesperSubtitleState.unavailable()
        let manifestIdentityFailure = hlsManifestLoad.error.map {
            $0.code == "subtitle_track_identity_ambiguous"
                || $0.code == "subtitle_default_track_ambiguous"
        } ?? false
        var nativeIdentityAmbiguous = manifestIdentityFailure
        let existingSubtitleResourceError = publishedSubtitleState.catalogError?.phase == .resource
            ? publishedSubtitleState.catalogError
            : nil
        let advertisedSubtitleCount: Int = {
            if let dashManifestCatalog {
                return dashManifestCatalog.advertisedSubtitleTrackCount
            }
            if let hlsManifestCatalog, hlsManifestCatalog.isMasterPlaylist {
                return hlsManifestCatalog.advertisedTrackCount
            }
            if currentSource?.protocol == .hls {
                return 0
            }
            return legibleGroup?.options.count ?? 0
        }()
        let isDashSource = currentSource?.protocol == .dash
        let isHlsSource = currentSource?.protocol == .hls
        let hlsSubtitleDescriptors = hlsManifestCatalog?.renditions.map { rendition in
            VesperMediaTrack(
                id: rendition.id,
                kind: .subtitle,
                label: rendition.name,
                language: rendition.language,
                codec: nil,
                bitRate: nil,
                width: nil,
                height: nil,
                frameRate: nil,
                channels: nil,
                sampleRate: nil,
                isDefault: rendition.isDefault,
                isForced: rendition.isForced
            )
        }
        let hlsGroupPairs: [(String, String)] =
            hlsManifestCatalog?.renditions.map { ($0.id, $0.groupId) } ?? []
        let hlsGroupIdByDescriptorId = Dictionary(uniqueKeysWithValues: hlsGroupPairs)
        let manifestSubtitleDescriptors = dashManifestCatalog?.subtitleTracks
            ?? hlsSubtitleDescriptors
        if let legibleGroup {
            if let descriptors = manifestSubtitleDescriptors,
               isDashSource || (isHlsSource && !descriptors.isEmpty) {
                // Manifest-backed path: match each AV option to a descriptor
                // so the catalog id remains stable across option reordering.
                var selectableDescriptors = descriptors.filter {
                    !failedSubtitleTrackIds.contains($0.id)
                }
                var identityAmbiguous = false
                var matched = 0
                if isHlsSource {
                    let optionKeys = legibleGroup.options.map {
                        hlsSubtitleMatchKey(for: $0)
                    }
                    let descriptorIdentities: [VesperHlsSubtitleDescriptorIdentity] =
                        selectableDescriptors.compactMap { descriptor in
                        guard let groupId = hlsGroupIdByDescriptorId[descriptor.id] else {
                            return nil
                        }
                        return VesperHlsSubtitleDescriptorIdentity(
                            id: descriptor.id,
                            groupId: groupId,
                            key: hlsSubtitleMatchKey(for: descriptor)
                        )
                        }
                    switch resolveHlsSubtitleDescriptorGroup(
                        optionKeys: optionKeys,
                        descriptors: descriptorIdentities
                    ) {
                    case .none:
                        selectableDescriptors = []
                    case let .unique(ids):
                        selectableDescriptors = selectableDescriptors.filter {
                            ids.contains($0.id)
                        }
                    case .ambiguous:
                        identityAmbiguous = true
                        selectableDescriptors = []
                    }
                }
                // Track which descriptors have already been claimed by a
                // previous option. Two AV options binding to the same
                // descriptor is an identity ambiguity because the reverse
                // mapping must also be unique.
                var claimedDescriptorIds = Set<String>()
                for option in legibleGroup.options {
                    let match = matchedSubtitleDescriptor(
                        for: option,
                        in: selectableDescriptors,
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
                    nativeIdentityAmbiguous = true
                    subtitleState = .failed(
                        advertisedTrackCount: advertisedSubtitleCount,
                        code: "subtitle_track_identity_ambiguous",
                        phase: .identity,
                        message: "subtitle AV option(s) could not be uniquely matched to manifest descriptors"
                    )
                } else if matched == 0 && !failedSubtitleTrackIds.isEmpty {
                    subtitleState = .failed(
                        advertisedTrackCount: advertisedSubtitleCount,
                        code: existingSubtitleResourceError?.code ?? "subtitle_resource_failed",
                        phase: existingSubtitleResourceError?.phase ?? .resource,
                        trackId: existingSubtitleResourceError?.trackId,
                        retriable: existingSubtitleResourceError?.retriable ?? true,
                        message: existingSubtitleResourceError?.message
                            ?? "DASH subtitle resource loading failed"
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
                    if let existingSubtitleResourceError {
                        subtitleState = VesperSubtitleState(
                            catalogState: .ready,
                            selectionState: subtitleState.selectionState,
                            advertisedTrackCount: subtitleState.advertisedTrackCount,
                            selectableTrackCount: subtitleState.selectableTrackCount,
                            catalogError: existingSubtitleResourceError,
                            selectionError: subtitleState.selectionError
                        )
                    }
                }
            } else if !isDashSource {
                let identifiedOptions = legibleGroup.options.map { option in
                    let identity = embeddedSubtitleIdentity(for: option)
                    return (option: option, identity: identity)
                }
                let identities = Dictionary(grouping: identifiedOptions) { entry in
                    entry.identity.trackId
                }
                if identities.values.contains(where: { $0.count > 1 }) {
                    nativeIdentityAmbiguous = true
                    subtitleState = .failed(
                        advertisedTrackCount: advertisedSubtitleCount,
                        code: "subtitle_track_identity_ambiguous",
                        phase: .identity,
                        message: "multiple AV legible options have the same canonical subtitle identity"
                    )
                } else {
                    for entry in identifiedOptions {
                        subtitleOptionsByTrackId[entry.identity.trackId] = entry.option
                        tracks.append(
                            VesperMediaTrack(
                                id: entry.identity.trackId,
                                kind: .subtitle,
                                label: entry.option.displayName,
                                language: entry.option.extendedLanguageTag
                                    ?? entry.option.locale?.identifier,
                                codec: entry.identity.codec,
                                bitRate: nil,
                                width: nil,
                                height: nil,
                                frameRate: nil,
                                channels: nil,
                                sampleRate: nil,
                                isDefault: legibleGroup.defaultOption == entry.option,
                                isForced: entry.identity.isForced
                            )
                        )
                    }
                }
                if identifiedOptions.isEmpty {
                    subtitleState = manifestLoadFailure ?? .unavailable()
                } else if !nativeIdentityAmbiguous {
                    let manifestError = manifestLoadFailure?.catalogError
                    subtitleState = VesperSubtitleState(
                        catalogState: .ready,
                        selectionState: .idle,
                        advertisedTrackCount: advertisedSubtitleCount,
                        selectableTrackCount: identifiedOptions.count,
                        catalogError: manifestError,
                        selectionError: nil
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

        let declaredExternalSubtitles = currentSource?.externalSubtitles ?? []
        let sideLoadAdvertisedCount = declaredExternalSubtitles.count
        let sideLoadTrackIds = Set(subtitleOverlayRenderer.loadedTrackIds)
        let nativeSubtitleIds = Set(subtitleOptionsByTrackId.keys)
        let declaredExternalIds = Set(declaredExternalSubtitles.map(\.id))
        let conflictingExternalIds = declaredExternalIds.intersection(nativeSubtitleIds)
        var externalCatalogError: VesperSubtitleError?
        if let conflictingExternalId = conflictingExternalIds.sorted().first {
            externalCatalogError = VesperSubtitleError(
                code: "subtitle_track_identity_ambiguous",
                phase: .identity,
                trackId: conflictingExternalId,
                retriable: false,
                message: "native and external subtitle identities must be unique"
            )
        }
        let nativeDefaultCount =
            tracks.filter {
                $0.kind == .subtitle && nativeSubtitleIds.contains($0.id) && $0.isDefault
            }.count
        let externalDefaultCount = declaredExternalSubtitles.filter(\.isDefault).count
        let externalDefaultAmbiguous = externalDefaultCount > 1
        let nativeDefaultAmbiguous = !isHlsSource && nativeDefaultCount > 1
        if externalDefaultAmbiguous {
            externalCatalogError = VesperSubtitleError(
                code: "subtitle_default_track_ambiguous",
                phase: .identity,
                trackId: nil,
                retriable: false,
                message: "a subtitle group may contain at most one default track"
            )
        }
        if let currentSource {
            for configuration in currentSource.externalSubtitles
                where sideLoadTrackIds.contains(configuration.id) &&
                    !conflictingExternalIds.contains(configuration.id) &&
                    !externalDefaultAmbiguous {
                tracks.append(
                    VesperMediaTrack(
                        id: configuration.id,
                        kind: .subtitle,
                        label: configuration.label ?? configuration.id,
                        language: configuration.language,
                        codec: configuration.mimeType,
                        bitRate: nil,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: nil,
                        sampleRate: nil,
                        isDefault: configuration.isDefault,
                        isForced: configuration.isForced
                    )
                )
            }
        }

        let externalIdentityAmbiguous =
            !conflictingExternalIds.isEmpty || externalDefaultAmbiguous
        if nativeIdentityAmbiguous || nativeDefaultAmbiguous {
            tracks.removeAll {
                $0.kind == .subtitle && nativeSubtitleIds.contains($0.id)
            }
            subtitleOptionsByTrackId.removeAll()
        }
        if externalIdentityAmbiguous {
            tracks.removeAll { conflictingExternalIds.contains($0.id) ||
                (externalDefaultAmbiguous && declaredExternalIds.contains($0.id)) }
        }

        let nativeSelectableSubtitleCount = subtitleOptionsByTrackId.count
        let selectableExternalSubtitleCount = externalIdentityAmbiguous
            ? 0
            : sideLoadTrackIds.subtracting(conflictingExternalIds).count
        let selectableSubtitleCount =
            nativeSelectableSubtitleCount + selectableExternalSubtitleCount
        let combinedAdvertisedSubtitleCount = advertisedSubtitleCount + sideLoadAdvertisedCount
        let catalogError = externalCatalogError
            ?? subtitleState.catalogError
            ?? pendingSubtitleOverlayFailure?.error
        if selectableSubtitleCount > 0 {
            subtitleState = VesperSubtitleState(
                catalogState: .ready,
                selectionState: .idle,
                advertisedTrackCount: combinedAdvertisedSubtitleCount,
                selectableTrackCount: selectableSubtitleCount,
                catalogError: catalogError,
                selectionError: nil
            )
        } else if combinedAdvertisedSubtitleCount > 0, let catalogError {
            subtitleState = VesperSubtitleState(
                catalogState: .failed,
                selectionState: .idle,
                advertisedTrackCount: combinedAdvertisedSubtitleCount,
                selectableTrackCount: 0,
                catalogError: catalogError,
                selectionError: nil
            )
        } else if combinedAdvertisedSubtitleCount > 0, subtitleState.status == .unavailable {
            subtitleState = .loading(advertisedTrackCount: combinedAdvertisedSubtitleCount)
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

    /// HLS media groups are an internal matching boundary. AVFoundation does
    /// not expose GROUP-ID on AVMediaSelectionOption, so a complete option set
    /// must identify exactly one manifest group before descriptor matching.
    struct VesperHlsSubtitleMatchKey: Hashable {
        let language: String
        let label: String
        let isForced: Bool
    }

    struct VesperHlsSubtitleDescriptorIdentity: Hashable {
        let id: String
        let groupId: String
        let key: VesperHlsSubtitleMatchKey
    }

    enum VesperHlsSubtitleDescriptorGroupResolution: Equatable {
        case none
        case unique(Set<String>)
        case ambiguous
    }

    /// Resolves an AV option set to one HLS group without using positional
    /// order. A group must explain every option that any group can explain;
    /// disjoint or tied explanations are platform identity ambiguity.
    func resolveHlsSubtitleDescriptorGroup(
        optionKeys: [VesperHlsSubtitleMatchKey],
        descriptors: [VesperHlsSubtitleDescriptorIdentity]
    ) -> VesperHlsSubtitleDescriptorGroupResolution {
        let grouped = Dictionary(grouping: descriptors, by: \.groupId)
        var candidates: [(ids: Set<String>, optionIndexes: Set<Int>)] = []

        for groupDescriptors in grouped.values {
            var ids = Set<String>()
            var optionIndexes = Set<Int>()
            var groupIsAmbiguous = false
            for (index, optionKey) in optionKeys.enumerated() {
                let matches = groupDescriptors.filter { $0.key == optionKey }
                if matches.count > 1 {
                    groupIsAmbiguous = true
                    break
                }
                if let match = matches.first {
                    ids.insert(match.id)
                    optionIndexes.insert(index)
                }
            }
            if !groupIsAmbiguous && !optionIndexes.isEmpty {
                candidates.append((ids: ids, optionIndexes: optionIndexes))
            }
        }

        let allMatchedIndexes = candidates.reduce(into: Set<Int>()) {
            $0.formUnion($1.optionIndexes)
        }
        guard !allMatchedIndexes.isEmpty else { return .none }
        let coveringCandidates = candidates.filter {
            $0.optionIndexes == allMatchedIndexes
        }
        guard coveringCandidates.count == 1 else { return .ambiguous }
        return .unique(coveringCandidates[0].ids)
    }

    private func hlsSubtitleMatchKey(
        for option: AVMediaSelectionOption
    ) -> VesperHlsSubtitleMatchKey {
        VesperHlsSubtitleMatchKey(
            language: subtitleMatchLanguagePrimary(
                option.extendedLanguageTag ?? option.locale?.identifier
            ),
            label: option.displayName
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased(),
            isForced: option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
        )
    }

    private func hlsSubtitleMatchKey(
        for descriptor: VesperMediaTrack
    ) -> VesperHlsSubtitleMatchKey {
        VesperHlsSubtitleMatchKey(
            language: subtitleMatchLanguagePrimary(descriptor.language),
            label: descriptor.label?
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased() ?? "",
            isForced: descriptor.isForced
        )
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

    private func embeddedSubtitleIdentity(
        for option: AVMediaSelectionOption
    ) -> EmbeddedSubtitleIdentity {
        let codecValues = option.mediaSubTypes
            .map(\.uint32Value)
            .sorted()
        let characteristics = stableSubtitleMediaCharacteristics.compactMap { characteristic in
            option.hasMediaCharacteristic(characteristic) ? characteristic.rawValue : nil
        }
        let language = subtitleMatchLanguagePrimary(
            option.extendedLanguageTag ?? option.locale?.identifier
        )
        let isForced = option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
        return EmbeddedSubtitleIdentity(
            trackId: stableEmbeddedSubtitleTrackId(
                language: language.isEmpty ? nil : language,
                label: option.displayName,
                isForced: isForced,
                codecValues: codecValues,
                characteristics: characteristics
            ),
            codec: codecValues.first.map(fourCharCodeString),
            isForced: isForced
        )
    }

    /// Loads the DASH manifest track catalog snapshot and surfaces
    /// structured subtitle failures instead of silently swallowing them.
    /// The previous `try?` shape hid manifest parse errors, identity
    /// ambiguity, and multiple-default-subtitle failures behind a `nil`,
    /// which caused `loadTrackCatalogState` to fall through to an empty
    /// catalog with no `subtitleState` signal. These failures surface as
    /// `subtitle_manifest_parse_failed` /
    /// `subtitle_track_identity_ambiguous` / `subtitle_default_track_ambiguous`.
    func loadDashManifestTrackCatalogSnapshot(
        sourceEpoch: UInt64? = nil
    ) async -> VesperDashManifestTrackCatalogSnapshot? {
        guard let source = currentSource,
              source.protocol == .dash,
              let session = currentDashSession,
              sourceEpoch.map({ $0 == subtitleSourceEpoch }) ?? true
        else {
            return nil
        }
        do {
            let snapshot = try await session.manifestTrackCatalogSnapshot()
            guard currentSource == source,
                  currentDashSession === session,
                  sourceEpoch.map({ $0 == subtitleSourceEpoch }) ?? true
            else {
                return nil
            }
            return snapshot
        } catch {
            guard currentSource == source,
                  currentDashSession === session,
                  sourceEpoch.map({ $0 == subtitleSourceEpoch }) ?? true
            else {
                return nil
            }
            let details: VesperDashSubtitleErrorDetails
            if case let VesperDashBridgeError.subtitle(subtitleDetails) = error {
                details = subtitleDetails
            } else {
                details = VesperDashSubtitleErrorDetails(
                    code: "subtitle_manifest_parse_failed",
                    phase: VesperSubtitleErrorPhase.manifest.rawValue,
                    trackId: nil,
                    retriable: false,
                    message: "DASH subtitle manifest loading failed"
                )
            }
            let phase = VesperSubtitleErrorPhase(rawValue: details.phase) ?? .unknown
            publishedSubtitleState = publishedSubtitleState.replacingCatalog(
                with: .failed(
                    advertisedTrackCount: publishedSubtitleState.advertisedTrackCount,
                    code: details.code,
                    phase: phase,
                    trackId: details.trackId,
                    retriable: details.retriable,
                    message: details.message,
                    phaseRawValue: phase == .unknown ? details.phase : nil
                )
            )
            iosHostLog("dashManifestCatalog failed code=\(details.code) phase=\(details.phase)")
            return nil
        }
    }

    private func loadHlsSubtitleManifestState(
        sourceEpoch: UInt64?
    ) async -> HlsSubtitleManifestLoadState {
        guard let source = currentSource, source.protocol == .hls else {
            return HlsSubtitleManifestLoadState(snapshot: nil, error: nil)
        }
        guard let url = URL(string: source.uri) else {
            return HlsSubtitleManifestLoadState(
                snapshot: nil,
                error: VesperSubtitleError(
                    code: "subtitle_uri_invalid",
                    phase: .resource,
                    trackId: nil,
                    retriable: false,
                    message: "HLS manifest URI is invalid"
                )
            )
        }
        do {
            let snapshot = try await VesperHlsSubtitleManifestInspector.load(
                from: url,
                headers: source.headers
            )
            guard currentSource == source,
                  sourceEpoch.map({ $0 == subtitleSourceEpoch }) ?? true
            else {
                return HlsSubtitleManifestLoadState(snapshot: nil, error: nil)
            }
            return HlsSubtitleManifestLoadState(snapshot: snapshot, error: nil)
        } catch {
            guard currentSource == source,
                  sourceEpoch.map({ $0 == subtitleSourceEpoch }) ?? true
            else {
                return HlsSubtitleManifestLoadState(snapshot: nil, error: nil)
            }
            let details = hlsSubtitleManifestErrorDetails(error)
            return HlsSubtitleManifestLoadState(snapshot: nil, error: details)
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

private struct EmbeddedSubtitleIdentity {
    let trackId: String
    let codec: String?
    let isForced: Bool
}

private let stableSubtitleMediaCharacteristics: [AVMediaCharacteristic] = [
    .containsOnlyForcedSubtitles,
    .transcribesSpokenDialogForAccessibility,
    .describesMusicAndSoundForAccessibility,
    .easyToRead,
    .describesVideoForAccessibility,
    .isMainProgramContent,
    .isAuxiliaryContent,
    .isOriginalContent,
    .languageTranslation,
    .dubbedTranslation,
    .voiceOverTranslation,
]

func stableEmbeddedSubtitleTrackId(
    language: String?,
    label: String?,
    isForced: Bool,
    codecValues: [UInt32],
    characteristics: [String]
) -> String {
    var payload = Data()
    appendStableSubtitleIdentityValue(language, to: &payload)
    appendStableSubtitleIdentityValue(label, to: &payload)
    appendStableSubtitleIdentityValue(isForced ? "1" : "0", to: &payload)
    appendStableSubtitleIdentityValues(
        codecValues.sorted().map { String(format: "%08X", $0) },
        to: &payload
    )
    appendStableSubtitleIdentityValues(characteristics.sorted(), to: &payload)
    let encoded = payload.base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
    return "subtitle:av:\(encoded)"
}

private func appendStableSubtitleIdentityValues(
    _ values: [String],
    to payload: inout Data
) {
    appendStableSubtitleIdentityBytes("l\(values.count):", to: &payload)
    for value in values {
        appendStableSubtitleIdentityValue(value, to: &payload)
    }
}

private func appendStableSubtitleIdentityValue(
    _ value: String?,
    to payload: inout Data
) {
    guard let value else {
        appendStableSubtitleIdentityBytes("n", to: &payload)
        return
    }
    let bytes = Data(value.utf8)
    appendStableSubtitleIdentityBytes("v\(bytes.count):", to: &payload)
    payload.append(bytes)
}

private func appendStableSubtitleIdentityBytes(
    _ value: String,
    to payload: inout Data
) {
    payload.append(contentsOf: value.utf8)
}

// MARK: - HLS subtitle manifest inspection

private struct HlsSubtitleManifestLoadState {
    let snapshot: VesperHlsSubtitleManifestSnapshot?
    let error: VesperSubtitleError?
}

private func hlsSubtitleManifestErrorDetails(_ error: Error) -> VesperSubtitleError {
    switch error {
    case let VesperHlsSubtitleManifestError.identity(message):
        return VesperSubtitleError(
            code: "subtitle_track_identity_ambiguous",
            phase: .identity,
            trackId: nil,
            retriable: false,
            message: message
        )
    case let VesperHlsSubtitleManifestError.duplicateDefault(message):
        return VesperSubtitleError(
            code: "subtitle_default_track_ambiguous",
            phase: .identity,
            trackId: nil,
            retriable: false,
            message: message
        )
    case let VesperHlsSubtitleManifestError.invalid(message):
        return VesperSubtitleError(
            code: "subtitle_manifest_parse_failed",
            phase: .manifest,
            trackId: nil,
            retriable: false,
            message: message
        )
    case let VesperHlsSubtitleManifestError.resource(message):
        return VesperSubtitleError(
            code: "subtitle_resource_failed",
            phase: .resource,
            trackId: nil,
            retriable: true,
            message: message
        )
    case VesperHlsSubtitleManifestError.tooLarge:
        return VesperSubtitleError(
            code: "subtitle_resource_failed",
            phase: .resource,
            trackId: nil,
            retriable: false,
            message: "HLS manifest exceeds the 2 MiB inspection limit"
        )
    default:
        return VesperSubtitleError(
            code: "subtitle_resource_failed",
            phase: .resource,
            trackId: nil,
            retriable: true,
            message: "HLS subtitle manifest inspection failed"
        )
    }
}

/// A subtitle rendition declared by an HLS master playlist.
struct VesperHlsSubtitleRendition: Equatable {
    let id: String
    let groupId: String
    let name: String
    let uri: String
    let language: String?
    let isDefault: Bool
    let isForced: Bool
}

struct VesperHlsSubtitleManifestSnapshot: Equatable {
    let isMasterPlaylist: Bool
    let renditions: [VesperHlsSubtitleRendition]

    var advertisedTrackCount: Int {
        isMasterPlaylist ? renditions.count : 0
    }
}

enum VesperHlsSubtitleManifestError: Error, Equatable {
    case invalid(String)
    case resource(String)
    case identity(String)
    case duplicateDefault(String)
    case tooLarge
}

/// Strict, bounded HLS master inspection used for subtitle catalog metadata.
/// The download planner has a separate tolerant parser and is deliberately not
/// reused here because catalog identity and default validation are contract
/// failures, not optional planning hints.
enum VesperHlsSubtitleManifestInspector {
    static let maxManifestBytes = 2 * 1024 * 1024
    static let requestTimeout: TimeInterval = 5

    static func parse(_ text: String) throws -> VesperHlsSubtitleManifestSnapshot {
        let lines = text.components(separatedBy: .newlines)
        guard lines.first?.trimmingCharacters(in: .whitespacesAndNewlines) == "#EXTM3U" else {
            throw VesperHlsSubtitleManifestError.invalid("HLS playlist is missing #EXTM3U")
        }

        let hasMasterMarker = lines.contains { line in
            line.hasPrefix("#EXT-X-STREAM-INF:") || line.hasPrefix("#EXT-X-MEDIA:")
        }
        guard hasMasterMarker else {
            return VesperHlsSubtitleManifestSnapshot(isMasterPlaylist: false, renditions: [])
        }

        var renditions: [VesperHlsSubtitleRendition] = []
        var defaultsByGroup: [String: Int] = [:]
        var namesByGroup: [String: Set<String>] = [:]
        for line in lines where line.hasPrefix("#EXT-X-MEDIA:") {
            let attributes = try parseAttributeList(String(line.dropFirst("#EXT-X-MEDIA:".count)))
            guard let type = attributes["TYPE"] else {
                throw VesperHlsSubtitleManifestError.invalid("EXT-X-MEDIA is missing TYPE")
            }
            guard type == "SUBTITLES" else { continue }
            guard let groupId = attributes["GROUP-ID"], !groupId.isEmpty,
                  let name = attributes["NAME"], !name.isEmpty,
                  let uri = attributes["URI"], !uri.isEmpty
            else {
                throw VesperHlsSubtitleManifestError.invalid(
                    "subtitle rendition requires GROUP-ID, NAME, and URI"
                )
            }
            if namesByGroup[groupId, default: []].contains(name) {
                throw VesperHlsSubtitleManifestError.identity(
                    "duplicate subtitle NAME in group \(groupId)"
                )
            }
            namesByGroup[groupId, default: []].insert(name)
            let isDefault = try parseBoolean(attributes["DEFAULT"] ?? "NO", field: "DEFAULT")
            let isForced = try parseBoolean(attributes["FORCED"] ?? "NO", field: "FORCED")
            if isDefault {
                let count = defaultsByGroup[groupId, default: 0] + 1
                defaultsByGroup[groupId] = count
                if count > 1 {
                    throw VesperHlsSubtitleManifestError.duplicateDefault(
                        "subtitle group \(groupId) declares multiple DEFAULT renditions"
                    )
                }
            }
            let language = attributes["LANGUAGE"]
            let id = opaqueHlsSubtitleId(
                groupId: groupId,
                name: name
            )
            if renditions.contains(where: { $0.id == id }) {
                throw VesperHlsSubtitleManifestError.identity(
                    "subtitle rendition identity is not unique"
                )
            }
            renditions.append(
                VesperHlsSubtitleRendition(
                    id: id,
                    groupId: groupId,
                    name: name,
                    uri: uri,
                    language: language,
                    isDefault: isDefault,
                    isForced: isForced
                )
            )
        }
        return VesperHlsSubtitleManifestSnapshot(
            isMasterPlaylist: true,
            renditions: renditions
        )
    }

    static func load(
        from url: URL,
        headers: [String: String] = [:]
    ) async throws -> VesperHlsSubtitleManifestSnapshot {
        let data: Data
        if url.isFileURL {
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            var bounded = Data()
            while let chunk = try handle.read(upToCount: 64 * 1024), !chunk.isEmpty {
                bounded.append(chunk)
                if bounded.count > maxManifestBytes {
                    throw VesperHlsSubtitleManifestError.tooLarge
                }
            }
            data = bounded
        } else {
            var request = URLRequest(url: url)
            request.timeoutInterval = requestTimeout
            for (name, value) in headers {
                request.setValue(value, forHTTPHeaderField: name)
            }
            let (bytes, response) = try await URLSession.shared.bytes(for: request)
            if let httpResponse = response as? HTTPURLResponse,
               !(200..<300).contains(httpResponse.statusCode) {
                throw VesperHlsSubtitleManifestError.resource(
                    "HLS manifest request returned HTTP \(httpResponse.statusCode)"
                )
            }
            var bounded = Data()
            for try await byte in bytes {
                bounded.append(byte)
                if bounded.count > maxManifestBytes {
                    throw VesperHlsSubtitleManifestError.tooLarge
                }
            }
            data = bounded
        }
        guard let text = String(data: data, encoding: .utf8) else {
            throw VesperHlsSubtitleManifestError.invalid("HLS manifest is not UTF-8")
        }
        return try parse(text)
    }

    private static func parseAttributeList(_ raw: String) throws -> [String: String] {
        var parts: [String] = []
        var current = ""
        var quoted = false
        for character in raw {
            if character == "\"" {
                quoted.toggle()
                current.append(character)
            } else if character == ",", !quoted {
                parts.append(current)
                current.removeAll(keepingCapacity: true)
            } else {
                current.append(character)
            }
        }
        guard !quoted else {
            throw VesperHlsSubtitleManifestError.invalid("unterminated HLS attribute quote")
        }
        parts.append(current)

        var attributes: [String: String] = [:]
        for part in parts {
            guard let separator = part.firstIndex(of: "=") else {
                throw VesperHlsSubtitleManifestError.invalid("malformed HLS attribute")
            }
            let key = String(part[..<separator]).trimmingCharacters(in: .whitespaces)
                .uppercased()
            var value = String(part[part.index(after: separator)...])
                .trimmingCharacters(in: .whitespaces)
            guard !key.isEmpty, !value.isEmpty else {
                throw VesperHlsSubtitleManifestError.invalid("malformed HLS attribute")
            }
            if value.first == "\"" {
                guard value.last == "\"", value.count >= 2 else {
                    throw VesperHlsSubtitleManifestError.invalid("unterminated HLS attribute value")
                }
                value.removeFirst()
                value.removeLast()
            } else if value.contains("\"") {
                throw VesperHlsSubtitleManifestError.invalid("malformed HLS attribute value")
            }
            guard attributes.updateValue(value, forKey: key) == nil else {
                throw VesperHlsSubtitleManifestError.invalid("duplicate HLS attribute \(key)")
            }
        }
        return attributes
    }

    private static func parseBoolean(_ raw: String, field: String) throws -> Bool {
        switch raw.uppercased() {
        case "YES": return true
        case "NO": return false
        default:
            throw VesperHlsSubtitleManifestError.invalid("HLS \(field) must be YES or NO")
        }
    }
}

private func opaqueHlsSubtitleId(
    groupId: String,
    name: String
) -> String {
    // URI and descriptive metadata may change during a master refresh. The
    // HLS group/name pair is already validated as unique and is the stable
    // rendition identity required by the public opaque id contract.
    let canonical = [groupId, name]
        .map { "\($0.utf8.count):\($0)" }
        .joined(separator: "|")
    let digest = SHA256.hash(data: Data(canonical.utf8))
    return "hls-" + digest.map { String(format: "%02x", $0) }.joined()
}
