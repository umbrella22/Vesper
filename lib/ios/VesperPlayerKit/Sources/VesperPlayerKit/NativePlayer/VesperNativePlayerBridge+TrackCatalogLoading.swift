@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func refreshTrackCatalogAndSelection(for item: AVPlayerItem) {
        Task { [weak self, weak item] in
            guard let self, let item else { return }
            let trackState = await self.loadTrackCatalogState(for: item)
            guard self.player?.currentItem === item else { return }
            self.audioGroup = trackState.audioGroup
            self.subtitleGroup = trackState.subtitleGroup
            self.videoVariantPinsByTrackId = trackState.videoVariantPinsByTrackId
            self.audioOptionsByTrackId = trackState.audioOptionsByTrackId
            self.subtitleOptionsByTrackId = trackState.subtitleOptionsByTrackId
            self.publishedTrackCatalog = trackState.catalog
            self.applyDefaultTrackPreferencesIfNeeded(for: item)
            self.applyPendingResilienceRestore(ifNeededFor: item, phase: .trackSelection)
            self.refreshEffectiveVideoTrackObservation(for: item)
        }
    }

    func loadTrackCatalogState(for item: AVPlayerItem) async -> LoadedTrackCatalogState {
        let asset = item.asset
        let audibleGroup = await loadMediaSelectionGroup(for: .audible, asset: asset)
        let legibleGroup = await loadMediaSelectionGroup(for: .legible, asset: asset)
        let dashManifestCatalog = await loadDashManifestTrackCatalogSnapshot()
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

        if let legibleGroup {
            for (index, option) in legibleGroup.options.enumerated() {
                let trackId = "subtitle:\(index)"
                let dashSubtitleMetadata = dashManifestCatalog?.subtitleMetadata(at: index)
                subtitleOptionsByTrackId[trackId] = option
                tracks.append(
                    VesperMediaTrack(
                        id: trackId,
                        kind: .subtitle,
                        label: option.displayName.isEmpty
                            ? dashSubtitleMetadata?.label
                            : option.displayName,
                        language: option.extendedLanguageTag ?? option.locale?.identifier
                            ?? dashSubtitleMetadata?.language,
                        codec: dashSubtitleMetadata?.codec,
                        bitRate: dashSubtitleMetadata?.bitRate,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: dashSubtitleMetadata?.channels,
                        sampleRate: dashSubtitleMetadata?.sampleRate,
                        isDefault: legibleGroup.defaultOption == option,
                        isForced: option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
                    )
                )
            }
        } else if let dashManifestCatalog {
            tracks.append(contentsOf: dashManifestCatalog.subtitleTracks)
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
            subtitleOptionsByTrackId: subtitleOptionsByTrackId
        )
    }

    func loadDashManifestTrackCatalogSnapshot() async -> VesperDashManifestTrackCatalogSnapshot? {
        guard currentSource?.protocol == .dash, let currentDashSession else {
            return nil
        }
        return try? await currentDashSession.manifestTrackCatalogSnapshot()
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
