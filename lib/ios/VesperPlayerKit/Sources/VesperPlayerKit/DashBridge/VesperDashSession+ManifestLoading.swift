@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

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

        let task = Task {
            let parsed = try await self.fetchManifestFromNetwork()
            try self.installManifest(parsed)
            return parsed
        }
        manifestLoadTask = task
        defer { manifestLoadTask = nil }
        return try await task.value
    }

    private func installManifest(_ parsed: VesperDashManifest) throws {
        let validatedSegments = try validatePublishedLiveRenditions(in: parsed)
        if manifest != nil, parsed.type == .dynamic {
            clearManifestDerivedCaches()
            templateSegmentsByRenditionId = validatedSegments
        }
        manifest = parsed
        manifestLoadedAt = Date()
    }

    private func validatePublishedLiveRenditions(
        in candidate: VesperDashManifest
    ) throws -> [String: [VesperDashTemplateSegment]] {
        guard !liveTargetDurationByRenditionId.isEmpty else { return [:] }
        guard candidate.type == .dynamic else {
            throw VesperDashBridgeError.unsupportedManifest("DASH manifest type changed after live publication")
        }
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: candidate,
            variantPolicy: .all
        )
        var validated: [String: [VesperDashTemplateSegment]] = [:]
        for playable in selected.video + selected.audio + selected.subtitles {
            guard let target = liveTargetDurationByRenditionId[playable.renditionId] else { continue }
            guard let template = playable.representation.segmentTemplate else {
                throw VesperDashBridgeError.unsupportedManifest("Live DASH rendition stopped using SegmentTemplate")
            }
            validated[playable.renditionId] = try VesperDashHlsBuilder.templateSegments(
                manifestType: candidate.type,
                durationMs: candidate.durationMs,
                segmentTemplate: template,
                targetDuration: target
            )
        }
        return validated
    }

    func fetchManifestFromNetwork() async throws -> VesperDashManifest {
        let (data, finalURL) = try await networkClient.manifestData(for: sourceURL)
        return try VesperDashManifestParser.parse(data: data, manifestURL: finalURL)
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
        let loaded = try await loadManifest()
        return try playableRepresentation(renditionId: renditionId, manifest: manifest ?? loaded)
    }

    func playableRepresentation(
        renditionId: String,
        manifest: VesperDashManifest
    ) throws -> VesperDashPlayableRepresentation {
        if let cached = playableByRenditionId[renditionId] {
            return cached
        }
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
