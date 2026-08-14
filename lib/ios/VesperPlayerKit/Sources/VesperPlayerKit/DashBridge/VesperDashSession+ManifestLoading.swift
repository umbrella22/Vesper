@preconcurrency import AVFoundation
import Foundation
internal import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func manifestTypeSnapshot() -> VesperDashManifestType? {
        manifest?.type
    }

    func mediaSegments(
        for playable: VesperDashPlayableRepresentation,
        segmentBase: VesperDashSegmentBase
    ) async throws -> [VesperDashMediaSegment] {
        if let cached = mediaSegmentsByRenditionId[playable.renditionId] {
            return cached
        }
        let sidx = try await loadSidx(for: playable)
        let segments = try VesperDashHlsBuilder.mediaSegments(
            segmentBase: segmentBase,
            sidx: sidx
        )
        mediaSegmentsByRenditionId[playable.renditionId] = segments
        return segments
    }

    func templateSegments(
        for playable: VesperDashPlayableRepresentation,
        manifest: VesperDashManifest,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> [VesperDashTemplateSegment] {
        if let cached = templateSegmentsByRenditionId[playable.renditionId] {
            return cached
        }
        let segments = try VesperDashHlsBuilder.templateSegments(
            manifestType: manifest.type,
            durationMs: manifest.durationMs,
            segmentTemplate: segmentTemplate
        )
        templateSegmentsByRenditionId[playable.renditionId] = segments
        return segments
    }

    func loadManifest() async throws -> VesperDashManifest {
        if let manifest, !shouldRefreshManifest(manifest) {
            return manifest
        }
        if let manifestLoadTask {
            return try await manifestLoadTask.value
        }

        let task = Task { try await self.fetchManifestFromNetwork() }
        manifestLoadTask = task
        let parsed: VesperDashManifest
        do {
            parsed = try await task.value
            manifestLoadTask = nil
        } catch {
            manifestLoadTask = nil
            throw error
        }
        if manifest != nil, parsed.type == .dynamic {
            clearManifestDerivedCaches()
        }
        manifest = parsed
        manifestLoadedAt = Date()
        return parsed
    }

    func fetchManifestFromNetwork() async throws -> VesperDashManifest {
        let data = try await networkClient.data(for: sourceURL)
        return try VesperDashManifestParser.parse(data: data, manifestURL: sourceURL)
    }

    func shouldRefreshManifest(_ manifest: VesperDashManifest) -> Bool {
        guard manifest.type == .dynamic else {
            return false
        }
        guard let manifestLoadedAt else {
            return true
        }
        let refreshIntervalMs = max(
            manifest.minimumUpdatePeriodMs ?? Self.defaultDynamicManifestRefreshIntervalMs,
            Self.minimumDynamicManifestRefreshIntervalMs
        )
        return Date().timeIntervalSince(manifestLoadedAt) * 1_000 >= Double(refreshIntervalMs)
    }

    func clearManifestDerivedCaches() {
        masterPlaylistCache = nil
        mediaPlaylistCacheByRenditionId = [:]
        selectedPlayableByPolicy = [:]
        playableByRenditionId = [:]
        videoDecodeCapabilitiesCache = nil
        sidxByRenditionId = [:]
        mediaSegmentsByRenditionId = [:]
        templateSegmentsByRenditionId = [:]
        mediaPlaylistTasksByRenditionId = [:]
        sidxLoadTasksByRenditionId = [:]
        segmentDownloadTasksByKey = [:]
    }

    func playableRepresentation(renditionId: String) async throws -> VesperDashPlayableRepresentation {
        if let cached = playableByRenditionId[renditionId] {
            return cached
        }
        let manifest = try await loadManifest()
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )
        guard let playable = (selected.audio + selected.video + selected.subtitles).first(where: {
            $0.renditionId == renditionId
        }) else {
            throw VesperDashBridgeError.invalidManifest(
                "missing DASH representation for rendition \(renditionId)"
            )
        }
        return playable
    }

    func loadSidx(for playable: VesperDashPlayableRepresentation) async throws -> VesperDashSidxBox {
        if let cached = sidxByRenditionId[playable.renditionId] {
            return cached
        }
        if let inFlightTask = sidxLoadTasksByRenditionId[playable.renditionId] {
            return try await inFlightTask.value
        }
        guard let segmentBase = playable.representation.segmentBase else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase"
            )
        }
        let task = Task {
            try await self.fetchSidx(
                playable: playable,
                segmentBase: segmentBase
            )
        }
        sidxLoadTasksByRenditionId[playable.renditionId] = task
        do {
            let sidx = try await task.value
            sidxLoadTasksByRenditionId[playable.renditionId] = nil
            sidxByRenditionId[playable.renditionId] = sidx
            return sidx
        } catch {
            sidxLoadTasksByRenditionId[playable.renditionId] = nil
            throw error
        }
    }

    func fetchSidx(
        playable: VesperDashPlayableRepresentation,
        segmentBase: VesperDashSegmentBase
    ) async throws -> VesperDashSidxBox {
        guard let mediaURL = URL(string: playable.representation.baseURL) else {
            throw VesperDashBridgeError.invalidManifest(
                "invalid media URL \(playable.representation.baseURL)"
            )
        }
        let data = try await networkClient.data(for: mediaURL, byteRange: segmentBase.indexRange)
        return try VesperDashSidxParser.parse(data: data)
    }
}
